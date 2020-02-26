use crate::utils::{if_sequence, parent_node_is_if_expr, span_help_and_lint, SpanlessEq};
use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};

declare_clippy_lint! {
    /// **What it does:** Checks comparison chains written with `if` that can be
    /// rewritten with `match` and `cmp`.
    ///
    /// **Why is this bad?** `if` is not guaranteed to be exhaustive and conditionals can get
    /// repetitive
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust,ignore
    /// # fn a() {}
    /// # fn b() {}
    /// # fn c() {}
    /// fn f(x: u8, y: u8) {
    ///     if x > y {
    ///         a()
    ///     } else if x < y {
    ///         b()
    ///     } else {
    ///         c()
    ///     }
    /// }
    /// ```
    ///
    /// Could be written:
    ///
    /// ```rust,ignore
    /// use std::cmp::Ordering;
    /// # fn a() {}
    /// # fn b() {}
    /// # fn c() {}
    /// fn f(x: u8, y: u8) {
    ///      match x.cmp(&y) {
    ///          Ordering::Greater => a(),
    ///          Ordering::Less => b(),
    ///          Ordering::Equal => c()
    ///      }
    /// }
    /// ```
    pub COMPARISON_CHAIN,
    style,
    "`if`s that can be rewritten with `match` and `cmp`"
}

declare_lint_pass!(ComparisonChain => [COMPARISON_CHAIN]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for ComparisonChain {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        if expr.span.from_expansion() {
            return;
        }

        // We only care about the top-most `if` in the chain
        if parent_node_is_if_expr(expr, cx) {
            return;
        }

        // Check that there exists at least one explicit else condition
        let (conds, _) = if_sequence(expr);
        if conds.len() < 2 {
            return;
        }

        for cond in conds.windows(2) {
            if let (
                &ExprKind::Binary(ref kind1, ref lhs1, ref rhs1),
                &ExprKind::Binary(ref kind2, ref lhs2, ref rhs2),
            ) = (&cond[0].kind, &cond[1].kind)
            {
                if !kind_is_cmp(kind1.node) || !kind_is_cmp(kind2.node) {
                    return;
                }

                // Check that both sets of operands are equal
                let mut spanless_eq = SpanlessEq::new(cx);
                if (!spanless_eq.eq_expr(lhs1, lhs2) || !spanless_eq.eq_expr(rhs1, rhs2))
                    && (!spanless_eq.eq_expr(lhs1, rhs2) || !spanless_eq.eq_expr(rhs1, lhs2))
                {
                    return;
                }
            } else {
                // We only care about comparison chains
                return;
            }
        }
        span_help_and_lint(
            cx,
            COMPARISON_CHAIN,
            expr.span,
            "`if` chain can be rewritten with `match`",
            "Consider rewriting the `if` chain to use `cmp` and `match`.",
        )
    }
}

fn kind_is_cmp(kind: BinOpKind) -> bool {
    match kind {
        BinOpKind::Lt | BinOpKind::Gt | BinOpKind::Eq => true,
        _ => false,
    }
}
