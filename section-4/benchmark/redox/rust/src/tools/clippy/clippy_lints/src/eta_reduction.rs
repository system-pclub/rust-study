use if_chain::if_chain;
use matches::matches;
use rustc::hir::*;
use rustc::lint::{in_external_macro, LateContext, LateLintPass, LintArray, LintContext, LintPass};
use rustc::ty::{self, Ty};
use rustc::{declare_lint_pass, declare_tool_lint};
use rustc_errors::Applicability;

use crate::utils::{
    implements_trait, is_adjusted, iter_input_pats, snippet_opt, span_lint_and_then, type_is_unsafe_function,
};

declare_clippy_lint! {
    /// **What it does:** Checks for closures which just call another function where
    /// the function can be called directly. `unsafe` functions or calls where types
    /// get adjusted are ignored.
    ///
    /// **Why is this bad?** Needlessly creating a closure adds code for no benefit
    /// and gives the optimizer more work.
    ///
    /// **Known problems:** If creating the closure inside the closure has a side-
    /// effect then moving the closure creation out will change when that side-
    /// effect runs.
    /// See rust-lang/rust-clippy#1439 for more details.
    ///
    /// **Example:**
    /// ```rust,ignore
    /// xs.map(|x| foo(x))
    /// ```
    /// where `foo(_)` is a plain function that takes the exact argument type of
    /// `x`.
    pub REDUNDANT_CLOSURE,
    style,
    "redundant closures, i.e., `|a| foo(a)` (which can be written as just `foo`)"
}

declare_clippy_lint! {
    /// **What it does:** Checks for closures which only invoke a method on the closure
    /// argument and can be replaced by referencing the method directly.
    ///
    /// **Why is this bad?** It's unnecessary to create the closure.
    ///
    /// **Known problems:** rust-lang/rust-clippy#3071, rust-lang/rust-clippy#4002,
    /// rust-lang/rust-clippy#3942
    ///
    ///
    /// **Example:**
    /// ```rust,ignore
    /// Some('a').map(|s| s.to_uppercase());
    /// ```
    /// may be rewritten as
    /// ```rust,ignore
    /// Some('a').map(char::to_uppercase);
    /// ```
    pub REDUNDANT_CLOSURE_FOR_METHOD_CALLS,
    pedantic,
    "redundant closures for method calls"
}

declare_lint_pass!(EtaReduction => [REDUNDANT_CLOSURE, REDUNDANT_CLOSURE_FOR_METHOD_CALLS]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for EtaReduction {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        if in_external_macro(cx.sess(), expr.span) {
            return;
        }

        match expr.kind {
            ExprKind::Call(_, ref args) | ExprKind::MethodCall(_, _, ref args) => {
                for arg in args {
                    check_closure(cx, arg)
                }
            },
            _ => (),
        }
    }
}

fn check_closure(cx: &LateContext<'_, '_>, expr: &Expr) {
    if let ExprKind::Closure(_, ref decl, eid, _, _) = expr.kind {
        let body = cx.tcx.hir().body(eid);
        let ex = &body.value;

        if_chain!(
            if let ExprKind::Call(ref caller, ref args) = ex.kind;

            if let ExprKind::Path(_) = caller.kind;

            // Not the same number of arguments, there is no way the closure is the same as the function return;
            if args.len() == decl.inputs.len();

            // Are the expression or the arguments type-adjusted? Then we need the closure
            if !(is_adjusted(cx, ex) || args.iter().any(|arg| is_adjusted(cx, arg)));

            let fn_ty = cx.tables.expr_ty(caller);

            if matches!(fn_ty.kind, ty::FnDef(_, _) | ty::FnPtr(_) | ty::Closure(_, _));

            if !type_is_unsafe_function(cx, fn_ty);

            if compare_inputs(&mut iter_input_pats(decl, body), &mut args.into_iter());

            then {
                span_lint_and_then(cx, REDUNDANT_CLOSURE, expr.span, "redundant closure found", |db| {
                    if let Some(snippet) = snippet_opt(cx, caller.span) {
                        db.span_suggestion(
                            expr.span,
                            "remove closure as shown",
                            snippet,
                            Applicability::MachineApplicable,
                        );
                    }
                });
            }
        );

        if_chain!(
            if let ExprKind::MethodCall(ref path, _, ref args) = ex.kind;

            // Not the same number of arguments, there is no way the closure is the same as the function return;
            if args.len() == decl.inputs.len();

            // Are the expression or the arguments type-adjusted? Then we need the closure
            if !(is_adjusted(cx, ex) || args.iter().skip(1).any(|arg| is_adjusted(cx, arg)));

            let method_def_id = cx.tables.type_dependent_def_id(ex.hir_id).unwrap();
            if !type_is_unsafe_function(cx, cx.tcx.type_of(method_def_id));

            if compare_inputs(&mut iter_input_pats(decl, body), &mut args.into_iter());

            if let Some(name) = get_ufcs_type_name(cx, method_def_id, &args[0]);

            then {
                span_lint_and_then(cx, REDUNDANT_CLOSURE_FOR_METHOD_CALLS, expr.span, "redundant closure found", |db| {
                    db.span_suggestion(
                        expr.span,
                        "remove closure as shown",
                        format!("{}::{}", name, path.ident.name),
                        Applicability::MachineApplicable,
                    );
                });
            }
        );
    }
}

