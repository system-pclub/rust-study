use crate::utils::span_lint;
use rustc::hir::{Expr, ExprKind};
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::ty;
use rustc::{declare_lint_pass, declare_tool_lint};

declare_clippy_lint! {
    /// **What it does:** Checks for needlessly including a base struct on update
    /// when all fields are changed anyway.
    ///
    /// **Why is this bad?** This will cost resources (because the base has to be
    /// somewhere), and make the code less readable.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// # struct Point {
    /// #     x: i32,
    /// #     y: i32,
    /// #     z: i32,
    /// # }
    /// # let zero_point = Point { x: 0, y: 0, z: 0 };
    /// Point {
    ///     x: 1,
    ///     y: 1,
    ///     ..zero_point
    /// };
    /// ```
    pub NEEDLESS_UPDATE,
    complexity,
    "using `Foo { ..base }` when there are no missing fields"
}

declare_lint_pass!(NeedlessUpdate => [NEEDLESS_UPDATE]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for NeedlessUpdate {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        if let ExprKind::Struct(_, ref fields, Some(ref base)) = expr.kind {
            let ty = cx.tables.expr_ty(expr);
            if let ty::Adt(def, _) = ty.kind {
                if fields.len() == def.non_enum_variant().fields.len() {
                    span_lint(
                        cx,
                        NEEDLESS_UPDATE,
                        base.span,
                        "struct update has no effect, all the fields in the struct have already been specified",
                    );
                }
            }
        }
    }
}
