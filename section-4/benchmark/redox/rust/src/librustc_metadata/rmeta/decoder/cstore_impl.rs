use crate::cstore::{self, LoadedMacro};
use crate::link_args;
use crate::native_libs;
use crate::foreign_modules;
use crate::rmeta::{self, encoder};

use rustc::ty::query::QueryConfig;
use rustc::middle::cstore::{CrateSource, CrateStore, DepKind, EncodedMetadata, NativeLibraryKind};
use rustc::middle::exported_symbols::ExportedSymbol;
use rustc::middle::stability::DeprecationEntry;
use rustc::hir::def;
use rustc::hir;
use rustc::session::{CrateDisambiguator, Session};
use rustc::ty::{self, TyCtxt};
use rustc::ty::query::Providers;
use rustc::hir::def_id::{CrateNum, DefId, LOCAL_CRATE, CRATE_DEF_INDEX};
use rustc::hir::map::{DefKey, DefPath, DefPathHash};
use rustc::hir::map::definitions::DefPathTable;
use rustc::util::nodemap::DefIdMap;
use rustc_data_structures::svh::Svh;
use rustc_parse::source_file_to_stream;
use rustc_parse::parser::emit_unclosed_delims;

use smallvec::SmallVec;
use std::any::Any;
use rustc_data_structures::sync::Lrc;
use std::sync::Arc;

use syntax::ast;
use syntax::attr;
use syntax::source_map;
use syntax::source_map::Spanned;
use syntax::symbol::Symbol;
use syntax::expand::allocator::AllocatorKind;
use syntax_pos::{Span, FileName};

macro_rules! provide {
    (<$lt:tt> $tcx:ident, $def_id:ident, $other:ident, $cdata:ident,
      $($name:ident => $compute:block)*) => {
        pub fn provide_extern<$lt>(providers: &mut Providers<$lt>) {
            // HACK(eddyb) `$lt: $lt` forces `$lt` to be early-bound, which
            // allows the associated type in the return type to be normalized.
            $(fn $name<$lt: $lt, T: IntoArgs>(
                $tcx: TyCtxt<$lt>,
                def_id_arg: T,
            ) -> <ty::queries::$name<$lt> as QueryConfig<$lt>>::Value {
                let _prof_timer =
                    $tcx.prof.generic_activity("metadata_decode_entry");

                #[allow(unused_variables)]
                let ($def_id, $other) = def_id_arg.into_args();
                assert!(!$def_id.is_local());

                let $cdata = $tcx.crate_data_as_any($def_id.krate);
                let $cdata = $cdata.downcast_ref::<rmeta::CrateMetadata>()
                    .expect("CrateStore created data is not a CrateMetadata");

                if $tcx.dep_graph.is_fully_enabled() {
                    let crate_dep_node_index = $cdata.get_crate_dep_node_index($tcx);
                    $tcx.dep_graph.read_index(crate_dep_node_index);
                }

                $compute
            })*

            *providers = Providers {
                $($name,)*
                ..*providers
            };
        }
    }
}

// small trait to work around different signature queries all being defined via
// the macro above.
trait IntoArgs {
    fn into_args(self) -> (DefId, DefId);
}

impl IntoArgs for DefId {
    fn into_args(self) -> (DefId, DefId) { (self, self) }
}

impl IntoArgs for CrateNum {
    fn into_args(self) -> (DefId, DefId) { (self.as_def_id(), self.as_def_id()) }
}

impl IntoArgs for (CrateNum, DefId) {
    fn into_args(self) -> (DefId, DefId) { (self.0.as_def_id(), self.1) }
}

