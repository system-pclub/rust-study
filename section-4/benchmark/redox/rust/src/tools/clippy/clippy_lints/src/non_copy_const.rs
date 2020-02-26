//! Checks for uses of const which the type is not `Freeze` (`Cell`-free).
//!
//! This lint is **deny** by default.

use std::ptr;

use rustc::hir::def::{DefKind, Res};
use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, Lint, LintArray, LintPass};
use rustc::ty::adjustment::Adjust;
use rustc::ty::{Ty, TypeFlags};
use rustc::{declare_lint_pass, declare_tool_lint};
use rustc_typeck::hir_ty_to_ty;
use syntax_pos::{InnerSpan, Span, DUMMY_SP};

use crate::utils::{in_constant, is_copy, qpath_res, span_lint_and_then};

declare_clippy_lint! {
    /// **What it does:** Checks for declaration of `const` items which is interior
    /// mutable (e.g., contains a `Cell`, `Mutex`, `AtomicXxxx`, etc.).
    ///
    /// **Why is this bad?** Consts are copied everywhere they are referenced, i.e.,
    /// every time you refer to the const a fresh instance of the `Cell` or `Mutex`
    /// or `AtomicXxxx` will be created, which defeats the whole purpose of using
    /// these types in the first place.
    ///
    /// The `const` should better be replaced by a `static` item if a global
    /// variable is wanted, or replaced by a `const fn` if a constructor is wanted.
    ///
    /// **Known problems:** A "non-constant" const item is a legacy way to supply an
    /// initialized value to downstream `static` items (e.g., the
    /// `std::sync::ONCE_INIT` constant). In this case the use of `const` is legit,
    /// and this lint should be suppressed.
    ///
    /// **Example:**
    /// ```rust
    /// use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    ///
    /// // Bad.
    /// const CONST_ATOM: AtomicUsize = AtomicUsize::new(12);
    /// CONST_ATOM.store(6, SeqCst); // the content of the atomic is unchanged
    /// assert_eq!(CONST_ATOM.load(SeqCst), 12); // because the CONST_ATOM in these lines are distinct
    ///
    /// // Good.
    /// static STATIC_ATOM: AtomicUsize = AtomicUsize::new(15);
    /// STATIC_ATOM.store(9, SeqCst);
    /// assert_eq!(STATIC_ATOM.load(SeqCst), 9); // use a `static` item to refer to the same instance
    /// ```
    pub DECLARE_INTERIOR_MUTABLE_CONST,
    correctness,
    "declaring const with interior mutability"
}

declare_clippy_lint! {
    /// **What it does:** Checks if `const` items which is interior mutable (e.g.,
    /// contains a `Cell`, `Mutex`, `AtomicXxxx`, etc.) has been borrowed directly.
    ///
    /// **Why is this bad?** Consts are copied everywhere they are referenced, i.e.,
    /// every time you refer to the const a fresh instance of the `Cell` or `Mutex`
    /// or `AtomicXxxx` will be created, which defeats the whole purpose of using
    /// these types in the first place.
    ///
    /// The `const` value should be stored inside a `static` item.
    ///
    /// **Known problems:** None
    ///
    /// **Example:**
    /// ```rust
    /// use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    /// const CONST_ATOM: AtomicUsize = AtomicUsize::new(12);
    ///
    /// // Bad.
    /// CONST_ATOM.store(6, SeqCst); // the content of the atomic is unchanged
    /// assert_eq!(CONST_ATOM.load(SeqCst), 12); // because the CONST_ATOM in these lines are distinct
    ///
    /// // Good.
    /// static STATIC_ATOM: AtomicUsize = CONST_ATOM;
    /// STATIC_ATOM.store(9, SeqCst);
    /// assert_eq!(STATIC_ATOM.load(SeqCst), 9); // use a `static` item to refer to the same instance
    /// ```
    pub BORROW_INTERIOR_MUTABLE_CONST,
    correctness,
    "referencing const with interior mutability"
}

#[allow(dead_code)]
#[derive(Copy, Clone)]
enum Source {
    Item { item: Span },
    Assoc { item: Span, ty: Span },
    Expr { expr: Span },
}

impl Source {
    #[must_use]
    fn lint(&self) -> (&'static Lint, &'static str, Span) {
        match self {
            Self::Item { item } | Self::Assoc { item, .. } => (
                DECLARE_INTERIOR_MUTABLE_CONST,
                "a const item should never be interior mutable",
                *item,
            ),
            Self::Expr { expr } => (
                BORROW_INTERIOR_MUTABLE_CONST,
                "a const item with interior mutability should not be borrowed",
                *expr,
            ),
        }
    }
}

