use crate::consts::{constant_simple, Constant};
use crate::utils::span_help_and_lint;
use if_chain::if_chain;
use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};

declare_clippy_lint! {
    /// **What it does:** Checks for `0.0 / 0.0`.
    ///
    /// **Why is this bad?** It's less readable than `std::f32::NAN` or
    /// `std::f64::NAN`.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// 0.0f32 / 0.0;
    /// ```
    pub ZERO_DIVIDED_BY_ZERO,
    complexity,
    "usage of `0.0 / 0.0` to obtain NaN instead of std::f32::NaN or std::f64::NaN"
}

declare_lint_pass!(ZeroDiv => [ZERO_DIVIDED_BY_ZERO]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for ZeroDiv {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        // check for instances of 0.0/0.0
        if_chain! {
            if let ExprKind::Binary(ref op, ref left, ref right) = expr.kind;
            if let BinOpKind::Div = op.node;
            // TODO - constant_simple does not fold many operations involving floats.
            // That's probably fine for this lint - it's pretty unlikely that someone would
            // do something like 0.0/(2.0 - 2.0), but it would be nice to warn on that case too.
            if let Some(lhs_value) = constant_simple(cx, cx.tables, left);
            if let Some(rhs_value) = constant_simple(cx, cx.tables, right);
            if Constant::F32(0.0) == lhs_value || Constant::F64(0.0) == lhs_value;
            if Constant::F32(0.0) == rhs_value || Constant::F64(0.0) == rhs_value;
            then {
                // since we're about to suggest a use of std::f32::NaN or std::f64::NaN,
                // match the precision of the literals that are given.
                let float_type = match (lhs_value, rhs_value) {
                    (Constant::F64(_), _)
                    | (_, Constant::F64(_)) => "f64",
                    _ => "f32"
                };
                span_help_and_lint(
                    cx,
                    ZERO_DIVIDED_BY_ZERO,
                    expr.span,
                    "constant division of 0.0 with 0.0 will always result in NaN",
                    &format!(
                        "Consider using `std::{}::NAN` if you would like a constant representing NaN",
                        float_type,
                    ),
                );
            }
        }
    }
}
