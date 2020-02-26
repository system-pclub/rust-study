use crate::interface::{Compiler, Result};
use crate::util;
use crate::proc_macro_decls;

use log::{info, warn, log_enabled};
use rustc::dep_graph::DepGraph;
use rustc::hir;
use rustc::hir::lowering::lower_crate;
use rustc::hir::def_id::{CrateNum, LOCAL_CRATE};
use rustc::lint;
use rustc::middle::{self, reachable, resolve_lifetime, stability};
use rustc::middle::cstore::{CrateStore, MetadataLoader, MetadataLoaderDyn};
use rustc::ty::{self, AllArenas, ResolverOutputs, TyCtxt, GlobalCtxt};
use rustc::ty::steal::Steal;
use rustc::traits;
use rustc::util::common::{time, ErrorReported};
use rustc::session::Session;
use rustc::session::config::{self, CrateType, Input, OutputFilenames, OutputType};
use rustc::session::config::{PpMode, PpSourceMode};
use rustc::session::search_paths::PathKind;
use rustc_codegen_ssa::back::link::emit_metadata;
use rustc_codegen_utils::codegen_backend::CodegenBackend;
use rustc_codegen_utils::link::filename_for_metadata;
use rustc_data_structures::{box_region_allow_access, declare_box_region_type, parallel};
use rustc_data_structures::sync::{Lrc, ParallelIterator, par_iter};
use rustc_errors::PResult;
use rustc_incremental;
use rustc_metadata::cstore;
use rustc_mir as mir;
use rustc_parse::{parse_crate_from_file, parse_crate_from_source_str};
use rustc_passes::{self, ast_validation, hir_stats, layout_test};
use rustc_plugin_impl as plugin;
use rustc_plugin_impl::registry::Registry;
use rustc_privacy;
use rustc_resolve::{Resolver, ResolverArenas};
use rustc_traits;
use rustc_typeck as typeck;
use syntax::{self, ast, visit};
use syntax::early_buffered_lints::BufferedEarlyLint;
use syntax_expand::base::ExtCtxt;
use syntax::mut_visit::MutVisitor;
use syntax::util::node_count::NodeCounter;
use syntax::symbol::Symbol;
use syntax_pos::FileName;
use syntax_ext;

use rustc_serialize::json;
use tempfile::Builder as TempFileBuilder;

use std::{env, fs, iter, mem};
use std::any::Any;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::cell::RefCell;
use std::rc::Rc;

pub fn parse<'a>(sess: &'a Session, input: &Input) -> PResult<'a, ast::Crate> {
    sess.diagnostic()
        .set_continue_after_error(sess.opts.debugging_opts.continue_parse_after_error);
    let krate = time(sess, "parsing", || {
        let _prof_timer = sess.prof.generic_activity("parse_crate");

        match input {
            Input::File(file) => parse_crate_from_file(file, &sess.parse_sess),
            Input::Str { input, name } => {
                parse_crate_from_source_str(name.clone(), input.clone(), &sess.parse_sess)
            }
        }
    })?;

    sess.diagnostic().set_continue_after_error(true);

    if sess.opts.debugging_opts.ast_json_noexpand {
        println!("{}", json::as_json(&krate));
    }

    if sess.opts.debugging_opts.input_stats {
        println!(
            "Lines of code:             {}",
            sess.source_map().count_lines()
        );
        println!("Pre-expansion node count:  {}", count_nodes(&krate));
    }

    if let Some(ref s) = sess.opts.debugging_opts.show_span {
        syntax::show_span::run(sess.diagnostic(), s, &krate);
    }

    if sess.opts.debugging_opts.hir_stats {
        hir_stats::print_ast_stats(&krate, "PRE EXPANSION AST STATS");
    }

    Ok(krate)
}

fn count_nodes(krate: &ast::Crate) -> usize {
    let mut counter = NodeCounter::new();
    visit::walk_crate(&mut counter, krate);
    counter.count
}

