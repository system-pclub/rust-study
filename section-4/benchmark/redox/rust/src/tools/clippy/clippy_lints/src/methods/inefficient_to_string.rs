use super::INEFFICIENT_TO_STRING;
use crate::utils::{match_def_path, paths, snippet_with_applicability, span_lint_and_then, walk_ptrs_ty_depth};
use if_chain::if_chain;
use rustc::hir;
use rustc::lint::LateContext;
use rustc::ty::{self, Ty};
use rustc_errors::Applicability;

/// Checks for the `INEFFICIENT_TO_STRING` lint
pub fn lint<'tcx>(cx: &LateContext<'_, 'tcx>, expr: &hir::Expr, arg: &hir::Expr, arg_ty: Ty<'tcx>) {
    if_chain! {
        if let Some(to_string_meth_did) = cx.tables.type_dependent_def_id(expr.hir_id);
        if match_def_path(cx, to_string_meth_did, &paths::TO_STRING_METHOD);
        if let Some(substs) = cx.tables.node_substs_opt(expr.hir_id);
        let self_ty = substs.type_at(0);
        let (deref_self_ty, deref_count) = walk_ptrs_ty_depth(self_ty);
        if deref_count >= 1;
        if specializes_tostring(cx, deref_self_ty);
        then {
            span_lint_and_then(
                cx,
                INEFFICIENT_TO_STRING,
                expr.span,
                &format!("calling `to_string` on `{}`", arg_ty),
                |db| {
                    db.help(&format!(
                        "`{}` implements `ToString` through a slower blanket impl, but `{}` has a fast specialization of `ToString`",
                        self_ty, deref_self_ty
                    ));
                    let mut applicability = Applicability::MachineApplicable;
                    let arg_snippet = snippet_with_applicability(cx, arg.span, "..", &mut applicability);
                    db.span_suggestion(
                        expr.span,
                        "try dereferencing the receiver",
                        format!("({}{}).to_string()", "*".repeat(deref_count), arg_snippet),
                        applicability,
                    );
                },
            );
        }
    }
}

/// Returns whether `ty` specializes `ToString`.
/// Currently, these are `str`, `String`, and `Cow<'_, str>`.
fn specializes_tostring(cx: &LateContext<'_, '_>, ty: Ty<'_>) -> bool {
    match ty.kind {
        ty::Str => true,
        ty::Adt(adt, substs) => {
            match_def_path(cx, adt.did, &paths::STRING)
                || (match_def_path(cx, adt.did, &paths::COW) && substs.type_at(1).is_str())
        },
        _ => false,
    }
}
