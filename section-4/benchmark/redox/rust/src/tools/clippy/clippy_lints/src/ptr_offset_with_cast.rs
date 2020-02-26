use crate::utils;
use rustc::hir::{Expr, ExprKind};
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};
use rustc_errors::Applicability;
use std::fmt;

declare_clippy_lint! {
    /// **What it does:** Checks for usage of the `offset` pointer method with a `usize` casted to an
    /// `isize`.
    ///
    /// **Why is this bad?** If we’re always increasing the pointer address, we can avoid the numeric
    /// cast by using the `add` method instead.
    ///
    /// **Known problems:** None
    ///
    /// **Example:**
    /// ```rust
    /// let vec = vec![b'a', b'b', b'c'];
    /// let ptr = vec.as_ptr();
    /// let offset = 1_usize;
    ///
    /// unsafe {
    ///     ptr.offset(offset as isize);
    /// }
    /// ```
    ///
    /// Could be written:
    ///
    /// ```rust
    /// let vec = vec![b'a', b'b', b'c'];
    /// let ptr = vec.as_ptr();
    /// let offset = 1_usize;
    ///
    /// unsafe {
    ///     ptr.add(offset);
    /// }
    /// ```
    pub PTR_OFFSET_WITH_CAST,
    complexity,
    "unneeded pointer offset cast"
}

declare_lint_pass!(PtrOffsetWithCast => [PTR_OFFSET_WITH_CAST]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for PtrOffsetWithCast {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        // Check if the expressions is a ptr.offset or ptr.wrapping_offset method call
        let (receiver_expr, arg_expr, method) = match expr_as_ptr_offset_call(cx, expr) {
            Some(call_arg) => call_arg,
            None => return,
        };

        // Check if the argument to the method call is a cast from usize
        let cast_lhs_expr = match expr_as_cast_from_usize(cx, arg_expr) {
            Some(cast_lhs_expr) => cast_lhs_expr,
            None => return,
        };

        let msg = format!("use of `{}` with a `usize` casted to an `isize`", method);
        if let Some(sugg) = build_suggestion(cx, method, receiver_expr, cast_lhs_expr) {
            utils::span_lint_and_sugg(
                cx,
                PTR_OFFSET_WITH_CAST,
                expr.span,
                &msg,
                "try",
                sugg,
                Applicability::MachineApplicable,
            );
        } else {
            utils::span_lint(cx, PTR_OFFSET_WITH_CAST, expr.span, &msg);
        }
    }
}

// If the given expression is a cast from a usize, return the lhs of the cast
fn expr_as_cast_from_usize<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) -> Option<&'tcx Expr> {
    if let ExprKind::Cast(ref cast_lhs_expr, _) = expr.kind {
        if is_expr_ty_usize(cx, &cast_lhs_expr) {
            return Some(cast_lhs_expr);
        }
    }
    None
}

// If the given expression is a ptr::offset  or ptr::wrapping_offset method call, return the
// receiver, the arg of the method call, and the method.
fn expr_as_ptr_offset_call<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    expr: &'tcx Expr,
) -> Option<(&'tcx Expr, &'tcx Expr, Method)> {
    if let ExprKind::MethodCall(ref path_segment, _, ref args) = expr.kind {
        if is_expr_ty_raw_ptr(cx, &args[0]) {
            if path_segment.ident.name == sym!(offset) {
                return Some((&args[0], &args[1], Method::Offset));
            }
            if path_segment.ident.name == sym!(wrapping_offset) {
                return Some((&args[0], &args[1], Method::WrappingOffset));
            }
        }
    }
    None
}

// Is the type of the expression a usize?
fn is_expr_ty_usize<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &Expr) -> bool {
    cx.tables.expr_ty(expr) == cx.tcx.types.usize
}

// Is the type of the expression a raw pointer?
fn is_expr_ty_raw_ptr<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, expr: &Expr) -> bool {
    cx.tables.expr_ty(expr).is_unsafe_ptr()
}

fn build_suggestion<'a, 'tcx>(
    cx: &LateContext<'a, 'tcx>,
    method: Method,
    receiver_expr: &Expr,
    cast_lhs_expr: &Expr,
) -> Option<String> {
    let receiver = utils::snippet_opt(cx, receiver_expr.span)?;
    let cast_lhs = utils::snippet_opt(cx, cast_lhs_expr.span)?;
    Some(format!("{}.{}({})", receiver, method.suggestion(), cast_lhs))
}

#[derive(Copy, Clone)]
enum Method {
    Offset,
    WrappingOffset,
}

impl Method {
    #[must_use]
    fn suggestion(self) -> &'static str {
        match self {
            Self::Offset => "add",
            Self::WrappingOffset => "wrapping_add",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Offset => write!(f, "offset"),
            Self::WrappingOffset => write!(f, "wrapping_offset"),
        }
    }
}