provide! { <'tcx> tcx, def_id, other, cdata,
    type_of => { cdata.get_type(def_id.index, tcx) }
    generics_of => {
        tcx.arena.alloc(cdata.get_generics(def_id.index, tcx.sess))
    }
    explicit_predicates_of => { cdata.get_explicit_predicates(def_id.index, tcx) }
    inferred_outlives_of => { cdata.get_inferred_outlives(def_id.index, tcx) }
    super_predicates_of => { cdata.get_super_predicates(def_id.index, tcx) }
    trait_def => {
        tcx.arena.alloc(cdata.get_trait_def(def_id.index, tcx.sess))
    }
    adt_def => { cdata.get_adt_def(def_id.index, tcx) }
    adt_destructor => {
        let _ = cdata;
        tcx.calculate_dtor(def_id, &mut |_,_| Ok(()))
    }
    variances_of => { tcx.arena.alloc_from_iter(cdata.get_item_variances(def_id.index)) }
    associated_item_def_ids => {
        let mut result = SmallVec::<[_; 8]>::new();
        cdata.each_child_of_item(def_id.index,
          |child| result.push(child.res.def_id()), tcx.sess);
        tcx.arena.alloc_slice(&result)
    }
    associated_item => { cdata.get_associated_item(def_id.index) }
    impl_trait_ref => { cdata.get_impl_trait(def_id.index, tcx) }
    impl_polarity => { cdata.get_impl_polarity(def_id.index) }
    coerce_unsized_info => {
        cdata.get_coerce_unsized_info(def_id.index).unwrap_or_else(|| {
            bug!("coerce_unsized_info: `{:?}` is missing its info", def_id);
        })
    }
    optimized_mir => { tcx.arena.alloc(cdata.get_optimized_mir(tcx, def_id.index)) }
    promoted_mir => { tcx.arena.alloc(cdata.get_promoted_mir(tcx, def_id.index)) }
    mir_const_qualif => { cdata.mir_const_qualif(def_id.index) }
    fn_sig => { cdata.fn_sig(def_id.index, tcx) }
    inherent_impls => { cdata.get_inherent_implementations_for_type(tcx, def_id.index) }
    is_const_fn_raw => { cdata.is_const_fn_raw(def_id.index) }
    asyncness => { cdata.asyncness(def_id.index) }
    is_foreign_item => { cdata.is_foreign_item(def_id.index) }
    static_mutability => { cdata.static_mutability(def_id.index) }
    def_kind => { cdata.def_kind(def_id.index) }
    def_span => { cdata.get_span(def_id.index, &tcx.sess) }
    lookup_stability => {
        cdata.get_stability(def_id.index).map(|s| tcx.intern_stability(s))
    }
    lookup_deprecation_entry => {
        cdata.get_deprecation(def_id.index).map(DeprecationEntry::external)
    }
    item_attrs => { cdata.get_item_attrs(def_id.index, tcx.sess) }
    // FIXME(#38501) We've skipped a `read` on the `HirBody` of
    // a `fn` when encoding, so the dep-tracking wouldn't work.
    // This is only used by rustdoc anyway, which shouldn't have
    // incremental recompilation ever enabled.
    fn_arg_names => { cdata.get_fn_param_names(def_id.index) }
    rendered_const => { cdata.get_rendered_const(def_id.index) }
    impl_parent => { cdata.get_parent_impl(def_id.index) }
    trait_of_item => { cdata.get_trait_of_item(def_id.index) }
    is_mir_available => { cdata.is_item_mir_available(def_id.index) }

    dylib_dependency_formats => { cdata.get_dylib_dependency_formats(tcx) }
    is_panic_runtime => { cdata.root.panic_runtime }
    is_compiler_builtins => { cdata.root.compiler_builtins }
    has_global_allocator => { cdata.root.has_global_allocator }
    has_panic_handler => { cdata.root.has_panic_handler }
    is_sanitizer_runtime => { cdata.root.sanitizer_runtime }
    is_profiler_runtime => { cdata.root.profiler_runtime }
    panic_strategy => { cdata.root.panic_strategy }
    extern_crate => {
        let r = *cdata.extern_crate.lock();
        r.map(|c| &*tcx.arena.alloc(c))
    }
    is_no_builtins => { cdata.root.no_builtins }
    symbol_mangling_version => { cdata.root.symbol_mangling_version }
    impl_defaultness => { cdata.get_impl_defaultness(def_id.index) }
    reachable_non_generics => {
        let reachable_non_generics = tcx
            .exported_symbols(cdata.cnum)
            .iter()
            .filter_map(|&(exported_symbol, export_level)| {
                if let ExportedSymbol::NonGeneric(def_id) = exported_symbol {
                    return Some((def_id, export_level))
                } else {
                    None
                }
            })
            .collect();

        tcx.arena.alloc(reachable_non_generics)
    }
    native_libraries => { Lrc::new(cdata.get_native_libraries(tcx.sess)) }
    foreign_modules => { cdata.get_foreign_modules(tcx) }
    plugin_registrar_fn => {
        cdata.root.plugin_registrar_fn.map(|index| {
            DefId { krate: def_id.krate, index }
        })
    }
    proc_macro_decls_static => {
        cdata.root.proc_macro_decls_static.map(|index| {
            DefId { krate: def_id.krate, index }
        })
    }
    crate_disambiguator => { cdata.root.disambiguator }
    crate_hash => { cdata.root.hash }
    original_crate_name => { cdata.root.name }

    extra_filename => { cdata.root.extra_filename.clone() }

    implementations_of_trait => {
        cdata.get_implementations_for_trait(tcx, Some(other))
    }

    all_trait_implementations => {
        cdata.get_implementations_for_trait(tcx, None)
    }

    visibility => { cdata.get_visibility(def_id.index) }
    dep_kind => {
        let r = *cdata.dep_kind.lock();
        r
    }
    crate_name => { cdata.root.name }
    item_children => {
        let mut result = SmallVec::<[_; 8]>::new();
        cdata.each_child_of_item(def_id.index, |child| result.push(child), tcx.sess);
        tcx.arena.alloc_slice(&result)
    }
    defined_lib_features => { cdata.get_lib_features(tcx) }
    defined_lang_items => { cdata.get_lang_items(tcx) }
    diagnostic_items => { cdata.get_diagnostic_items(tcx) }
    missing_lang_items => { cdata.get_missing_lang_items(tcx) }

    missing_extern_crate_item => {
        let r = match *cdata.extern_crate.borrow() {
            Some(extern_crate) if !extern_crate.is_direct() => true,
            _ => false,
        };
        r
    }

    used_crate_source => { Lrc::new(cdata.source.clone()) }

    exported_symbols => {
        let syms = cdata.exported_symbols(tcx);

        // FIXME rust-lang/rust#64319, rust-lang/rust#64872: We want
        // to block export of generics from dylibs, but we must fix
        // rust-lang/rust#65890 before we can do that robustly.

        Arc::new(syms)
    }
}

