use crate::utils::span_lint;
use rustc::lint::{EarlyContext, EarlyLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};
use syntax::ast::*;

declare_clippy_lint! {
    /// **What it does:** Checks for unnecessary double parentheses.
    ///
    /// **Why is this bad?** This makes code harder to read and might indicate a
    /// mistake.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// # fn foo(bar: usize) {}
    /// ((0));
    /// foo((0));
    /// ((1, 2));
    /// ```
    pub DOUBLE_PARENS,
    complexity,
    "Warn on unnecessary double parentheses"
}

declare_lint_pass!(DoubleParens => [DOUBLE_PARENS]);

impl EarlyLintPass for DoubleParens {
    fn check_expr(&mut self, cx: &EarlyContext<'_>, expr: &Expr) {
        if expr.span.from_expansion() {
            return;
        }

        match expr.kind {
            ExprKind::Paren(ref in_paren) => match in_paren.kind {
                ExprKind::Paren(_) | ExprKind::Tup(_) => {
                    span_lint(
                        cx,
                        DOUBLE_PARENS,
                        expr.span,
                        "Consider removing unnecessary double parentheses",
                    );
                },
                _ => {},
            },
            ExprKind::Call(_, ref params) => {
                if params.len() == 1 {
                    let param = &params[0];
                    if let ExprKind::Paren(_) = param.kind {
                        span_lint(
                            cx,
                            DOUBLE_PARENS,
                            param.span,
                            "Consider removing unnecessary double parentheses",
                        );
                    }
                }
            },
            ExprKind::MethodCall(_, ref params) => {
                if params.len() == 2 {
                    let param = &params[1];
                    if let ExprKind::Paren(_) = param.kind {
                        span_lint(
                            cx,
                            DOUBLE_PARENS,
                            param.span,
                            "Consider removing unnecessary double parentheses",
                        );
                    }
                }
            },
            _ => {},
        }
    }
}
