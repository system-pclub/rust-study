use crate::utils::is_adjusted;
use crate::utils::span_lint;
use rustc::hir::def::{DefKind, Res};
use rustc::hir::{Expr, ExprKind};
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};

declare_clippy_lint! {
    /// **What it does:** Checks for construction of a structure or tuple just to
    /// assign a value in it.
    ///
    /// **Why is this bad?** Readability. If the structure is only created to be
    /// updated, why not write the structure you want in the first place?
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// (0, 0).0 = 1
    /// ```
    pub TEMPORARY_ASSIGNMENT,
    complexity,
    "assignments to temporaries"
}

fn is_temporary(cx: &LateContext<'_, '_>, expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Struct(..) | ExprKind::Tup(..) => true,
        ExprKind::Path(qpath) => {
            if let Res::Def(DefKind::Const, ..) = cx.tables.qpath_res(qpath, expr.hir_id) {
                true
            } else {
                false
            }
        },
        _ => false,
    }
}

declare_lint_pass!(TemporaryAssignment => [TEMPORARY_ASSIGNMENT]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for TemporaryAssignment {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        if let ExprKind::Assign(target, _) = &expr.kind {
            let mut base = target;
            while let ExprKind::Field(f, _) | ExprKind::Index(f, _) = &base.kind {
                base = f;
            }
            if is_temporary(cx, base) && !is_adjusted(cx, base) {
                span_lint(cx, TEMPORARY_ASSIGNMENT, expr.span, "assignment to temporary");
            }
        }
    }
}