pub fn provide(providers: &mut Providers<'_>) {
    // FIXME(#44234) - almost all of these queries have no sub-queries and
    // therefore no actual inputs, they're just reading tables calculated in
    // resolve! Does this work? Unsure! That's what the issue is about
    *providers = Providers {
        is_dllimport_foreign_item: |tcx, id| {
            match tcx.native_library_kind(id) {
                Some(NativeLibraryKind::NativeUnknown) |
                Some(NativeLibraryKind::NativeRawDylib) => true,
                _ => false,
            }
        },
        is_statically_included_foreign_item: |tcx, id| {
            match tcx.native_library_kind(id) {
                Some(NativeLibraryKind::NativeStatic) |
                Some(NativeLibraryKind::NativeStaticNobundle) => true,
                _ => false,
            }
        },
        native_library_kind: |tcx, id| {
            tcx.native_libraries(id.krate)
                .iter()
                .filter(|lib| native_libs::relevant_lib(&tcx.sess, lib))
                .find(|lib| {
                    let fm_id = match lib.foreign_module {
                        Some(id) => id,
                        None => return false,
                    };
                    tcx.foreign_modules(id.krate)
                        .iter()
                        .find(|m| m.def_id == fm_id)
                        .expect("failed to find foreign module")
                        .foreign_items
                        .contains(&id)
                })
                .map(|l| l.kind)
        },
        native_libraries: |tcx, cnum| {
            assert_eq!(cnum, LOCAL_CRATE);
            Lrc::new(native_libs::collect(tcx))
        },
        foreign_modules: |tcx, cnum| {
            assert_eq!(cnum, LOCAL_CRATE);
            &tcx.arena.alloc(foreign_modules::collect(tcx))[..]
        },
        link_args: |tcx, cnum| {
            assert_eq!(cnum, LOCAL_CRATE);
            Lrc::new(link_args::collect(tcx))
        },

        // Returns a map from a sufficiently visible external item (i.e., an
        // external item that is visible from at least one local module) to a
        // sufficiently visible parent (considering modules that re-export the
        // external item to be parents).
        visible_parent_map: |tcx, cnum| {
            use std::collections::vec_deque::VecDeque;
            use std::collections::hash_map::Entry;

            assert_eq!(cnum, LOCAL_CRATE);
            let mut visible_parent_map: DefIdMap<DefId> = Default::default();

            // Issue 46112: We want the map to prefer the shortest
            // paths when reporting the path to an item. Therefore we
            // build up the map via a breadth-first search (BFS),
            // which naturally yields minimal-length paths.
            //
            // Note that it needs to be a BFS over the whole forest of
            // crates, not just each individual crate; otherwise you
            // only get paths that are locally minimal with respect to
            // whatever crate we happened to encounter first in this
            // traversal, but not globally minimal across all crates.
            let bfs_queue = &mut VecDeque::new();

            // Preferring shortest paths alone does not guarantee a
            // deterministic result; so sort by crate num to avoid
            // hashtable iteration non-determinism. This only makes
            // things as deterministic as crate-nums assignment is,
            // which is to say, its not deterministic in general. But
            // we believe that libstd is consistently assigned crate
            // num 1, so it should be enough to resolve #46112.
            let mut crates: Vec<CrateNum> = (*tcx.crates()).to_owned();
            crates.sort();

            for &cnum in crates.iter() {
                // Ignore crates without a corresponding local `extern crate` item.
                if tcx.missing_extern_crate_item(cnum) {
                    continue
                }

                bfs_queue.push_back(DefId {
                    krate: cnum,
                    index: CRATE_DEF_INDEX
                });
            }

            // (restrict scope of mutable-borrow of `visible_parent_map`)
            {
                let visible_parent_map = &mut visible_parent_map;
                let mut add_child = |bfs_queue: &mut VecDeque<_>,
                                     child: &def::Export<hir::HirId>,
                                     parent: DefId| {
                    if child.vis != ty::Visibility::Public {
                        return;
                    }

                    if let Some(child) = child.res.opt_def_id() {
                        match visible_parent_map.entry(child) {
                            Entry::Occupied(mut entry) => {
                                // If `child` is defined in crate `cnum`, ensure
                                // that it is mapped to a parent in `cnum`.
                                if child.krate == cnum && entry.get().krate != cnum {
                                    entry.insert(parent);
                                }
                            }
                            Entry::Vacant(entry) => {
                                entry.insert(parent);
                                bfs_queue.push_back(child);
                            }
                        }
                    }
                };

                while let Some(def) = bfs_queue.pop_front() {
                    for child in tcx.item_children(def).iter() {
                        add_child(bfs_queue, child, def);
                    }
                }
            }

            tcx.arena.alloc(visible_parent_map)
        },

        dependency_formats: |tcx, cnum| {
            assert_eq!(cnum, LOCAL_CRATE);
            Lrc::new(crate::dependency_format::calculate(tcx))
        },

        ..*providers
    };
}

