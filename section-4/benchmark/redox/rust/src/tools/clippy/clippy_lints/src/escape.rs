use rustc::hir::intravisit as visit;
use rustc::hir::{self, *};
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::middle::expr_use_visitor::*;
use rustc::middle::mem_categorization::{cmt_, Categorization};
use rustc::ty::layout::LayoutOf;
use rustc::ty::{self, Ty};
use rustc::util::nodemap::HirIdSet;
use rustc::{declare_tool_lint, impl_lint_pass};
use syntax::source_map::Span;

use crate::utils::span_lint;

#[derive(Copy, Clone)]
pub struct BoxedLocal {
    pub too_large_for_stack: u64,
}

declare_clippy_lint! {
    /// **What it does:** Checks for usage of `Box<T>` where an unboxed `T` would
    /// work fine.
    ///
    /// **Why is this bad?** This is an unnecessary allocation, and bad for
    /// performance. It is only necessary to allocate if you wish to move the box
    /// into something.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// # fn foo(bar: usize) {}
    /// let x = Box::new(1);
    /// foo(*x);
    /// println!("{}", *x);
    /// ```
    pub BOXED_LOCAL,
    perf,
    "using `Box<T>` where unnecessary"
}

fn is_non_trait_box(ty: Ty<'_>) -> bool {
    ty.is_box() && !ty.boxed_ty().is_trait()
}

struct EscapeDelegate<'a, 'tcx> {
    cx: &'a LateContext<'a, 'tcx>,
    set: HirIdSet,
    too_large_for_stack: u64,
}

impl_lint_pass!(BoxedLocal => [BOXED_LOCAL]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for BoxedLocal {
    fn check_fn(
        &mut self,
        cx: &LateContext<'a, 'tcx>,
        _: visit::FnKind<'tcx>,
        _: &'tcx FnDecl,
        body: &'tcx Body,
        _: Span,
        hir_id: HirId,
    ) {
        // If the method is an impl for a trait, don't warn.
        let parent_id = cx.tcx.hir().get_parent_item(hir_id);
        let parent_node = cx.tcx.hir().find(parent_id);

        if let Some(Node::Item(item)) = parent_node {
            if let ItemKind::Impl(_, _, _, _, Some(..), _, _) = item.kind {
                return;
            }
        }

        let mut v = EscapeDelegate {
            cx,
            set: HirIdSet::default(),
            too_large_for_stack: self.too_large_for_stack,
        };

        let fn_def_id = cx.tcx.hir().local_def_id(hir_id);
        let region_scope_tree = &cx.tcx.region_scope_tree(fn_def_id);
        ExprUseVisitor::new(&mut v, cx.tcx, fn_def_id, cx.param_env, region_scope_tree, cx.tables).consume_body(body);

        for node in v.set {
            span_lint(
                cx,
                BOXED_LOCAL,
                cx.tcx.hir().span(node),
                "local variable doesn't need to be boxed here",
            );
        }
    }
}

// TODO: Replace with Map::is_argument(..) when it's fixed
fn is_argument(map: &hir::map::Map<'_>, id: HirId) -> bool {
    match map.find(id) {
        Some(Node::Binding(_)) => (),
        _ => return false,
    }

    match map.find(map.get_parent_node(id)) {
        Some(Node::Param(_)) => true,
        _ => false,
    }
}

impl<'a, 'tcx> Delegate<'tcx> for EscapeDelegate<'a, 'tcx> {
    fn consume(&mut self, cmt: &cmt_<'tcx>, mode: ConsumeMode) {
        if let Categorization::Local(lid) = cmt.cat {
            if let ConsumeMode::Move = mode {
                // moved out or in. clearly can't be localized
                self.set.remove(&lid);
            }
        }
        let map = &self.cx.tcx.hir();
        if let Categorization::Local(lid) = cmt.cat {
            if let Some(Node::Binding(_)) = map.find(cmt.hir_id) {
                if self.set.contains(&lid) {
                    // let y = x where x is known
                    // remove x, insert y
                    self.set.insert(cmt.hir_id);
                    self.set.remove(&lid);
                }
            }
        }
    }

    fn borrow(&mut self, cmt: &cmt_<'tcx>, _: ty::BorrowKind) {
        if let Categorization::Local(lid) = cmt.cat {
            self.set.remove(&lid);
        }
    }

    fn mutate(&mut self, cmt: &cmt_<'tcx>) {
        let map = &self.cx.tcx.hir();
        if is_argument(map, cmt.hir_id) {
            // Skip closure arguments
            let parent_id = map.get_parent_node(cmt.hir_id);
            if let Some(Node::Expr(..)) = map.find(map.get_parent_node(parent_id)) {
                return;
            }

            if is_non_trait_box(cmt.ty) && !self.is_large_box(cmt.ty) {
                self.set.insert(cmt.hir_id);
            }
            return;
        }
    }
}

impl<'a, 'tcx> EscapeDelegate<'a, 'tcx> {
    fn is_large_box(&self, ty: Ty<'tcx>) -> bool {
        // Large types need to be boxed to avoid stack overflows.
        if ty.is_box() {
            self.cx.layout_of(ty.boxed_ty()).ok().map_or(0, |l| l.size.bytes()) > self.too_large_for_stack
        } else {
            false
        }
    }
}
