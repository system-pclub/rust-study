use crate::utils::{get_parent_expr, higher, if_sequence, same_tys, snippet, span_lint_and_then, span_note_and_lint};
use crate::utils::{SpanlessEq, SpanlessHash};
use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::ty::Ty;
use rustc::{declare_lint_pass, declare_tool_lint};
use rustc_data_structures::fx::FxHashMap;
use std::collections::hash_map::Entry;
use std::hash::BuildHasherDefault;
use syntax::symbol::Symbol;

declare_clippy_lint! {
    /// **What it does:** Checks for consecutive `if`s with the same condition.
    ///
    /// **Why is this bad?** This is probably a copy & paste error.
    ///
    /// **Known problems:** Hopefully none.
    ///
    /// **Example:**
    /// ```ignore
    /// if a == b {
    ///     …
    /// } else if a == b {
    ///     …
    /// }
    /// ```
    ///
    /// Note that this lint ignores all conditions with a function call as it could
    /// have side effects:
    ///
    /// ```ignore
    /// if foo() {
    ///     …
    /// } else if foo() { // not linted
    ///     …
    /// }
    /// ```
    pub IFS_SAME_COND,
    correctness,
    "consecutive `ifs` with the same condition"
}

declare_clippy_lint! {
    /// **What it does:** Checks for `if/else` with the same body as the *then* part
    /// and the *else* part.
    ///
    /// **Why is this bad?** This is probably a copy & paste error.
    ///
    /// **Known problems:** Hopefully none.
    ///
    /// **Example:**
    /// ```ignore
    /// let foo = if … {
    ///     42
    /// } else {
    ///     42
    /// };
    /// ```
    pub IF_SAME_THEN_ELSE,
    correctness,
    "if with the same *then* and *else* blocks"
}

declare_clippy_lint! {
    /// **What it does:** Checks for `match` with identical arm bodies.
    ///
    /// **Why is this bad?** This is probably a copy & paste error. If arm bodies
    /// are the same on purpose, you can factor them
    /// [using `|`](https://doc.rust-lang.org/book/patterns.html#multiple-patterns).
    ///
    /// **Known problems:** False positive possible with order dependent `match`
    /// (see issue
    /// [#860](https://github.com/rust-lang/rust-clippy/issues/860)).
    ///
    /// **Example:**
    /// ```rust,ignore
    /// match foo {
    ///     Bar => bar(),
    ///     Quz => quz(),
    ///     Baz => bar(), // <= oops
    /// }
    /// ```
    ///
    /// This should probably be
    /// ```rust,ignore
    /// match foo {
    ///     Bar => bar(),
    ///     Quz => quz(),
    ///     Baz => baz(), // <= fixed
    /// }
    /// ```
    ///
    /// or if the original code was not a typo:
    /// ```rust,ignore
    /// match foo {
    ///     Bar | Baz => bar(), // <= shows the intent better
    ///     Quz => quz(),
    /// }
    /// ```
    pub MATCH_SAME_ARMS,
    pedantic,
    "`match` with identical arm bodies"
}

declare_lint_pass!(CopyAndPaste => [IFS_SAME_COND, IF_SAME_THEN_ELSE, MATCH_SAME_ARMS]);

impl<'a, 'tcx> LateLintPass<'a, 'tcx> for CopyAndPaste {
    fn check_expr(&mut self, cx: &LateContext<'a, 'tcx>, expr: &'tcx Expr) {
        if !expr.span.from_expansion() {
            // skip ifs directly in else, it will be checked in the parent if
            if let Some(expr) = get_parent_expr(cx, expr) {
                if let Some((_, _, Some(ref else_expr))) = higher::if_block(&expr) {
                    if else_expr.hir_id == expr.hir_id {
                        return;
                    }
                }
            }

            let (conds, blocks) = if_sequence(expr);
            lint_same_then_else(cx, &blocks);
            lint_same_cond(cx, &conds);
            lint_match_arms(cx, expr);
        }
    }
}

