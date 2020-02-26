use rustc::hir::map as hir_map;
use rustc::hir::def_id::{CrateNum, CRATE_DEF_INDEX, DefId, LOCAL_CRATE};
use rustc::session::{config, Session};
use rustc::session::config::EntryFnType;
use syntax::attr;
use syntax::entry::EntryPointType;
use syntax::symbol::sym;
use syntax_pos::Span;
use rustc::hir::{HirId, Item, ItemKind, ImplItem, TraitItem};
use rustc::hir::itemlikevisit::ItemLikeVisitor;
use rustc::ty::TyCtxt;
use rustc::ty::query::Providers;

use rustc_error_codes::*;

struct EntryContext<'a, 'tcx> {
    session: &'a Session,

    map: &'a hir_map::Map<'tcx>,

    /// The top-level function called `main`.
    main_fn: Option<(HirId, Span)>,

    /// The function that has attribute named `main`.
    attr_main_fn: Option<(HirId, Span)>,

    /// The function that has the attribute 'start' on it.
    start_fn: Option<(HirId, Span)>,

    /// The functions that one might think are `main` but aren't, e.g.
    /// main functions not defined at the top level. For diagnostics.
    non_main_fns: Vec<(HirId, Span)> ,
}

impl<'a, 'tcx> ItemLikeVisitor<'tcx> for EntryContext<'a, 'tcx> {
    fn visit_item(&mut self, item: &'tcx Item) {
        let def_id = self.map.local_def_id(item.hir_id);
        let def_key = self.map.def_key(def_id);
        let at_root = def_key.parent == Some(CRATE_DEF_INDEX);
        find_item(item, self, at_root);
    }

    fn visit_trait_item(&mut self, _trait_item: &'tcx TraitItem) {
        // Entry fn is never a trait item.
    }

    fn visit_impl_item(&mut self, _impl_item: &'tcx ImplItem) {
        // Entry fn is never a trait item.
    }
}

fn entry_fn(tcx: TyCtxt<'_>, cnum: CrateNum) -> Option<(DefId, EntryFnType)> {
    assert_eq!(cnum, LOCAL_CRATE);

    let any_exe = tcx.sess.crate_types.borrow().iter().any(|ty| {
        *ty == config::CrateType::Executable
    });
    if !any_exe {
        // No need to find a main function.
        return None;
    }

    // If the user wants no main function at all, then stop here.
    if attr::contains_name(&tcx.hir().krate().attrs, sym::no_main) {
        return None;
    }

    let mut ctxt = EntryContext {
        session: tcx.sess,
        map: tcx.hir(),
        main_fn: None,
        attr_main_fn: None,
        start_fn: None,
        non_main_fns: Vec::new(),
    };

    tcx.hir().krate().visit_all_item_likes(&mut ctxt);

    configure_main(tcx, &ctxt)
}

// Beware, this is duplicated in `libsyntax/entry.rs`, so make sure to keep
// them in sync.
fn entry_point_type(item: &Item, at_root: bool) -> EntryPointType {
    match item.kind {
        ItemKind::Fn(..) => {
            if attr::contains_name(&item.attrs, sym::start) {
                EntryPointType::Start
            } else if attr::contains_name(&item.attrs, sym::main) {
                EntryPointType::MainAttr
            } else if item.ident.name == sym::main {
                if at_root {
                    // This is a top-level function so can be `main`.
                    EntryPointType::MainNamed
                } else {
                    EntryPointType::OtherMain
                }
            } else {
                EntryPointType::None
            }
        }
        _ => EntryPointType::None,
    }
}