impl cstore::CStore {
    pub fn export_macros_untracked(&self, cnum: CrateNum) {
        let data = self.get_crate_data(cnum);
        let mut dep_kind = data.dep_kind.lock();
        if *dep_kind == DepKind::UnexportedMacrosOnly {
            *dep_kind = DepKind::MacrosOnly;
        }
    }

    pub fn struct_field_names_untracked(&self, def: DefId, sess: &Session) -> Vec<Spanned<Symbol>> {
        self.get_crate_data(def.krate).get_struct_field_names(def.index, sess)
    }

    pub fn item_children_untracked(
        &self,
        def_id: DefId,
        sess: &Session
    ) -> Vec<def::Export<hir::HirId>> {
        let mut result = vec![];
        self.get_crate_data(def_id.krate)
            .each_child_of_item(def_id.index, |child| result.push(child), sess);
        result
    }

    pub fn load_macro_untracked(&self, id: DefId, sess: &Session) -> LoadedMacro {
        let _prof_timer = sess.prof.generic_activity("metadata_load_macro");

        let data = self.get_crate_data(id.krate);
        if data.root.is_proc_macro_crate() {
            return LoadedMacro::ProcMacro(data.load_proc_macro(id.index, sess));
        }

        let def = data.get_macro(id.index);
        let macro_full_name = data.def_path(id.index).to_string_friendly(|_| data.root.name);
        let source_name = FileName::Macros(macro_full_name);

        let source_file = sess.parse_sess.source_map().new_source_file(source_name, def.body);
        let local_span = Span::with_root_ctxt(source_file.start_pos, source_file.end_pos);
        let (body, mut errors) = source_file_to_stream(&sess.parse_sess, source_file, None);
        emit_unclosed_delims(&mut errors, &sess.parse_sess);

        // Mark the attrs as used
        let attrs = data.get_item_attrs(id.index, sess);
        for attr in attrs.iter() {
            attr::mark_used(attr);
        }

        let name = data.def_key(id.index).disambiguated_data.data
            .get_opt_name().expect("no name in load_macro");
        sess.imported_macro_spans.borrow_mut()
            .insert(local_span, (name.to_string(), data.get_span(id.index, sess)));

        LoadedMacro::MacroDef(ast::Item {
            // FIXME: cross-crate hygiene
            ident: ast::Ident::with_dummy_span(name),
            id: ast::DUMMY_NODE_ID,
            span: local_span,
            attrs: attrs.iter().cloned().collect(),
            kind: ast::ItemKind::MacroDef(ast::MacroDef {
                tokens: body.into(),
                legacy: def.legacy,
            }),
            vis: source_map::respan(local_span.shrink_to_lo(), ast::VisibilityKind::Inherited),
            tokens: None,
        }, data.root.edition)
    }