/// Implementation of `IF_SAME_THEN_ELSE`.
fn lint_same_then_else(cx: &LateContext<'_, '_>, blocks: &[&Block]) {
    let eq: &dyn Fn(&&Block, &&Block) -> bool = &|&lhs, &rhs| -> bool { SpanlessEq::new(cx).eq_block(lhs, rhs) };

    if let Some((i, j)) = search_same_sequenced(blocks, eq) {
        span_note_and_lint(
            cx,
            IF_SAME_THEN_ELSE,
            j.span,
            "this `if` has identical blocks",
            i.span,
            "same as this",
        );
    }
}

/// Implementation of `IFS_SAME_COND`.
fn lint_same_cond(cx: &LateContext<'_, '_>, conds: &[&Expr]) {
    let hash: &dyn Fn(&&Expr) -> u64 = &|expr| -> u64 {
        let mut h = SpanlessHash::new(cx, cx.tables);
        h.hash_expr(expr);
        h.finish()
    };

    let eq: &dyn Fn(&&Expr, &&Expr) -> bool =
        &|&lhs, &rhs| -> bool { SpanlessEq::new(cx).ignore_fn().eq_expr(lhs, rhs) };

    for (i, j) in search_same(conds, hash, eq) {
        span_note_and_lint(
            cx,
            IFS_SAME_COND,
            j.span,
            "this `if` has the same condition as a previous if",
            i.span,
            "same as this",
        );
    }
}

/// Implementation of `MATCH_SAME_ARMS`.
fn lint_match_arms<'tcx>(cx: &LateContext<'_, 'tcx>, expr: &Expr) {
    fn same_bindings<'tcx>(
        cx: &LateContext<'_, 'tcx>,
        lhs: &FxHashMap<Symbol, Ty<'tcx>>,
        rhs: &FxHashMap<Symbol, Ty<'tcx>>,
    ) -> bool {
        lhs.len() == rhs.len()
            && lhs
                .iter()
                .all(|(name, l_ty)| rhs.get(name).map_or(false, |r_ty| same_tys(cx, l_ty, r_ty)))
    }

    if let ExprKind::Match(_, ref arms, MatchSource::Normal) = expr.kind {
        let hash = |&(_, arm): &(usize, &Arm)| -> u64 {
            let mut h = SpanlessHash::new(cx, cx.tables);
            h.hash_expr(&arm.body);
            h.finish()
        };

        let eq = |&(lindex, lhs): &(usize, &Arm), &(rindex, rhs): &(usize, &Arm)| -> bool {
            let min_index = usize::min(lindex, rindex);
            let max_index = usize::max(lindex, rindex);

            // Arms with a guard are ignored, those can’t always be merged together
            // This is also the case for arms in-between each there is an arm with a guard
            (min_index..=max_index).all(|index| arms[index].guard.is_none()) &&
                SpanlessEq::new(cx).eq_expr(&lhs.body, &rhs.body) &&
                // all patterns should have the same bindings
                same_bindings(cx, &bindings(cx, &lhs.pat), &bindings(cx, &rhs.pat))
        };

        let indexed_arms: Vec<(usize, &Arm)> = arms.iter().enumerate().collect();
        for (&(_, i), &(_, j)) in search_same(&indexed_arms, hash, eq) {
            span_lint_and_then(
                cx,
                MATCH_SAME_ARMS,
                j.body.span,
                "this `match` has identical arm bodies",
                |db| {
                    db.span_note(i.body.span, "same as this");

                    // Note: this does not use `span_suggestion` on purpose:
                    // there is no clean way
                    // to remove the other arm. Building a span and suggest to replace it to ""
                    // makes an even more confusing error message. Also in order not to make up a
                    // span for the whole pattern, the suggestion is only shown when there is only
                    // one pattern. The user should know about `|` if they are already using it…

                    let lhs = snippet(cx, i.pat.span, "<pat1>");
                    let rhs = snippet(cx, j.pat.span, "<pat2>");

                    if let PatKind::Wild = j.pat.kind {
                        // if the last arm is _, then i could be integrated into _
                        // note that i.pat cannot be _, because that would mean that we're
                        // hiding all the subsequent arms, and rust won't compile
                        db.span_note(
                            i.body.span,
                            &format!(
                                "`{}` has the same arm body as the `_` wildcard, consider removing it`",
                                lhs
                            ),
                        );
                    } else {
                        db.span_help(i.pat.span, &format!("consider refactoring into `{} | {}`", lhs, rhs));
                    }
                },
            );
        }
    }
}