fn find_item(item: &Item, ctxt: &mut EntryContext<'_, '_>, at_root: bool) {
    match entry_point_type(item, at_root) {
        EntryPointType::MainNamed => {
            if ctxt.main_fn.is_none() {
                ctxt.main_fn = Some((item.hir_id, item.span));
            } else {
                span_err!(ctxt.session, item.span, E0136,
                          "multiple `main` functions");
            }
        },
        EntryPointType::OtherMain => {
            ctxt.non_main_fns.push((item.hir_id, item.span));
        },
        EntryPointType::MainAttr => {
            if ctxt.attr_main_fn.is_none() {
                ctxt.attr_main_fn = Some((item.hir_id, item.span));
            } else {
                struct_span_err!(ctxt.session, item.span, E0137,
                                 "multiple functions with a `#[main]` attribute")
                .span_label(item.span, "additional `#[main]` function")
                .span_label(ctxt.attr_main_fn.unwrap().1, "first `#[main]` function")
                .emit();
            }
        },
        EntryPointType::Start => {
            if ctxt.start_fn.is_none() {
                ctxt.start_fn = Some((item.hir_id, item.span));
            } else {
                struct_span_err!(ctxt.session, item.span, E0138, "multiple `start` functions")
                    .span_label(ctxt.start_fn.unwrap().1, "previous `start` function here")
                    .span_label(item.span, "multiple `start` functions")
                    .emit();
            }
        }
        EntryPointType::None => (),
    }
}

fn configure_main(tcx: TyCtxt<'_>, visitor: &EntryContext<'_, '_>) -> Option<(DefId, EntryFnType)> {
    if let Some((hir_id, _)) = visitor.start_fn {
        Some((tcx.hir().local_def_id(hir_id), EntryFnType::Start))
    } else if let Some((hir_id, _)) = visitor.attr_main_fn {
        Some((tcx.hir().local_def_id(hir_id), EntryFnType::Main))
    } else if let Some((hir_id, _)) = visitor.main_fn {
        Some((tcx.hir().local_def_id(hir_id), EntryFnType::Main))
    } else {
        no_main_err(tcx, visitor);
        None
    }
}

fn no_main_err(tcx: TyCtxt<'_>, visitor: &EntryContext<'_, '_>) {
    let sp = tcx.hir().krate().span;
    if *tcx.sess.parse_sess.reached_eof.borrow() {
        // There's an unclosed brace that made the parser reach `Eof`, we shouldn't complain about
        // the missing `fn main()` then as it might have been hidden inside an unclosed block.
        tcx.sess.delay_span_bug(sp, "`main` not found, but expected unclosed brace error");
        return;
    }

    // There is no main function.
    let mut err = struct_err!(tcx.sess, E0601,
        "`main` function not found in crate `{}`", tcx.crate_name(LOCAL_CRATE));
    let filename = &tcx.sess.local_crate_source_file;
    let note = if !visitor.non_main_fns.is_empty() {
        for &(_, span) in &visitor.non_main_fns {
            err.span_note(span, "here is a function named `main`");
        }
        err.note("you have one or more functions named `main` not defined at the crate level");
        err.help("either move the `main` function definitions or attach the `#[main]` attribute \
                  to one of them");
        // There were some functions named `main` though. Try to give the user a hint.
        format!("the main function must be defined at the crate level{}",
                 filename.as_ref().map(|f| format!(" (in `{}`)", f.display())).unwrap_or_default())
    } else if let Some(filename) = filename {
        format!("consider adding a `main` function to `{}`", filename.display())
    } else {
        String::from("consider adding a `main` function at the crate level")
    };
    // The file may be empty, which leads to the diagnostic machinery not emitting this
    // note. This is a relatively simple way to detect that case and emit a span-less
    // note instead.
    if let Ok(_) = tcx.sess.source_map().lookup_line(sp.lo()) {
        err.set_span(sp);
        err.span_label(sp, &note);
    } else {
        err.note(&note);
    }
    if tcx.sess.teach(&err.get_code().unwrap()) {
        err.note("If you don't know the basics of Rust, you can go look to the Rust Book \
                  to get started: https://doc.rust-lang.org/book/");
    }
    err.emit();
}

pub fn find_entry_point(tcx: TyCtxt<'_>) -> Option<(DefId, EntryFnType)> {
    tcx.entry_fn(LOCAL_CRATE)
}

pub fn provide(providers: &mut Providers<'_>) {
    *providers = Providers {
        entry_fn,
        ..*providers
    };
}
