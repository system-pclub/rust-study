#![feature(box_syntax, plugin, plugin_registrar, rustc_private)]
#![crate_type = "dylib"]

#[macro_use]
extern crate rustc;
extern crate rustc_driver;
extern crate syntax;

use rustc_driver::plugin::Registry;
use rustc::hir::{self, intravisit, Node};
use rustc::lint::{LateContext, LintPass, LintArray, LateLintPass, LintContext};
use syntax::print::pprust;
use syntax::source_map;

#[plugin_registrar]
pub fn plugin_registrar(reg: &mut Registry) {
    reg.lint_store.register_lints(&[&MISSING_WHITELISTED_ATTR]);
    reg.lint_store.register_late_pass(|| box MissingWhitelistedAttrPass);
}

declare_lint! {
    MISSING_WHITELISTED_ATTR,
    Deny,
    "Checks for missing `whitelisted_attr` attribute"
}

declare_lint_pass!(MissingWhitelistedAttrPass => [MISSING_WHITELISTED_ATTR]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for MissingWhitelistedAttrPass {
    fn check_fn(&mut self,
                cx: &LateContext<'a, 'tcx>,
                _: intravisit::FnKind<'tcx>,
                _: &'tcx hir::FnDecl,
                _: &'tcx hir::Body,
                span: source_map::Span,
                id: hir::HirId) {

        let item = match cx.tcx.hir().get(id) {
            Node::Item(item) => item,
            _ => cx.tcx.hir().expect_item(cx.tcx.hir().get_parent_item(id)),
        };

        let whitelisted = |attr| pprust::attribute_to_string(attr).contains("whitelisted_attr");
        if !item.attrs.iter().any(whitelisted) {
            cx.span_lint(MISSING_WHITELISTED_ATTR, span,
                         "Missing 'whitelisted_attr' attribute");
        }
    }
}
