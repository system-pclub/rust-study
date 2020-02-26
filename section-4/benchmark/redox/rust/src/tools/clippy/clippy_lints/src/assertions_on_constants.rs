use crate::consts::{constant, Constant};
use crate::utils::paths;
use crate::utils::{is_direct_expn_of, is_expn_of, match_function_call, snippet_opt, span_help_and_lint};
use if_chain::if_chain;
use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};
use syntax::ast::LitKind;

declare_clippy_lint! {
    /// **What it does:** Checks for `assert!(true)` and `assert!(false)` calls.
    ///
    /// **Why is this bad?** Will be optimized out by the compiler or should probably be replaced by a
    /// panic!() or unreachable!()
    ///
    /// **Known problems:** None
    ///
    /// **Example:**
    /// ```rust,ignore
    /// assert!(false)
    /// // or
    /// assert!(true)
    /// // or
    /// const B: bool = false;
    /// assert!(B)
    /// ```
    pub ASSERTIONS_ON_CONSTANTS,
    style,
    "`assert!(true)` / `assert!(false)` will be optimized out by the compiler, and should probably be replaced by a `panic!()` or `unreachable!()`"
}

declare_lint_pass!(AssertionsOnConstants => [ASSERTIONS_ON_CONSTANTS]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for AssertionsOnConstants {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, e: &'tcx Expr) {
        let lint_true = || {
            span_help_and_lint(
                cx,
                ASSERTIONS_ON_CONSTANTS,
                e.span,
                "`assert!(true)` will be optimized out by the compiler",
                "remove it",
            );
        };
        let lint_false_without_message = || {
            span_help_and_lint(
                cx,
                ASSERTIONS_ON_CONSTANTS,
                e.span,
                "`assert!(false)` should probably be replaced",
                "use `panic!()` or `unreachable!()`",
            );
        };
        let lint_false_with_message = |panic_message: String| {
            span_help_and_lint(
                cx,
                ASSERTIONS_ON_CONSTANTS,
                e.span,
                &format!("`assert!(false, {})` should probably be replaced", panic_message),
                &format!("use `panic!({})` or `unreachable!({})`", panic_message, panic_message),
            )
        };

        if let Some(debug_assert_span) = is_expn_of(e.span, "debug_assert") {
            if debug_assert_span.from_expansion() {
                return;
            }
            if_chain! {
                if let ExprKind::Unary(_, ref lit) = e.kind;
                if let Some((Constant::Bool(is_true), _)) = constant(cx, cx.tables, lit);
                if is_true;
                then {
                    lint_true();
                }
            };
        } else if let Some(assert_span) = is_direct_expn_of(e.span, "assert") {
            if assert_span.from_expansion() {
                return;
            }
            if let Some(assert_match) = match_assert_with_message(&cx, e) {
                match assert_match {
                    // matched assert but not message
                    AssertKind::WithoutMessage(false) => lint_false_without_message(),
                    AssertKind::WithoutMessage(true) | AssertKind::WithMessage(_, true) => lint_true(),
                    AssertKind::WithMessage(panic_message, false) => lint_false_with_message(panic_message),
                };
            }
        }
    }
}

/// Result of calling `match_assert_with_message`.
enum AssertKind {
    WithMessage(String, bool),
    WithoutMessage(bool),
}

/// Check if the expression matches
///
/// ```rust,ignore
/// match { let _t = !c; _t } {
///     true => {
///         {
///             ::std::rt::begin_panic(message, _)
///         }
///     }
///     _ => { }
/// };
/// ```
///
/// where `message` is any expression and `c` is a constant bool.
fn match_assert_with_message<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) -> Option<AssertKind> {
    if_chain! {
        if let ExprKind::Match(ref expr, ref arms, _) = expr.kind;
        // matches { let _t = expr; _t }
        if let ExprKind::DropTemps(ref expr) = expr.kind;
        if let ExprKind::Unary(UnOp::UnNot, ref expr) = expr.kind;
        // bind the first argument of the `assert!` macro
        if let Some((Constant::Bool(is_true), _)) = constant(cx, cx.tables, expr);
        // arm 1 pattern
        if let PatKind::Lit(ref lit_expr) = arms[0].pat.kind;
        if let ExprKind::Lit(ref lit) = lit_expr.kind;
        if let LitKind::Bool(true) = lit.node;
        // arm 1 block
        if let ExprKind::Block(ref block, _) = arms[0].body.kind;
        if block.stmts.len() == 0;
        if let Some(block_expr) = &block.expr;
        if let ExprKind::Block(ref inner_block, _) = block_expr.kind;
        if let Some(begin_panic_call) = &inner_block.expr;
        // function call
        if let Some(args) = match_function_call(cx, begin_panic_call, &paths::BEGIN_PANIC);
        if args.len() == 2;
        // bind the second argument of the `assert!` macro if it exists
        if let panic_message = snippet_opt(cx, args[0].span);
        // second argument of begin_panic is irrelevant
        // as is the second match arm
        then {
            // an empty message occurs when it was generated by the macro
            // (and not passed by the user)
            return panic_message
                .filter(|msg| !msg.is_empty())
                .map(|msg| AssertKind::WithMessage(msg, is_true))
                .or(Some(AssertKind::WithoutMessage(is_true)));
        }
    }
    None
}
