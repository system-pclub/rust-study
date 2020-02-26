use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};
use syntax::source_map::Span;

use crate::consts::{constant_simple, Constant};
use crate::utils::span_lint;

declare_clippy_lint! {
    /// **What it does:** Checks for erasing operations, e.g., `x * 0`.
    ///
    /// **Why is this bad?** The whole expression can be replaced by zero.
    /// This is most likely not the intended outcome and should probably be
    /// corrected
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// let x = 1;
    /// 0 / x;
    /// 0 * x;
    /// x & 0;
    /// ```
    pub ERASING_OP,
    correctness,
    "using erasing operations, e.g., `x * 0` or `y & 0`"
}

declare_lint_pass!(ErasingOp => [ERASING_OP]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for ErasingOp {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, e: &'tcx Expr) {
        if e.span.from_expansion() {
            return;
        }
        if let ExprKind::Binary(ref cmp, ref left, ref right) = e.kind {
            match cmp.node {
                BinOpKind::Mul | BinOpKind::BitAnd => {
                    check(cx, left, e.span);
                    check(cx, right, e.span);
                },
                BinOpKind::Div => check(cx, left, e.span),
                _ => (),
            }
        }
    }
}

fn check(cx: &LateContext<'_, '_>, e: &Expr, span: Span) {
    if let Some(Constant::Int(0)) = constant_simple(cx, cx.tables, e) {
        span_lint(
            cx,
            ERASING_OP,
            span,
            "this operation will always return zero. This is likely not the intended outcome",
        );
    }
}