/// Tries to determine the type for universal function call to be used instead of the closure
fn get_ufcs_type_name(
    cx: &LateContext<'_, '_>,
    method_def_id: def_id::DefId,
    self_arg: &Expr,
) -> std::option::Option<String> {
    let expected_type_of_self = &cx.tcx.fn_sig(method_def_id).inputs_and_output().skip_binder()[0];
    let actual_type_of_self = &cx.tables.node_type(self_arg.hir_id);

    if let Some(trait_id) = cx.tcx.trait_of_item(method_def_id) {
        if match_borrow_depth(expected_type_of_self, &actual_type_of_self)
            && implements_trait(cx, actual_type_of_self, trait_id, &[])
        {
            return Some(cx.tcx.def_path_str(trait_id));
        }
    }

    cx.tcx.impl_of_method(method_def_id).and_then(|_| {
        //a type may implicitly implement other type's methods (e.g. Deref)
        if match_types(expected_type_of_self, &actual_type_of_self) {
            return Some(get_type_name(cx, &actual_type_of_self));
        }
        None
    })
}

fn match_borrow_depth(lhs: Ty<'_>, rhs: Ty<'_>) -> bool {
    match (&lhs.kind, &rhs.kind) {
        (ty::Ref(_, t1, mut1), ty::Ref(_, t2, mut2)) => mut1 == mut2 && match_borrow_depth(&t1, &t2),
        (l, r) => match (l, r) {
            (ty::Ref(_, _, _), _) | (_, ty::Ref(_, _, _)) => false,
            (_, _) => true,
        },
    }
}

fn match_types(lhs: Ty<'_>, rhs: Ty<'_>) -> bool {
    match (&lhs.kind, &rhs.kind) {
        (ty::Bool, ty::Bool)
        | (ty::Char, ty::Char)
        | (ty::Int(_), ty::Int(_))
        | (ty::Uint(_), ty::Uint(_))
        | (ty::Str, ty::Str) => true,
        (ty::Ref(_, t1, mut1), ty::Ref(_, t2, mut2)) => mut1 == mut2 && match_types(t1, t2),
        (ty::Array(t1, _), ty::Array(t2, _)) | (ty::Slice(t1), ty::Slice(t2)) => match_types(t1, t2),
        (ty::Adt(def1, _), ty::Adt(def2, _)) => def1 == def2,
        (_, _) => false,
    }
}

fn get_type_name(cx: &LateContext<'_, '_>, ty: Ty<'_>) -> String {
    match ty.kind {
        ty::Adt(t, _) => cx.tcx.def_path_str(t.did),
        ty::Ref(_, r, _) => get_type_name(cx, &r),
        _ => ty.to_string(),
    }
}

fn compare_inputs(
    closure_inputs: &mut dyn Iterator<Item = &Param>,
    call_args: &mut dyn Iterator<Item = &Expr>,
) -> bool {
    for (closure_input, function_arg) in closure_inputs.zip(call_args) {
        if let PatKind::Binding(_, _, ident, _) = closure_input.pat.kind {
            // XXXManishearth Should I be checking the binding mode here?
            if let ExprKind::Path(QPath::Resolved(None, ref p)) = function_arg.kind {
                if p.segments.len() != 1 {
                    // If it's a proper path, it can't be a local variable
                    return false;
                }
                if p.segments[0].ident.name != ident.name {
                    // The two idents should be the same
                    return false;
                }
            } else {
                return false;
            }
        } else {
            return false;
        }
    }
    true
}