    pub fn associated_item_cloned_untracked(&self, def: DefId) -> ty::AssocItem {
        self.get_crate_data(def.krate).get_associated_item(def.index)
    }

    pub fn crate_source_untracked(&self, cnum: CrateNum) -> CrateSource {
        self.get_crate_data(cnum).source.clone()
    }
}

impl CrateStore for cstore::CStore {
    fn crate_data_as_any(&self, cnum: CrateNum) -> &dyn Any {
        self.get_crate_data(cnum)
    }

    fn item_generics_cloned_untracked(&self, def: DefId, sess: &Session) -> ty::Generics {
        self.get_crate_data(def.krate).get_generics(def.index, sess)
    }

    fn crate_name_untracked(&self, cnum: CrateNum) -> Symbol
    {
        self.get_crate_data(cnum).root.name
    }

    fn crate_is_private_dep_untracked(&self, cnum: CrateNum) -> bool {
        self.get_crate_data(cnum).private_dep
    }

    fn crate_disambiguator_untracked(&self, cnum: CrateNum) -> CrateDisambiguator
    {
        self.get_crate_data(cnum).root.disambiguator
    }

    fn crate_hash_untracked(&self, cnum: CrateNum) -> Svh
    {
        self.get_crate_data(cnum).root.hash
    }

    fn crate_host_hash_untracked(&self, cnum: CrateNum) -> Option<Svh> {
        self.get_crate_data(cnum).host_hash
    }

    /// Returns the `DefKey` for a given `DefId`. This indicates the
    /// parent `DefId` as well as some idea of what kind of data the
    /// `DefId` refers to.
    fn def_key(&self, def: DefId) -> DefKey {
        self.get_crate_data(def.krate).def_key(def.index)
    }

    fn def_path(&self, def: DefId) -> DefPath {
        self.get_crate_data(def.krate).def_path(def.index)
    }

    fn def_path_hash(&self, def: DefId) -> DefPathHash {
        self.get_crate_data(def.krate).def_path_hash(def.index)
    }

    fn def_path_table(&self, cnum: CrateNum) -> &DefPathTable {
        &self.get_crate_data(cnum).def_path_table
    }

    fn crates_untracked(&self) -> Vec<CrateNum>
    {
        let mut result = vec![];
        self.iter_crate_data(|cnum, _| result.push(cnum));
        result
    }

    fn postorder_cnums_untracked(&self) -> Vec<CrateNum> {
        self.do_postorder_cnums_untracked()
    }

    fn encode_metadata(&self, tcx: TyCtxt<'_>) -> EncodedMetadata {
        encoder::encode_metadata(tcx)
    }

    fn metadata_encoding_version(&self) -> &[u8]
    {
        rmeta::METADATA_HEADER
    }

    fn injected_panic_runtime(&self) -> Option<CrateNum> {
        self.injected_panic_runtime
    }

    fn allocator_kind(&self) -> Option<AllocatorKind> {
        self.allocator_kind
    }
}
