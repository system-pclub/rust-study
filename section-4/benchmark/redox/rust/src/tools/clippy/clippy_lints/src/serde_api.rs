use crate::utils::{get_trait_def_id, paths, span_lint};
use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};

declare_clippy_lint! {
    /// **What it does:** Checks for mis-uses of the serde API.
    ///
    /// **Why is this bad?** Serde is very finnicky about how its API should be
    /// used, but the type system can't be used to enforce it (yet?).
    ///
    /// **Known problems:** None.
    ///
    /// **Example:** Implementing `Visitor::visit_string` but not
    /// `Visitor::visit_str`.
    pub SERDE_API_MISUSE,
    correctness,
    "various things that will negatively affect your serde experience"
}

declare_lint_pass!(SerdeAPI => [SERDE_API_MISUSE]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for SerdeAPI {
    fn check_item(&mut self, cx: &LateContext<'a, 'tcx>, item: &'tcx Item) {
        if let ItemKind::Impl(_, _, _, _, Some(ref trait_ref), _, ref items) = item.kind {
            let did = trait_ref.path.res.def_id();
            if let Some(visit_did) = get_trait_def_id(cx, &paths::SERDE_DE_VISITOR) {
                if did == visit_did {
                    let mut seen_str = None;
                    let mut seen_string = None;
                    for item in items {
                        match &*item.ident.as_str() {
                            "visit_str" => seen_str = Some(item.span),
                            "visit_string" => seen_string = Some(item.span),
                            _ => {},
                        }
                    }
                    if let Some(span) = seen_string {
                        if seen_str.is_none() {
                            span_lint(
                                cx,
                                SERDE_API_MISUSE,
                                span,
                                "you should not implement `visit_string` without also implementing `visit_str`",
                            );
                        }
                    }
                }
            }
        }
    }
}
