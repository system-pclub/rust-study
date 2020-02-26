use crate::utils::{in_macro, snippet, span_help_and_lint, SpanlessHash};
use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::{declare_tool_lint, impl_lint_pass};
use rustc_data_structures::fx::FxHashMap;

#[derive(Copy, Clone)]
pub struct TraitBounds;

declare_clippy_lint! {
    /// **What it does:** This lint warns about unnecessary type repetitions in trait bounds
    ///
    /// **Why is this bad?** Repeating the type for every bound makes the code
    /// less readable than combining the bounds
    ///
    /// **Example:**
    /// ```rust
    /// pub fn foo<T>(t: T) where T: Copy, T: Clone {}
    /// ```
    ///
    /// Could be written as:
    ///
    /// ```rust
    /// pub fn foo<T>(t: T) where T: Copy + Clone {}
    /// ```
    pub TYPE_REPETITION_IN_BOUNDS,
    pedantic,
    "Types are repeated unnecessary in trait bounds use `+` instead of using `T: _, T: _`"
}

impl_lint_pass!(TraitBounds => [TYPE_REPETITION_IN_BOUNDS]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for TraitBounds {
    fn check_generics(&mut self, cx: &LateContext<'a, 'tcx>, gen: &'tcx Generics) {
        if in_macro(gen.span) {
            return;
        }
        let hash = |ty| -> u64 {
            let mut hasher = SpanlessHash::new(cx, cx.tables);
            hasher.hash_ty(ty);
            hasher.finish()
        };
        let mut map = FxHashMap::default();
        for bound in &gen.where_clause.predicates {
            if let WherePredicate::BoundPredicate(ref p) = bound {
                let h = hash(&p.bounded_ty);
                if let Some(ref v) = map.insert(h, p.bounds.iter().collect::<Vec<_>>()) {
                    let mut hint_string = format!(
                        "consider combining the bounds: `{}:",
                        snippet(cx, p.bounded_ty.span, "_")
                    );
                    for b in v.iter() {
                        if let GenericBound::Trait(ref poly_trait_ref, _) = b {
                            let path = &poly_trait_ref.trait_ref.path;
                            hint_string.push_str(&format!(" {} +", path));
                        }
                    }
                    for b in p.bounds.iter() {
                        if let GenericBound::Trait(ref poly_trait_ref, _) = b {
                            let path = &poly_trait_ref.trait_ref.path;
                            hint_string.push_str(&format!(" {} +", path));
                        }
                    }
                    hint_string.truncate(hint_string.len() - 2);
                    hint_string.push('`');
                    span_help_and_lint(
                        cx,
                        TYPE_REPETITION_IN_BOUNDS,
                        p.span,
                        "this type has already been used as a bound predicate",
                        &hint_string,
                    );
                }
            }
        }
    }
}
