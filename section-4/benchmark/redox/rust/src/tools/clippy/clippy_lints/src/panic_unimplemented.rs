use crate::utils::{is_direct_expn_of, is_expn_of, match_function_call, paths, span_lint};
use if_chain::if_chain;
use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};
use syntax::ast::LitKind;
use syntax_pos::Span;

declare_clippy_lint! {
    /// **What it does:** Checks for missing parameters in `panic!`.
    ///
    /// **Why is this bad?** Contrary to the `format!` family of macros, there are
    /// two forms of `panic!`: if there are no parameters given, the first argument
    /// is not a format string and used literally. So while `format!("{}")` will
    /// fail to compile, `panic!("{}")` will not.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```no_run
    /// panic!("This `panic!` is probably missing a parameter there: {}");
    /// ```
    pub PANIC_PARAMS,
    style,
    "missing parameters in `panic!` calls"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `panic!`.
    ///
    /// **Why is this bad?** `panic!` will stop the execution of the executable
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```no_run
    /// panic!("even with a good reason");
    /// ```
    pub PANIC,
    restriction,
    "usage of the `panic!` macro"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `unimplemented!`.
    ///
    /// **Why is this bad?** This macro should not be present in production code
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```no_run
    /// unimplemented!();
    /// ```
    pub UNIMPLEMENTED,
    restriction,
    "`unimplemented!` should not be present in production code"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `todo!`.
    ///
    /// **Why is this bad?** This macro should not be present in production code
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```no_run
    /// todo!();
    /// ```
    pub TODO,
    restriction,
    "`todo!` should not be present in production code"
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `unreachable!`.
    ///
    /// **Why is this bad?** This macro can cause code to panic
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```no_run
    /// unreachable!();
    /// ```
    pub UNREACHABLE,
    restriction,
    "`unreachable!` should not be present in production code"
}

declare_lint_pass!(PanicUnimplemented => [PANIC_PARAMS, UNIMPLEMENTED, UNREACHABLE, TODO, PANIC]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for PanicUnimplemented {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        if_chain! {
            if let ExprKind::Block(ref block, _) = expr.kind;
            if let Some(ref ex) = block.expr;
            if let Some(params) = match_function_call(cx, ex, &paths::BEGIN_PANIC);
            if params.len() == 2;
            then {
                if is_expn_of(expr.span, "unimplemented").is_some() {
                    let span = get_outer_span(expr);
                    span_lint(cx, UNIMPLEMENTED, span,
                              "`unimplemented` should not be present in production code");
                } else if is_expn_of(expr.span, "todo").is_some() {
                    let span = get_outer_span(expr);
                    span_lint(cx, TODO, span,
                              "`todo` should not be present in production code");
                } else if is_expn_of(expr.span, "unreachable").is_some() {
                    let span = get_outer_span(expr);
                    span_lint(cx, UNREACHABLE, span,
                              "`unreachable` should not be present in production code");
                } else if is_expn_of(expr.span, "panic").is_some() {
                    let span = get_outer_span(expr);
                    span_lint(cx, PANIC, span,
                              "`panic` should not be present in production code");
                    match_panic(params, expr, cx);
                }
            }
        }
    }
}

fn get_outer_span(expr: &Expr) -> Span {
    if_chain! {
        if expr.span.from_expansion();
        let first = expr.span.ctxt().outer_expn_data();
        if first.call_site.from_expansion();
        let second = first.call_site.ctxt().outer_expn_data();
        then {
            second.call_site
        } else {
            expr.span
        }
    }
}

fn match_panic(params: &[Expr], expr: &Expr, cx: &LateContext<'_, '_>) {
    if_chain! {
        if let ExprKind::Lit(ref lit) = params[0].kind;
        if is_direct_expn_of(expr.span, "panic").is_some();
        if let LitKind::Str(ref string, _) = lit.node;
        let string = string.as_str().replace("{{", "").replace("}}", "");
        if let Some(par) = string.find('{');
        if string[par..].contains('}');
        if params[0].span.source_callee().is_none();
        if params[0].span.lo() != params[0].span.hi();
        then {
            span_lint(cx, PANIC_PARAMS, params[0].span,
                      "you probably are missing some parameter in your format string");
        }
    }
}