fn verify_ty_bound<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, ty: Ty<'tcx>, source: Source) {
    if ty.is_freeze(cx.tcx, cx.param_env, DUMMY_SP) || is_copy(cx, ty) {
        // An `UnsafeCell` is `!Copy`, and an `UnsafeCell` is also the only type which
        // is `!Freeze`, thus if our type is `Copy` we can be sure it must be `Freeze`
        // as well.
        return;
    }

    let (lint, msg, span) = source.lint();
    span_lint_and_then(cx, lint, span, msg, |db| {
        if span.from_expansion() {
            return; // Don't give suggestions into macros.
        }
        match source {
            Source::Item { .. } => {
                let const_kw_span = span.from_inner(InnerSpan::new(0, 5));
                db.span_label(const_kw_span, "make this a static item (maybe with lazy_static)");
            },
            Source::Assoc { ty: ty_span, .. } => {
                if ty.flags.contains(TypeFlags::HAS_FREE_LOCAL_NAMES) {
                    db.span_label(ty_span, &format!("consider requiring `{}` to be `Copy`", ty));
                }
            },
            Source::Expr { .. } => {
                db.help("assign this const to a local or static variable, and use the variable here");
            },
        }
    });
}

declare_lint_pass!(NonCopyConst => [DECLARE_INTERIOR_MUTABLE_CONST, BORROW_INTERIOR_MUTABLE_CONST]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for NonCopyConst {
    fn check_item(&mut self, cx: &LateContext<'a, 'tcx>, it: &'tcx Item) {
        if let ItemKind::Const(hir_ty, ..) = &it.kind {
            let ty = hir_ty_to_ty(cx.tcx, hir_ty);
            verify_ty_bound(cx, ty, Source::Item { item: it.span });
        }
    }

    fn check_trait_item(&mut self, cx: &LateContext<'a, 'tcx>, trait_item: &'tcx TraitItem) {
        if let TraitItemKind::Const(hir_ty, ..) = &trait_item.kind {
            let ty = hir_ty_to_ty(cx.tcx, hir_ty);
            verify_ty_bound(
                cx,
                ty,
                Source::Assoc {
                    ty: hir_ty.span,
                    item: trait_item.span,
                },
            );
        }
    }

    fn check_impl_item(&mut self, cx: &LateContext<'a, 'tcx>, impl_item: &'tcx ImplItem) {
        if let ImplItemKind::Const(hir_ty, ..) = &impl_item.kind {
            let item_hir_id = cx.tcx.hir().get_parent_node(impl_item.hir_id);
            let item = cx.tcx.hir().expect_item(item_hir_id);
            // Ensure the impl is an inherent impl.
            if let ItemKind::Impl(_, _, _, _, None, _, _) = item.kind {
                let ty = hir_ty_to_ty(cx.tcx, hir_ty);
                verify_ty_bound(
                    cx,
                    ty,
                    Source::Assoc {
                        ty: hir_ty.span,
                        item: impl_item.span,
                    },
                );
            }
        }
    }

    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        if let ExprKind::Path(qpath) = &expr.kind {
            // Only lint if we use the const item inside a function.
            if in_constant(cx, expr.hir_id) {
                return;
            }

            // Make sure it is a const item.
            match qpath_res(cx, qpath, expr.hir_id) {
                Res::Def(DefKind::Const, _) | Res::Def(DefKind::AssocConst, _) => {},
                _ => return,
            };

            // Climb up to resolve any field access and explicit referencing.
            let mut cur_expr = expr;
            let mut dereferenced_expr = expr;
            let mut needs_check_adjustment = true;
            loop {
                let parent_id = cx.tcx.hir().get_parent_node(cur_expr.hir_id);
                if parent_id == cur_expr.hir_id {
                    break;
                }
                if let Some(Node::Expr(parent_expr)) = cx.tcx.hir().find(parent_id) {
                    match &parent_expr.kind {
                        ExprKind::AddrOf(..) => {
                            // `&e` => `e` must be referenced.
                            needs_check_adjustment = false;
                        },
                        ExprKind::Field(..) => {
                            dereferenced_expr = parent_expr;
                            needs_check_adjustment = true;
                        },
                        ExprKind::Index(e, _) if ptr::eq(&**e, cur_expr) => {
                            // `e[i]` => desugared to `*Index::index(&e, i)`,
                            // meaning `e` must be referenced.
                            // no need to go further up since a method call is involved now.
                            needs_check_adjustment = false;
                            break;
                        },
                        ExprKind::Unary(UnDeref, _) => {
                            // `*e` => desugared to `*Deref::deref(&e)`,
                            // meaning `e` must be referenced.
                            // no need to go further up since a method call is involved now.
                            needs_check_adjustment = false;
                            break;
                        },
                        _ => break,
                    }
                    cur_expr = parent_expr;
                } else {
                    break;
                }
            }

            let ty = if needs_check_adjustment {
                let adjustments = cx.tables.expr_adjustments(dereferenced_expr);
                if let Some(i) = adjustments.iter().position(|adj| match adj.kind {
                    Adjust::Borrow(_) | Adjust::Deref(_) => true,
                    _ => false,
                }) {
                    if i == 0 {
                        cx.tables.expr_ty(dereferenced_expr)
                    } else {
                        adjustments[i - 1].target
                    }
                } else {
                    // No borrow adjustments means the entire const is moved.
                    return;
                }
            } else {
                cx.tables.expr_ty(dereferenced_expr)
            };

            verify_ty_bound(cx, ty, Source::Expr { expr: expr.span });
        }
    }
}
