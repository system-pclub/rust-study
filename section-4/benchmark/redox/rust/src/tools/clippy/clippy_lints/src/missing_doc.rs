// Note: More specifically this lint is largely inspired (aka copied) from
// *rustc*'s
// [`missing_doc`].
//
// [`missing_doc`]: https://github.com/rust-lang/rust/blob/d6d05904697d89099b55da3331155392f1db9c00/src/librustc_lint/builtin.rs#L246
//

use crate::utils::span_lint;
use if_chain::if_chain;
use rustc::hir;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintContext, LintPass};
use rustc::ty;
use rustc::{declare_tool_lint, impl_lint_pass};
use syntax::ast::{self, MetaItem, MetaItemKind};
use syntax::attr;
use syntax::source_map::Span;

declare_clippy_lint! {
    /// **What it does:** Warns if there is missing doc for any documentable item
    /// (public or private).
    ///
    /// **Why is this bad?** Doc is good. *rustc* has a `MISSING_DOCS`
    /// allowed-by-default lint for
    /// public members, but has no way to enforce documentation of private items.
    /// This lint fixes that.
    ///
    /// **Known problems:** None.
    pub MISSING_DOCS_IN_PRIVATE_ITEMS,
    restriction,
    "detects missing documentation for public and private members"
}

pub struct MissingDoc {
    /// Stack of whether #[doc(hidden)] is set
    /// at each level which has lint attributes.
    doc_hidden_stack: Vec<bool>,
}

impl ::std::default::Default for MissingDoc {
    #[must_use]
    fn default() -> Self {
        Self::new()
    }
}

impl MissingDoc {
    #[must_use]
    pub fn new() -> Self {
        Self {
            doc_hidden_stack: vec![false],
        }
    }

    fn doc_hidden(&self) -> bool {
        *self.doc_hidden_stack.last().expect("empty doc_hidden_stack")
    }

    fn has_include(meta: Option<MetaItem>) -> bool {
        if_chain! {
            if let Some(meta) = meta;
            if let MetaItemKind::List(list) = meta.kind;
            if let Some(meta) = list.get(0);
            if let Some(name) = meta.ident();
            then {
                name.as_str() == "include"
            } else {
                false
            }
        }
    }

    fn check_missing_docs_attrs(
        &self,
        cx: &LateContext<'_, '_>,
        attrs: &[ast::Attribute],
        sp: Span,
        desc: &'static str,
    ) {
        // If we're building a test harness, then warning about
        // documentation is probably not really relevant right now.
        if cx.sess().opts.test {
            return;
        }

        // `#[doc(hidden)]` disables missing_docs check.
        if self.doc_hidden() {
            return;
        }

        if sp.from_expansion() {
            return;
        }

        let has_doc = attrs
            .iter()
            .any(|a| a.check_name(sym!(doc)) && (a.is_value_str() || Self::has_include(a.meta())));
        if !has_doc {
            span_lint(
                cx,
                MISSING_DOCS_IN_PRIVATE_ITEMS,
                sp,
                &format!("missing documentation for {}", desc),
            );
        }
    }
}

impl_lint_pass!(MissingDoc => [MISSING_DOCS_IN_PRIVATE_ITEMS]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for MissingDoc {
    fn enter_lint_attrs(&mut self, _: &LateContext<'a, 'tcx>, attrs: &'tcx [ast::Attribute]) {
        let doc_hidden = self.doc_hidden()
            || attrs.iter().any(|attr| {
                attr.check_name(sym!(doc))
                    && match attr.meta_item_list() {
                        None => false,
                        Some(l) => attr::list_contains_name(&l[..], sym!(hidden)),
                    }
            });
        self.doc_hidden_stack.push(doc_hidden);
    }

    fn exit_lint_attrs(&mut self, _: &LateContext<'a, 'tcx>, _: &'tcx [ast::Attribute]) {
        self.doc_hidden_stack.pop().expect("empty doc_hidden_stack");
    }

    fn check_crate(&mut self, cx: &LateContext<'a, 'tcx>, krate: &'tcx hir::Crate) {
        self.check_missing_docs_attrs(cx, &krate.attrs, krate.span, "crate");
    }

    fn check_item(&mut self, cx: &LateContext<'a, 'tcx>, it: &'tcx hir::Item) {
        let desc = match it.kind {
            hir::ItemKind::Const(..) => "a constant",
            hir::ItemKind::Enum(..) => "an enum",
            hir::ItemKind::Fn(..) => {
                // ignore main()
                if it.ident.name == sym!(main) {
                    let def_id = cx.tcx.hir().local_def_id(it.hir_id);
                    let def_key = cx.tcx.hir().def_key(def_id);
                    if def_key.parent == Some(hir::def_id::CRATE_DEF_INDEX) {
                        return;
                    }
                }
                "a function"
            },
            hir::ItemKind::Mod(..) => "a module",
            hir::ItemKind::Static(..) => "a static",
            hir::ItemKind::Struct(..) => "a struct",
            hir::ItemKind::Trait(..) => "a trait",
            hir::ItemKind::TraitAlias(..) => "a trait alias",
            hir::ItemKind::TyAlias(..) => "a type alias",
            hir::ItemKind::Union(..) => "a union",
            hir::ItemKind::OpaqueTy(..) => "an existential type",
            hir::ItemKind::ExternCrate(..)
            | hir::ItemKind::ForeignMod(..)
            | hir::ItemKind::GlobalAsm(..)
            | hir::ItemKind::Impl(..)
            | hir::ItemKind::Use(..) => return,
        };

        self.check_missing_docs_attrs(cx, &it.attrs, it.span, desc);
    }

    fn check_trait_item(&mut self, cx: &LateContext<'a, 'tcx>, trait_item: &'tcx hir::TraitItem) {
        let desc = match trait_item.kind {
            hir::TraitItemKind::Const(..) => "an associated constant",
            hir::TraitItemKind::Method(..) => "a trait method",
            hir::TraitItemKind::Type(..) => "an associated type",
        };

        self.check_missing_docs_attrs(cx, &trait_item.attrs, trait_item.span, desc);
    }

    fn check_impl_item(&mut self, cx: &LateContext<'a, 'tcx>, impl_item: &'tcx hir::ImplItem) {
        // If the method is an impl for a trait, don't doc.
        let def_id = cx.tcx.hir().local_def_id(impl_item.hir_id);
        match cx.tcx.associated_item(def_id).container {
            ty::TraitContainer(_) => return,
            ty::ImplContainer(cid) => {
                if cx.tcx.impl_trait_ref(cid).is_some() {
                    return;
                }
            },
        }

        let desc = match impl_item.kind {
            hir::ImplItemKind::Const(..) => "an associated constant",
            hir::ImplItemKind::Method(..) => "a method",
            hir::ImplItemKind::TyAlias(_) => "an associated type",
            hir::ImplItemKind::OpaqueTy(_) => "an existential type",
        };
        self.check_missing_docs_attrs(cx, &impl_item.attrs, impl_item.span, desc);
    }

    fn check_struct_field(&mut self, cx: &LateContext<'a, 'tcx>, sf: &'tcx hir::StructField) {
        if !sf.is_positional() {
            self.check_missing_docs_attrs(cx, &sf.attrs, sf.span, "a struct field");
        }
    }

    fn check_variant(&mut self, cx: &LateContext<'a, 'tcx>, v: &'tcx hir::Variant) {
        self.check_missing_docs_attrs(cx, &v.attrs, v.span, "a variant");
    }
}
