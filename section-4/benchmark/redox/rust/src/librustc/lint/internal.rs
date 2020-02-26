//! Some lints that are only useful in the compiler or crates that use compiler internals, such as
//! Clippy.

use crate::hir::{GenericArg, HirId, MutTy, Mutability, Path, PathSegment, QPath, Ty, TyKind};
use crate::lint::{
    EarlyContext, EarlyLintPass, LateContext, LateLintPass, LintArray, LintContext, LintPass,
};
use errors::Applicability;
use rustc_data_structures::fx::FxHashMap;
use syntax::ast::{Ident, Item, ItemKind};
use syntax::symbol::{sym, Symbol};

declare_tool_lint! {
    pub rustc::DEFAULT_HASH_TYPES,
    Allow,
    "forbid HashMap and HashSet and suggest the FxHash* variants"
}

pub struct DefaultHashTypes {
    map: FxHashMap<Symbol, Symbol>,
}

impl DefaultHashTypes {
    // we are allowed to use `HashMap` and `HashSet` as identifiers for implementing the lint itself
    #[allow(rustc::default_hash_types)]
    pub fn new() -> Self {
        let mut map = FxHashMap::default();
        map.insert(sym::HashMap, sym::FxHashMap);
        map.insert(sym::HashSet, sym::FxHashSet);
        Self { map }
    }
}

impl_lint_pass!(DefaultHashTypes => [DEFAULT_HASH_TYPES]);

impl EarlyLintPass for DefaultHashTypes {
    fn check_ident(&mut self, cx: &EarlyContext<'_>, ident: Ident) {
        if let Some(replace) = self.map.get(&ident.name) {
            let msg = format!("Prefer {} over {}, it has better performance", replace, ident);
            let mut db = cx.struct_span_lint(DEFAULT_HASH_TYPES, ident.span, &msg);
            db.span_suggestion(
                ident.span,
                "use",
                replace.to_string(),
                Applicability::MaybeIncorrect, // FxHashMap, ... needs another import
            );
            db.note(&format!("a `use rustc_data_structures::fx::{}` may be necessary", replace))
                .emit();
        }
    }
}

declare_tool_lint! {
    pub rustc::USAGE_OF_TY_TYKIND,
    Allow,
    "usage of `ty::TyKind` outside of the `ty::sty` module"
}

declare_tool_lint! {
    pub rustc::TY_PASS_BY_REFERENCE,
    Allow,
    "passing `Ty` or `TyCtxt` by reference"
}

declare_tool_lint! {
    pub rustc::USAGE_OF_QUALIFIED_TY,
    Allow,
    "using `ty::{Ty,TyCtxt}` instead of importing it"
}