/// Returns the list of bindings in a pattern.
fn bindings<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, pat: &Pat) -> FxHashMap<Symbol, Ty<'tcx>> {
    fn bindings_impl<'a, 'tcx>(cx: &LateContext<'a, 'tcx>, pat: &Pat, map: &mut FxHashMap<Symbol, Ty<'tcx>>) {
        match pat.kind {
            PatKind::Box(ref pat) | PatKind::Ref(ref pat, _) => bindings_impl(cx, pat, map),
            PatKind::TupleStruct(_, ref pats, _) => {
                for pat in pats {
                    bindings_impl(cx, pat, map);
                }
            },
            PatKind::Binding(.., ident, ref as_pat) => {
                if let Entry::Vacant(v) = map.entry(ident.name) {
                    v.insert(cx.tables.pat_ty(pat));
                }
                if let Some(ref as_pat) = *as_pat {
                    bindings_impl(cx, as_pat, map);
                }
            },
            PatKind::Or(ref fields) | PatKind::Tuple(ref fields, _) => {
                for pat in fields {
                    bindings_impl(cx, pat, map);
                }
            },
            PatKind::Struct(_, ref fields, _) => {
                for pat in fields {
                    bindings_impl(cx, &pat.pat, map);
                }
            },
            PatKind::Slice(ref lhs, ref mid, ref rhs) => {
                for pat in lhs {
                    bindings_impl(cx, pat, map);
                }
                if let Some(ref mid) = *mid {
                    bindings_impl(cx, mid, map);
                }
                for pat in rhs {
                    bindings_impl(cx, pat, map);
                }
            },
            PatKind::Lit(..) | PatKind::Range(..) | PatKind::Wild | PatKind::Path(..) => (),
        }
    }

    let mut result = FxHashMap::default();
    bindings_impl(cx, pat, &mut result);
    result
}

fn search_same_sequenced<T, Eq>(exprs: &[T], eq: Eq) -> Option<(&T, &T)>
where
    Eq: Fn(&T, &T) -> bool,
{
    for win in exprs.windows(2) {
        if eq(&win[0], &win[1]) {
            return Some((&win[0], &win[1]));
        }
    }
    None
}

fn search_common_cases<'a, T, Eq>(exprs: &'a [T], eq: &Eq) -> Option<(&'a T, &'a T)>
where
    Eq: Fn(&T, &T) -> bool,
{
    if exprs.len() == 2 && eq(&exprs[0], &exprs[1]) {
        Some((&exprs[0], &exprs[1]))
    } else {
        None
    }
}

fn search_same<T, Hash, Eq>(exprs: &[T], hash: Hash, eq: Eq) -> Vec<(&T, &T)>
where
    Hash: Fn(&T) -> u64,
    Eq: Fn(&T, &T) -> bool,
{
    if let Some(expr) = search_common_cases(&exprs, &eq) {
        return vec![expr];
    }

    let mut match_expr_list: Vec<(&T, &T)> = Vec::new();

    let mut map: FxHashMap<_, Vec<&_>> =
        FxHashMap::with_capacity_and_hasher(exprs.len(), BuildHasherDefault::default());

    for expr in exprs {
        match map.entry(hash(expr)) {
            Entry::Occupied(mut o) => {
                for o in o.get() {
                    if eq(o, expr) {
                        match_expr_list.push((o, expr));
                    }
                }
                o.get_mut().push(expr);
            },
            Entry::Vacant(v) => {
                v.insert(vec![expr]);
            },
        }
    }

    match_expr_list
}