declare_box_region_type!(
    pub BoxedResolver,
    for(),
    (&mut Resolver<'_>) -> (Result<ast::Crate>, ResolverOutputs)
);

/// Runs the "early phases" of the compiler: initial `cfg` processing,
/// loading compiler plugins (including those from `addl_plugins`),
/// syntax expansion, secondary `cfg` expansion, synthesis of a test
/// harness if one is to be provided, injection of a dependency on the
/// standard library and prelude, and name resolution.
///
/// Returns `None` if we're aborting after handling -W help.
pub fn configure_and_expand(
    sess: Lrc<Session>,
    lint_store: Lrc<lint::LintStore>,
    metadata_loader: Box<MetadataLoaderDyn>,
    krate: ast::Crate,
    crate_name: &str,
) -> Result<(ast::Crate, BoxedResolver)> {
    // Currently, we ignore the name resolution data structures for the purposes of dependency
    // tracking. Instead we will run name resolution and include its output in the hash of each
    // item, much like we do for macro expansion. In other words, the hash reflects not just
    // its contents but the results of name resolution on those contents. Hopefully we'll push
    // this back at some point.
    let crate_name = crate_name.to_string();
    let (result, resolver) = BoxedResolver::new(static move || {
        let sess = &*sess;
        let resolver_arenas = Resolver::arenas();
        let res = configure_and_expand_inner(
            sess,
            &lint_store,
            krate,
            &crate_name,
            &resolver_arenas,
            &*metadata_loader,
        );
        let mut resolver = match res {
            Err(v) => {
                yield BoxedResolver::initial_yield(Err(v));
                panic!()
            }
            Ok((krate, resolver)) => {
                yield BoxedResolver::initial_yield(Ok(krate));
                resolver
            }
        };
        box_region_allow_access!(for(), (&mut Resolver<'_>), (&mut resolver));
        resolver.into_outputs()
    });
    result.map(|k| (k, resolver))
}

impl BoxedResolver {
    pub fn to_resolver_outputs(resolver: Rc<RefCell<BoxedResolver>>) -> ResolverOutputs {
        match Rc::try_unwrap(resolver) {
            Ok(resolver) => resolver.into_inner().complete(),
            Err(resolver) => resolver.borrow_mut().access(|resolver| resolver.clone_outputs()),
        }
    }
}

pub fn register_plugins<'a>(
    sess: &'a Session,
    metadata_loader: &'a dyn MetadataLoader,
    register_lints: impl Fn(&Session, &mut lint::LintStore),
    mut krate: ast::Crate,
    crate_name: &str,
) -> Result<(ast::Crate, Lrc<lint::LintStore>)> {
    krate = time(sess, "attributes injection", || {
        syntax_ext::cmdline_attrs::inject(
            krate, &sess.parse_sess, &sess.opts.debugging_opts.crate_attr
        )
    });

    let (krate, features) = syntax_expand::config::features(
        krate,
        &sess.parse_sess,
        sess.edition(),
        &sess.opts.debugging_opts.allow_features,
    );
    // these need to be set "early" so that expansion sees `quote` if enabled.
    sess.init_features(features);

    let crate_types = util::collect_crate_types(sess, &krate.attrs);
    sess.crate_types.set(crate_types);

    let disambiguator = util::compute_crate_disambiguator(sess);
    sess.crate_disambiguator.set(disambiguator);
    rustc_incremental::prepare_session_directory(sess, &crate_name, disambiguator);

    if sess.opts.incremental.is_some() {
        time(sess, "garbage-collect incremental cache directory", || {
            let _prof_timer =
                sess.prof.generic_activity("incr_comp_garbage_collect_session_directories");
            if let Err(e) = rustc_incremental::garbage_collect_session_directories(sess) {
                warn!(
                    "Error while trying to garbage collect incremental \
                     compilation cache directory: {}",
                    e
                );
            }
        });
    }

    time(sess, "recursion limit", || {
        middle::recursion_limit::update_limits(sess, &krate);
    });

    let registrars = time(sess, "plugin loading", || {
        plugin::load::load_plugins(
            sess,
            metadata_loader,
            &krate,
            Some(sess.opts.debugging_opts.extra_plugins.clone()),
        )
    });

    let mut lint_store = rustc_lint::new_lint_store(
        sess.opts.debugging_opts.no_interleave_lints,
        sess.unstable_options(),
    );

    (register_lints)(&sess, &mut lint_store);

    let mut registry = Registry::new(sess, &mut lint_store, krate.span);

    time(sess, "plugin registration", || {
        for registrar in registrars {
            registry.args_hidden = Some(registrar.args);
            (registrar.fun)(&mut registry);
        }
    });

    *sess.plugin_llvm_passes.borrow_mut() = registry.llvm_passes;

    Ok((krate, Lrc::new(lint_store)))
}

fn configure_and_expand_inner<'a>(
    sess: &'a Session,
    lint_store: &'a lint::LintStore,
    mut krate: ast::Crate,
    crate_name: &str,
    resolver_arenas: &'a ResolverArenas<'a>,
    metadata_loader: &'a MetadataLoaderDyn,
) -> Result<(ast::Crate, Resolver<'a>)> {
    time(sess, "pre-AST-expansion lint checks", || {
        lint::check_ast_crate(
            sess,
            lint_store,
            &krate,
            true,
            None,
            rustc_lint::BuiltinCombinedPreExpansionLintPass::new());
    });

    let mut resolver = Resolver::new(
        sess,
        &krate,
        crate_name,
        metadata_loader,
        &resolver_arenas,
    );
    syntax_ext::register_builtin_macros(&mut resolver, sess.edition());

    krate = time(sess, "crate injection", || {
        let alt_std_name = sess.opts.alt_std_name.as_ref().map(|s| Symbol::intern(s));
        let (krate, name) = syntax_ext::standard_library_imports::inject(
            krate,
            &mut resolver,
            &sess.parse_sess,
            alt_std_name,
        );
        if let Some(name) = name {
            sess.parse_sess.injected_crate_name.set(name);
        }
        krate
    });

    util::check_attr_crate_type(&krate.attrs, &mut resolver.lint_buffer());

    // Expand all macros
    krate = time(sess, "expansion", || {
        let _prof_timer = sess.prof.generic_activity("macro_expand_crate");
        // Windows dlls do not have rpaths, so they don't know how to find their
        // dependencies. It's up to us to tell the system where to find all the
        // dependent dlls. Note that this uses cfg!(windows) as opposed to
        // targ_cfg because syntax extensions are always loaded for the host
        // compiler, not for the target.
        //
        // This is somewhat of an inherently racy operation, however, as
        // multiple threads calling this function could possibly continue
        // extending PATH far beyond what it should. To solve this for now we
        // just don't add any new elements to PATH which are already there
        // within PATH. This is basically a targeted fix at #17360 for rustdoc
        // which runs rustc in parallel but has been seen (#33844) to cause
        // problems with PATH becoming too long.
        let mut old_path = OsString::new();
        if cfg!(windows) {
            old_path = env::var_os("PATH").unwrap_or(old_path);
            let mut new_path = sess.host_filesearch(PathKind::All).search_path_dirs();
            for path in env::split_paths(&old_path) {
                if !new_path.contains(&path) {
                    new_path.push(path);
                }
            }
            env::set_var(
                "PATH",
                &env::join_paths(
                    new_path
                        .iter()
                        .filter(|p| env::join_paths(iter::once(p)).is_ok()),
                ).unwrap(),
            );
        }

        // Create the config for macro expansion
        let features = sess.features_untracked();
        let cfg = syntax_expand::expand::ExpansionConfig {
            features: Some(&features),
            recursion_limit: *sess.recursion_limit.get(),
            trace_mac: sess.opts.debugging_opts.trace_macros,
            should_test: sess.opts.test,
            ..syntax_expand::expand::ExpansionConfig::default(crate_name.to_string())
        };

        let mut ecx = ExtCtxt::new(&sess.parse_sess, cfg, &mut resolver);

        // Expand macros now!
        let krate = time(sess, "expand crate", || {
            ecx.monotonic_expander().expand_crate(krate)
        });

        // The rest is error reporting

        time(sess, "check unused macros", || {
            ecx.check_unused_macros();
        });

        let mut missing_fragment_specifiers: Vec<_> = ecx.parse_sess
            .missing_fragment_specifiers
            .borrow()
            .iter()
            .cloned()
            .collect();
        missing_fragment_specifiers.sort();

        for span in missing_fragment_specifiers {
            let lint = lint::builtin::MISSING_FRAGMENT_SPECIFIER;
            let msg = "missing fragment specifier";
            resolver.lint_buffer().buffer_lint(lint, ast::CRATE_NODE_ID, span, msg);
        }
        if cfg!(windows) {
            env::set_var("PATH", &old_path);
        }
        krate
    });

    time(sess, "maybe building test harness", || {
        syntax_ext::test_harness::inject(
            &sess.parse_sess,
            &mut resolver,
            sess.opts.test,
            &mut krate,
            sess.diagnostic(),
            &sess.features_untracked(),
            sess.panic_strategy(),
            sess.target.target.options.panic_strategy,
            sess.opts.debugging_opts.panic_abort_tests,
        )
    });

    // If we're actually rustdoc then there's no need to actually compile
    // anything, so switch everything to just looping
    let mut should_loop = sess.opts.actually_rustdoc;
    if let Some(PpMode::PpmSource(PpSourceMode::PpmEveryBodyLoops)) = sess.opts.pretty {
        should_loop |= true;
    }
    if should_loop {
        util::ReplaceBodyWithLoop::new(&mut resolver).visit_crate(&mut krate);
    }

    let has_proc_macro_decls = time(sess, "AST validation", || {
        ast_validation::check_crate(sess, &krate, &mut resolver.lint_buffer())
    });


    let crate_types = sess.crate_types.borrow();
    let is_proc_macro_crate = crate_types.contains(&config::CrateType::ProcMacro);

    // For backwards compatibility, we don't try to run proc macro injection
    // if rustdoc is run on a proc macro crate without '--crate-type proc-macro' being
    // specified. This should only affect users who manually invoke 'rustdoc', as
    // 'cargo doc' will automatically pass the proper '--crate-type' flags.
    // However, we do emit a warning, to let such users know that they should
    // start passing '--crate-type proc-macro'
    if has_proc_macro_decls && sess.opts.actually_rustdoc && !is_proc_macro_crate {
        let mut msg = sess.diagnostic().struct_warn(&"Trying to document proc macro crate \
            without passing '--crate-type proc-macro to rustdoc");

        msg.warn("The generated documentation may be incorrect");
        msg.emit()
    } else {
        krate = time(sess, "maybe creating a macro crate", || {
            let num_crate_types = crate_types.len();
            let is_test_crate = sess.opts.test;
            syntax_ext::proc_macro_harness::inject(
                &sess.parse_sess,
                &mut resolver,
                krate,
                is_proc_macro_crate,
                has_proc_macro_decls,
                is_test_crate,
                num_crate_types,
                sess.diagnostic(),
            )
        });
    }

    // Done with macro expansion!

    if sess.opts.debugging_opts.input_stats {
        println!("Post-expansion node count: {}", count_nodes(&krate));
    }

    if sess.opts.debugging_opts.hir_stats {
        hir_stats::print_ast_stats(&krate, "POST EXPANSION AST STATS");
    }

    if sess.opts.debugging_opts.ast_json {
        println!("{}", json::as_json(&krate));
    }

    time(sess, "name resolution", || {
        resolver.resolve_crate(&krate);
    });

    // Needs to go *after* expansion to be able to check the results of macro expansion.
    time(sess, "complete gated feature checking", || {
        syntax::feature_gate::check_crate(
            &krate,
            &sess.parse_sess,
            &sess.features_untracked(),
            sess.opts.unstable_features,
        );
    });

    // Add all buffered lints from the `ParseSess` to the `Session`.
    sess.parse_sess.buffered_lints.with_lock(|buffered_lints| {
        info!("{} parse sess buffered_lints", buffered_lints.len());
        for BufferedEarlyLint{id, span, msg, lint_id} in buffered_lints.drain(..) {
            let lint = lint::Lint::from_parser_lint_id(lint_id);
            resolver.lint_buffer().buffer_lint(lint, id, span, &msg);
        }
    });

    Ok((krate, resolver))
}

pub fn lower_to_hir(
    sess: &Session,
    lint_store: &lint::LintStore,
    resolver: &mut Resolver<'_>,
    dep_graph: &DepGraph,
    krate: &ast::Crate,
) -> Result<hir::map::Forest> {
    // Lower AST to HIR.
    let hir_forest = time(sess, "lowering AST -> HIR", || {
        let nt_to_tokenstream = rustc_parse::nt_to_tokenstream;
        let hir_crate = lower_crate(sess, &dep_graph, &krate, resolver, nt_to_tokenstream);

        if sess.opts.debugging_opts.hir_stats {
            hir_stats::print_hir_stats(&hir_crate);
        }

        hir::map::Forest::new(hir_crate, &dep_graph)
    });

    time(sess, "early lint checks", || {
        lint::check_ast_crate(
            sess,
            lint_store,
            &krate,
            false,
            Some(std::mem::take(resolver.lint_buffer())),
            rustc_lint::BuiltinCombinedEarlyLintPass::new(),
        )
    });

    // Discard hygiene data, which isn't required after lowering to HIR.
    if !sess.opts.debugging_opts.keep_hygiene_data {
        syntax_pos::hygiene::clear_syntax_context_map();
    }

    Ok(hir_forest)
}

// Returns all the paths that correspond to generated files.
fn generated_output_paths(
    sess: &Session,
    outputs: &OutputFilenames,
    exact_name: bool,
    crate_name: &str,
) -> Vec<PathBuf> {
    let mut out_filenames = Vec::new();
    for output_type in sess.opts.output_types.keys() {
        let file = outputs.path(*output_type);
        match *output_type {
            // If the filename has been overridden using `-o`, it will not be modified
            // by appending `.rlib`, `.exe`, etc., so we can skip this transformation.
            OutputType::Exe if !exact_name => for crate_type in sess.crate_types.borrow().iter() {
                let p = ::rustc_codegen_utils::link::filename_for_input(
                    sess,
                    *crate_type,
                    crate_name,
                    outputs,
                );
                out_filenames.push(p);
            },
            OutputType::DepInfo if sess.opts.debugging_opts.dep_info_omit_d_target => {
                // Don't add the dep-info output when omitting it from dep-info targets
            }
            _ => {
                out_filenames.push(file);
            }
        }
    }
    out_filenames
}

// Runs `f` on every output file path and returns the first non-None result, or None if `f`
// returns None for every file path.
fn check_output<F, T>(output_paths: &[PathBuf], f: F) -> Option<T>
where
    F: Fn(&PathBuf) -> Option<T>,
{
    for output_path in output_paths {
        if let Some(result) = f(output_path) {
            return Some(result);
        }
    }
    None
}

fn output_contains_path(output_paths: &[PathBuf], input_path: &PathBuf) -> bool {
    let input_path = input_path.canonicalize().ok();
    if input_path.is_none() {
        return false;
    }
    let check = |output_path: &PathBuf| {
        if output_path.canonicalize().ok() == input_path {
            Some(())
        } else {
            None
        }
    };
    check_output(output_paths, check).is_some()
}

fn output_conflicts_with_dir(output_paths: &[PathBuf]) -> Option<PathBuf> {
    let check = |output_path: &PathBuf| {
        if output_path.is_dir() {
            Some(output_path.clone())
        } else {
            None
        }
    };
    check_output(output_paths, check)
}

fn escape_dep_filename(filename: &FileName) -> String {
    // Apparently clang and gcc *only* escape spaces:
    // http://llvm.org/klaus/clang/commit/9d50634cfc268ecc9a7250226dd5ca0e945240d4
    filename.to_string().replace(" ", "\\ ")
}

fn write_out_deps(
    sess: &Session,
    boxed_resolver: &Steal<Rc<RefCell<BoxedResolver>>>,
    outputs: &OutputFilenames,
    out_filenames: &[PathBuf],
) {
    // Write out dependency rules to the dep-info file if requested
    if !sess.opts.output_types.contains_key(&OutputType::DepInfo) {
        return;
    }
    let deps_filename = outputs.path(OutputType::DepInfo);

    let result = (|| -> io::Result<()> {
        // Build a list of files used to compile the output and
        // write Makefile-compatible dependency rules
        let mut files: Vec<String> = sess.source_map()
            .files()
            .iter()
            .filter(|fmap| fmap.is_real_file())
            .filter(|fmap| !fmap.is_imported())
            .map(|fmap| escape_dep_filename(&fmap.unmapped_path.as_ref().unwrap_or(&fmap.name)))
            .collect();

        if sess.binary_dep_depinfo() {
            boxed_resolver.borrow().borrow_mut().access(|resolver| {
                for cnum in resolver.cstore().crates_untracked() {
                    let source = resolver.cstore().crate_source_untracked(cnum);
                    if let Some((path, _)) = source.dylib {
                        files.push(escape_dep_filename(&FileName::Real(path)));
                    }
                    if let Some((path, _)) = source.rlib {
                        files.push(escape_dep_filename(&FileName::Real(path)));
                    }
                    if let Some((path, _)) = source.rmeta {
                        files.push(escape_dep_filename(&FileName::Real(path)));
                    }
                }
            });
        }

        let mut file = fs::File::create(&deps_filename)?;
        for path in out_filenames {
            writeln!(file, "{}: {}\n", path.display(), files.join(" "))?;
        }

        // Emit a fake target for each input file to the compilation. This
        // prevents `make` from spitting out an error if a file is later
        // deleted. For more info see #28735
        for path in files {
            writeln!(file, "{}:", path)?;
        }
        Ok(())
    })();

    match result {
        Ok(_) => {
            if sess.opts.json_artifact_notifications {
                 sess.parse_sess.span_diagnostic
                    .emit_artifact_notification(&deps_filename, "dep-info");
            }
        },
        Err(e) => {
            sess.fatal(&format!(
                "error writing dependencies to `{}`: {}",
                deps_filename.display(),
                e
            ))
        }
    }
}

pub fn prepare_outputs(
    sess: &Session,
    compiler: &Compiler,
    krate: &ast::Crate,
    boxed_resolver: &Steal<Rc<RefCell<BoxedResolver>>>,
    crate_name: &str
) -> Result<OutputFilenames> {
    // FIXME: rustdoc passes &[] instead of &krate.attrs here
    let outputs = util::build_output_filenames(
        &compiler.input,
        &compiler.output_dir,
        &compiler.output_file,
        &krate.attrs,
        sess
    );

    let output_paths = generated_output_paths(
        sess,
        &outputs,
        compiler.output_file.is_some(),
        &crate_name,
    );

    // Ensure the source file isn't accidentally overwritten during compilation.
    if let Some(ref input_path) = compiler.input_path {
        if sess.opts.will_create_output_file() {
            if output_contains_path(&output_paths, input_path) {
                sess.err(&format!(
                    "the input file \"{}\" would be overwritten by the generated \
                        executable",
                    input_path.display()
                ));
                return Err(ErrorReported);
            }
            if let Some(dir_path) = output_conflicts_with_dir(&output_paths) {
                sess.err(&format!(
                    "the generated executable for the input file \"{}\" conflicts with the \
                        existing directory \"{}\"",
                    input_path.display(),
                    dir_path.display()
                ));
                return Err(ErrorReported);
            }
        }
    }

    write_out_deps(sess, boxed_resolver, &outputs, &output_paths);

    let only_dep_info = sess.opts.output_types.contains_key(&OutputType::DepInfo)
        && sess.opts.output_types.len() == 1;

    if !only_dep_info {
        if let Some(ref dir) = compiler.output_dir {
            if fs::create_dir_all(dir).is_err() {
                sess.err("failed to find or create the directory specified by `--out-dir`");
                return Err(ErrorReported);
            }
        }
    }

    Ok(outputs)
}

pub fn default_provide(providers: &mut ty::query::Providers<'_>) {
    providers.analysis = analysis;
    proc_macro_decls::provide(providers);
    plugin::build::provide(providers);
    hir::provide(providers);
    mir::provide(providers);
    reachable::provide(providers);
    resolve_lifetime::provide(providers);
    rustc_privacy::provide(providers);
    typeck::provide(providers);
    ty::provide(providers);
    traits::provide(providers);
    stability::provide(providers);
    reachable::provide(providers);
    rustc_passes::provide(providers);
    rustc_traits::provide(providers);
    middle::region::provide(providers);
    cstore::provide(providers);
    lint::provide(providers);
    rustc_lint::provide(providers);
    rustc_codegen_utils::provide(providers);
    rustc_codegen_ssa::provide(providers);
}

pub fn default_provide_extern(providers: &mut ty::query::Providers<'_>) {
    cstore::provide_extern(providers);
    rustc_codegen_ssa::provide_extern(providers);
}

declare_box_region_type!(
    pub BoxedGlobalCtxt,
    for('tcx),
    (&'tcx GlobalCtxt<'tcx>) -> ((), ())
);

impl BoxedGlobalCtxt {
    pub fn enter<F, R>(&mut self, f: F) -> R
    where
        F: for<'tcx> FnOnce(TyCtxt<'tcx>) -> R,
    {
        self.access(|gcx| ty::tls::enter_global(gcx, |tcx| f(tcx)))
    }
}

pub fn create_global_ctxt(
    compiler: &Compiler,
    lint_store: Lrc<lint::LintStore>,
    mut hir_forest: hir::map::Forest,
    mut resolver_outputs: ResolverOutputs,
    outputs: OutputFilenames,
    crate_name: &str,
) -> BoxedGlobalCtxt {
    let sess = compiler.session().clone();
    let codegen_backend = compiler.codegen_backend().clone();
    let crate_name = crate_name.to_string();
    let defs = mem::take(&mut resolver_outputs.definitions);
    let override_queries = compiler.override_queries;

    let ((), result) = BoxedGlobalCtxt::new(static move || {
        let sess = &*sess;

        let global_ctxt: Option<GlobalCtxt<'_>>;
        let arenas = AllArenas::new();

        // Construct the HIR map.
        let hir_map = time(sess, "indexing HIR", || {
            hir::map::map_crate(sess, &*resolver_outputs.cstore, &mut hir_forest, &defs)
        });

        let query_result_on_disk_cache = time(sess, "load query result cache", || {
            rustc_incremental::load_query_result_cache(sess)
        });

        let mut local_providers = ty::query::Providers::default();
        default_provide(&mut local_providers);
        codegen_backend.provide(&mut local_providers);

        let mut extern_providers = local_providers;
        default_provide_extern(&mut extern_providers);
        codegen_backend.provide_extern(&mut extern_providers);

        if let Some(callback) = override_queries {
            callback(sess, &mut local_providers, &mut extern_providers);
        }

        let gcx = TyCtxt::create_global_ctxt(
            sess,
            lint_store,
            local_providers,
            extern_providers,
            &arenas,
            resolver_outputs,
            hir_map,
            query_result_on_disk_cache,
            &crate_name,
            &outputs
        );

        global_ctxt = Some(gcx);
        let gcx = global_ctxt.as_ref().unwrap();

        ty::tls::enter_global(gcx, |tcx| {
            // Do some initialization of the DepGraph that can only be done with the
            // tcx available.
            time(tcx.sess, "dep graph tcx init", || rustc_incremental::dep_graph_tcx_init(tcx));
        });

        yield BoxedGlobalCtxt::initial_yield(());
        box_region_allow_access!(for('tcx), (&'tcx GlobalCtxt<'tcx>), (gcx));

        if sess.opts.debugging_opts.query_stats {
            gcx.queries.print_stats();
        }
    });

    result
}

/// Runs the resolution, type-checking, region checking and other
/// miscellaneous analysis passes on the crate.
fn analysis(tcx: TyCtxt<'_>, cnum: CrateNum) -> Result<()> {
    assert_eq!(cnum, LOCAL_CRATE);

    let sess = tcx.sess;
    let mut entry_point = None;

    time(sess, "misc checking 1", || {
        parallel!({
            entry_point = time(sess, "looking for entry point", || {
                rustc_passes::entry::find_entry_point(tcx)
            });

            time(sess, "looking for plugin registrar", || {
                plugin::build::find_plugin_registrar(tcx)
            });

            time(sess, "looking for derive registrar", || {
                proc_macro_decls::find(tcx)
            });
        }, {
            par_iter(&tcx.hir().krate().modules).for_each(|(&module, _)| {
                let local_def_id = tcx.hir().local_def_id(module);
                tcx.ensure().check_mod_loops(local_def_id);
                tcx.ensure().check_mod_attrs(local_def_id);
                tcx.ensure().check_mod_unstable_api_usage(local_def_id);
                tcx.ensure().check_mod_const_bodies(local_def_id);
            });
        });
    });

    // passes are timed inside typeck
    typeck::check_crate(tcx)?;

    time(sess, "misc checking 2", || {
        parallel!({
            time(sess, "match checking", || {
                tcx.par_body_owners(|def_id| {
                    tcx.ensure().check_match(def_id);
                });
            });
        }, {
            time(sess, "liveness checking + intrinsic checking", || {
                par_iter(&tcx.hir().krate().modules).for_each(|(&module, _)| {
                    // this must run before MIR dump, because
                    // "not all control paths return a value" is reported here.
                    //
                    // maybe move the check to a MIR pass?
                    let local_def_id = tcx.hir().local_def_id(module);

                    tcx.ensure().check_mod_liveness(local_def_id);
                    tcx.ensure().check_mod_intrinsics(local_def_id);
                });
            });
        });
    });

    time(sess, "MIR borrow checking", || {
        tcx.par_body_owners(|def_id| tcx.ensure().mir_borrowck(def_id));
    });

    time(sess, "dumping Chalk-like clauses", || {
        rustc_traits::lowering::dump_program_clauses(tcx);
    });

    time(sess, "MIR effect checking", || {
        for def_id in tcx.body_owners() {
            mir::transform::check_unsafety::check_unsafety(tcx, def_id)
        }
    });

    time(sess, "layout testing", || layout_test::test_layout(tcx));

    // Avoid overwhelming user with errors if borrow checking failed.
    // I'm not sure how helpful this is, to be honest, but it avoids a
    // lot of annoying errors in the compile-fail tests (basically,
    // lint warnings and so on -- kindck used to do this abort, but
    // kindck is gone now). -nmatsakis
    if sess.has_errors() {
        return Err(ErrorReported);
    }

    time(sess, "misc checking 3", || {
        parallel!({
            time(sess, "privacy access levels", || {
                tcx.ensure().privacy_access_levels(LOCAL_CRATE);
            });
            parallel!({
                time(sess, "private in public", || {
                    tcx.ensure().check_private_in_public(LOCAL_CRATE);
                });
            }, {
                time(sess, "death checking", || rustc_passes::dead::check_crate(tcx));
            },  {
                time(sess, "unused lib feature checking", || {
                    stability::check_unused_or_stable_features(tcx)
                });
            }, {
                time(sess, "lint checking", || {
                    lint::check_crate(tcx, || rustc_lint::BuiltinCombinedLateLintPass::new());
                });
            });
        }, {
            time(sess, "privacy checking modules", || {
                par_iter(&tcx.hir().krate().modules).for_each(|(&module, _)| {
                    tcx.ensure().check_mod_privacy(tcx.hir().local_def_id(module));
                });
            });
        });
    });

    Ok(())
}

fn encode_and_write_metadata(
    tcx: TyCtxt<'_>,
    outputs: &OutputFilenames,
) -> (middle::cstore::EncodedMetadata, bool) {
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    enum MetadataKind {
        None,
        Uncompressed,
        Compressed
    }

    let metadata_kind = tcx.sess.crate_types.borrow().iter().map(|ty| {
        match *ty {
            CrateType::Executable |
            CrateType::Staticlib |
            CrateType::Cdylib => MetadataKind::None,

            CrateType::Rlib => MetadataKind::Uncompressed,

            CrateType::Dylib |
            CrateType::ProcMacro => MetadataKind::Compressed,
        }
    }).max().unwrap_or(MetadataKind::None);

    let metadata = match metadata_kind {
        MetadataKind::None => middle::cstore::EncodedMetadata::new(),
        MetadataKind::Uncompressed |
        MetadataKind::Compressed => tcx.encode_metadata(),
    };

    let need_metadata_file = tcx.sess.opts.output_types.contains_key(&OutputType::Metadata);
    if need_metadata_file {
        let crate_name = &tcx.crate_name(LOCAL_CRATE).as_str();
        let out_filename = filename_for_metadata(tcx.sess, crate_name, outputs);
        // To avoid races with another rustc process scanning the output directory,
        // we need to write the file somewhere else and atomically move it to its
        // final destination, with an `fs::rename` call. In order for the rename to
        // always succeed, the temporary file needs to be on the same filesystem,
        // which is why we create it inside the output directory specifically.
        let metadata_tmpdir = TempFileBuilder::new()
            .prefix("rmeta")
            .tempdir_in(out_filename.parent().unwrap())
            .unwrap_or_else(|err| {
                tcx.sess.fatal(&format!("couldn't create a temp dir: {}", err))
            });
        let metadata_filename = emit_metadata(tcx.sess, &metadata, &metadata_tmpdir);
        if let Err(e) = fs::rename(&metadata_filename, &out_filename) {
            tcx.sess.fatal(&format!("failed to write {}: {}", out_filename.display(), e));
        }
        if tcx.sess.opts.json_artifact_notifications {
            tcx.sess.parse_sess.span_diagnostic
                .emit_artifact_notification(&out_filename, "metadata");
        }
    }

    let need_metadata_module = metadata_kind == MetadataKind::Compressed;

    (metadata, need_metadata_module)
}

/// Runs the codegen backend, after which the AST and analysis can
/// be discarded.
pub fn start_codegen<'tcx>(
    codegen_backend: &dyn CodegenBackend,
    tcx: TyCtxt<'tcx>,
    outputs: &OutputFilenames,
) -> Box<dyn Any> {
    if log_enabled!(::log::Level::Info) {
        println!("Pre-codegen");
        tcx.print_debug_stats();
    }

    let (metadata, need_metadata_module) = time(tcx.sess, "metadata encoding and writing", || {
        encode_and_write_metadata(tcx, outputs)
    });

    let codegen = time(tcx.sess, "codegen", move || {
        let _prof_timer = tcx.prof.generic_activity("codegen_crate");
        codegen_backend.codegen_crate(tcx, metadata, need_metadata_module)
    });

    if log_enabled!(::log::Level::Info) {
        println!("Post-codegen");
        tcx.print_debug_stats();
    }

    if tcx.sess.opts.output_types.contains_key(&OutputType::Mir) {
        if let Err(e) = mir::transform::dump_mir::emit_mir(tcx, outputs) {
            tcx.sess.err(&format!("could not emit MIR: {}", e));
            tcx.sess.abort_if_errors();
        }
    }

    codegen
}