declare_lint_pass!(TyTyKind => [
    USAGE_OF_TY_TYKIND,
    TY_PASS_BY_REFERENCE,
    USAGE_OF_QUALIFIED_TY,
]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for TyTyKind {
    fn check_path(&mut self, cx: &LateContext<'_, '_>, path: &'tcx Path, _: HirId) {
        let segments = path.segments.iter().rev().skip(1).rev();

        if let Some(last) = segments.last() {
            let span = path.span.with_hi(last.ident.span.hi());
            if lint_ty_kind_usage(cx, last) {
                cx.struct_span_lint(USAGE_OF_TY_TYKIND, span, "usage of `ty::TyKind::<kind>`")
                    .span_suggestion(
                        span,
                        "try using ty::<kind> directly",
                        "ty".to_string(),
                        Applicability::MaybeIncorrect, // ty maybe needs an import
                    )
                    .emit();
            }
        }
    }

    fn check_ty(&mut self, cx: &LateContext<'_, '_>, ty: &'tcx Ty) {
        match &ty.kind {
            TyKind::Path(qpath) => {
                if let QPath::Resolved(_, path) = qpath {
                    if let Some(last) = path.segments.iter().last() {
                        if lint_ty_kind_usage(cx, last) {
                            cx.struct_span_lint(
                                USAGE_OF_TY_TYKIND,
                                path.span,
                                "usage of `ty::TyKind`",
                            )
                            .help("try using `Ty` instead")
                            .emit();
                        } else {
                            if ty.span.from_expansion() {
                                return;
                            }
                            if let Some(t) = is_ty_or_ty_ctxt(cx, ty) {
                                if path.segments.len() > 1 {
                                    cx.struct_span_lint(
                                        USAGE_OF_QUALIFIED_TY,
                                        path.span,
                                        &format!("usage of qualified `ty::{}`", t),
                                    )
                                    .span_suggestion(
                                        path.span,
                                        "try using it unqualified",
                                        t,
                                        // The import probably needs to be changed
                                        Applicability::MaybeIncorrect,
                                    )
                                    .emit();
                                }
                            }
                        }
                    }
                }
            }
            TyKind::Rptr(_, MutTy { ty: inner_ty, mutbl: Mutability::Immutable }) => {
                if let Some(impl_did) = cx.tcx.impl_of_method(ty.hir_id.owner_def_id()) {
                    if cx.tcx.impl_trait_ref(impl_did).is_some() {
                        return;
                    }
                }
                if let Some(t) = is_ty_or_ty_ctxt(cx, &inner_ty) {
                    cx.struct_span_lint(
                        TY_PASS_BY_REFERENCE,
                        ty.span,
                        &format!("passing `{}` by reference", t),
                    )
                    .span_suggestion(
                        ty.span,
                        "try passing by value",
                        t,
                        // Changing type of function argument
                        Applicability::MaybeIncorrect,
                    )
                    .emit();
                }
            }
            _ => {}
        }
    }
}

fn lint_ty_kind_usage(cx: &LateContext<'_, '_>, segment: &PathSegment) -> bool {
    if let Some(res) = segment.res {
        if let Some(did) = res.opt_def_id() {
            return cx.tcx.is_diagnostic_item(sym::TyKind, did);
        }
    }

    false
}

fn is_ty_or_ty_ctxt(cx: &LateContext<'_, '_>, ty: &Ty) -> Option<String> {
    match &ty.kind {
        TyKind::Path(qpath) => {
            if let QPath::Resolved(_, path) = qpath {
                let did = path.res.opt_def_id()?;
                if cx.tcx.is_diagnostic_item(sym::Ty, did) {
                    return Some(format!("Ty{}", gen_args(path.segments.last().unwrap())));
                } else if cx.tcx.is_diagnostic_item(sym::TyCtxt, did) {
                    return Some(format!("TyCtxt{}", gen_args(path.segments.last().unwrap())));
                }
            }
        }
        _ => {}
    }

    None
}

fn gen_args(segment: &PathSegment) -> String {
    if let Some(args) = &segment.args {
        let lifetimes = args
            .args
            .iter()
            .filter_map(|arg| {
                if let GenericArg::Lifetime(lt) = arg {
                    Some(lt.name.ident().to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if !lifetimes.is_empty() {
            return format!("<{}>", lifetimes.join(", "));
        }
    }

    String::new()
}

declare_tool_lint! {
    pub rustc::LINT_PASS_IMPL_WITHOUT_MACRO,
    Allow,
    "`impl LintPass` without the `declare_lint_pass!` or `impl_lint_pass!` macros"
}

declare_lint_pass!(LintPassImpl => [LINT_PASS_IMPL_WITHOUT_MACRO]);

impl EarlyLintPass for LintPassImpl {
    fn check_item(&mut self, cx: &EarlyContext<'_>, item: &Item) {
        if let ItemKind::Impl(_, _, _, _, Some(lint_pass), _, _) = &item.kind {
            if let Some(last) = lint_pass.path.segments.last() {
                if last.ident.name == sym::LintPass {
                    let expn_data = lint_pass.path.span.ctxt().outer_expn_data();
                    let call_site = expn_data.call_site;
                    if expn_data.kind.descr() != sym::impl_lint_pass &&
                       call_site.ctxt().outer_expn_data().kind.descr() != sym::declare_lint_pass {
                        cx.struct_span_lint(
                            LINT_PASS_IMPL_WITHOUT_MACRO,
                            lint_pass.path.span,
                            "implementing `LintPass` by hand",
                        )
                        .help("try using `declare_lint_pass!` or `impl_lint_pass!` instead")
                        .emit();
                    }
                }
            }
        }
    }
}
