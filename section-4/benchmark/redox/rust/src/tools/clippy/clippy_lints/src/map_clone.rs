use crate::utils::paths;
use crate::utils::{
    is_copy, match_trait_method, match_type, remove_blocks, snippet_with_applicability, span_lint_and_sugg,
};
use if_chain::if_chain;
use rustc::hir;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::ty;
use rustc::{declare_lint_pass, declare_tool_lint};
use rustc_errors::Applicability;
use syntax::ast::Ident;
use syntax::source_map::Span;

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `iterator.map(|x| x.clone())` and suggests
    /// `iterator.cloned()` instead
    ///
    /// **Why is this bad?** Readability, this can be written more concisely
    ///
    /// **Known problems:** None
    ///
    /// **Example:**
    ///
    /// ```rust
    /// let x = vec![42, 43];
    /// let y = x.iter();
    /// let z = y.map(|i| *i);
    /// ```
    ///
    /// The correct use would be:
    ///
    /// ```rust
    /// let x = vec![42, 43];
    /// let y = x.iter();
    /// let z = y.cloned();
    /// ```
    pub MAP_CLONE,
    style,
    "using `iterator.map(|x| x.clone())`, or dereferencing closures for `Copy` types"
}

declare_lint_pass!(MapClone => [MAP_CLONE]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for MapClone {
    fn check_expr(&mut self, cx: &LateContext<'_, '_>, e: &hir::Expr) {
        if e.span.from_expansion() {
            return;
        }

        if_chain! {
            if let hir::ExprKind::MethodCall(ref method, _, ref args) = e.kind;
            if args.len() == 2;
            if method.ident.as_str() == "map";
            let ty = cx.tables.expr_ty(&args[0]);
            if match_type(cx, ty, &paths::OPTION) || match_trait_method(cx, e, &paths::ITERATOR);
            if let hir::ExprKind::Closure(_, _, body_id, _, _) = args[1].kind;
            let closure_body = cx.tcx.hir().body(body_id);
            let closure_expr = remove_blocks(&closure_body.value);
            then {
                match closure_body.params[0].pat.kind {
                    hir::PatKind::Ref(ref inner, _) => if let hir::PatKind::Binding(
                        hir::BindingAnnotation::Unannotated, .., name, None
                    ) = inner.kind {
                        if ident_eq(name, closure_expr) {
                            lint(cx, e.span, args[0].span, true);
                        }
                    },
                    hir::PatKind::Binding(hir::BindingAnnotation::Unannotated, .., name, None) => {
                        match closure_expr.kind {
                            hir::ExprKind::Unary(hir::UnOp::UnDeref, ref inner) => {
                                if ident_eq(name, inner) && !cx.tables.expr_ty(inner).is_box() {
                                    lint(cx, e.span, args[0].span, true);
                                }
                            },
                            hir::ExprKind::MethodCall(ref method, _, ref obj) => {
                                if ident_eq(name, &obj[0]) && method.ident.as_str() == "clone"
                                    && match_trait_method(cx, closure_expr, &paths::CLONE_TRAIT) {

                                    let obj_ty = cx.tables.expr_ty(&obj[0]);
                                    if let ty::Ref(_, ty, _) = obj_ty.kind {
                                        let copy = is_copy(cx, ty);
                                        lint(cx, e.span, args[0].span, copy);
                                    } else {
                                        lint_needless_cloning(cx, e.span, args[0].span);
                                    }
                                }
                            },
                            _ => {},
                        }
                    },
                    _ => {},
                }
            }
        }
    }
}

fn ident_eq(name: Ident, path: &hir::Expr) -> bool {
    if let hir::ExprKind::Path(hir::QPath::Resolved(None, ref path)) = path.kind {
        path.segments.len() == 1 && path.segments[0].ident == name
    } else {
        false
    }
}

fn lint_needless_cloning(cx: &LateContext<'_, '_>, root: Span, receiver: Span) {
    span_lint_and_sugg(
        cx,
        MAP_CLONE,
        root.trim_start(receiver).unwrap(),
        "You are needlessly cloning iterator elements",
        "Remove the map call",
        String::new(),
        Applicability::MachineApplicable,
    )
}

fn lint(cx: &LateContext<'_, '_>, replace: Span, root: Span, copied: bool) {
    let mut applicability = Applicability::MachineApplicable;
    if copied {
        span_lint_and_sugg(
            cx,
            MAP_CLONE,
            replace,
            "You are using an explicit closure for copying elements",
            "Consider calling the dedicated `copied` method",
            format!(
                "{}.copied()",
                snippet_with_applicability(cx, root, "..", &mut applicability)
            ),
            applicability,
        )
    } else {
        span_lint_and_sugg(
            cx,
            MAP_CLONE,
            replace,
            "You are using an explicit closure for cloning elements",
            "Consider calling the dedicated `cloned` method",
            format!(
                "{}.cloned()",
                snippet_with_applicability(cx, root, "..", &mut applicability)
            ),
            applicability,
        )
    }
}
