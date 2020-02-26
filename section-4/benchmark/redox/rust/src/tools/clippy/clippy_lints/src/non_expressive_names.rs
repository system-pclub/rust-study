use crate::utils::{span_lint, span_lint_and_then};
use rustc::lint::{EarlyContext, EarlyLintPass, LintArray, LintPass};
use rustc::{declare_tool_lint, impl_lint_pass};
use std::cmp::Ordering;
use syntax::ast::*;
use syntax::attr;
use syntax::source_map::Span;
use syntax::symbol::SymbolStr;
use syntax::visit::{walk_block, walk_expr, walk_pat, Visitor};

declare_clippy_lint! {
    /// **What it does:** Checks for names that are very similar and thus confusing.
    ///
    /// **Why is this bad?** It's hard to distinguish between names that differ only
    /// by a single character.
    ///
    /// **Known problems:** None?
    ///
    /// **Example:**
    /// ```ignore
    /// let checked_exp = something;
    /// let checked_expr = something_else;
    /// ```
    pub SIMILAR_NAMES,
    pedantic,
    "similarly named items and bindings"
}

declare_clippy_lint! {
    /// **What it does:** Checks for too many variables whose name consists of a
    /// single character.
    ///
    /// **Why is this bad?** It's hard to memorize what a variable means without a
    /// descriptive name.
    ///
    /// **Known problems:** None?
    ///
    /// **Example:**
    /// ```ignore
    /// let (a, b, c, d, e, f, g) = (...);
    /// ```
    pub MANY_SINGLE_CHAR_NAMES,
    style,
    "too many single character bindings"
}

declare_clippy_lint! {
    /// **What it does:** Checks if you have variables whose name consists of just
    /// underscores and digits.
    ///
    /// **Why is this bad?** It's hard to memorize what a variable means without a
    /// descriptive name.
    ///
    /// **Known problems:** None?
    ///
    /// **Example:**
    /// ```rust
    /// let _1 = 1;
    /// let ___1 = 1;
    /// let __1___2 = 11;
    /// ```
    pub JUST_UNDERSCORES_AND_DIGITS,
    style,
    "unclear name"
}

#[derive(Copy, Clone)]
pub struct NonExpressiveNames {
    pub single_char_binding_names_threshold: u64,
}

impl_lint_pass!(NonExpressiveNames => [SIMILAR_NAMES, MANY_SINGLE_CHAR_NAMES, JUST_UNDERSCORES_AND_DIGITS]);

struct ExistingName {
    interned: SymbolStr,
    span: Span,
    len: usize,
    whitelist: &'static [&'static str],
}

struct SimilarNamesLocalVisitor<'a, 'tcx> {
    names: Vec<ExistingName>,
    cx: &'a EarlyContext<'tcx>,
    lint: &'a NonExpressiveNames,

    /// A stack of scopes containing the single-character bindings in each scope.
    single_char_names: Vec<Vec<Ident>>,
}

impl<'a, 'tcx> SimilarNamesLocalVisitor<'a, 'tcx> {
    fn check_single_char_names(&self) {
        let num_single_char_names = self.single_char_names.iter().flatten().count();
        let threshold = self.lint.single_char_binding_names_threshold;
        if num_single_char_names as u64 >= threshold {
            let span = self
                .single_char_names
                .iter()
                .flatten()
                .map(|ident| ident.span)
                .collect::<Vec<_>>();
            span_lint(
                self.cx,
                MANY_SINGLE_CHAR_NAMES,
                span,
                &format!(
                    "{} bindings with single-character names in scope",
                    num_single_char_names
                ),
            );
        }
    }
}

// this list contains lists of names that are allowed to be similar
// the assumption is that no name is ever contained in multiple lists.
#[rustfmt::skip]
const WHITELIST: &[&[&str]] = &[
    &["parsed", "parser"],
    &["lhs", "rhs"],
    &["tx", "rx"],
    &["set", "get"],
    &["args", "arms"],
    &["qpath", "path"],
    &["lit", "lint"],
];

struct SimilarNamesNameVisitor<'a, 'tcx, 'b>(&'b mut SimilarNamesLocalVisitor<'a, 'tcx>);

impl<'a, 'tcx, 'b> Visitor<'tcx> for SimilarNamesNameVisitor<'a, 'tcx, 'b> {
    fn visit_pat(&mut self, pat: &'tcx Pat) {
        match pat.kind {
            PatKind::Ident(_, ident, _) => self.check_ident(ident),
            PatKind::Struct(_, ref fields, _) => {
                for field in fields {
                    if !field.is_shorthand {
                        self.visit_pat(&field.pat);
                    }
                }
            },
            // just go through the first pattern, as either all patterns
            // bind the same bindings or rustc would have errored much earlier
            PatKind::Or(ref pats) => self.visit_pat(&pats[0]),
            _ => walk_pat(self, pat),
        }
    }
    fn visit_mac(&mut self, _mac: &Mac) {
        // do not check macs
    }
}

#[must_use]
fn get_whitelist(interned_name: &str) -> Option<&'static [&'static str]> {
    for &allow in WHITELIST {
        if whitelisted(interned_name, allow) {
            return Some(allow);
        }
    }
    None
}

#[must_use]
fn whitelisted(interned_name: &str, list: &[&str]) -> bool {
    list.iter()
        .any(|&name| interned_name.starts_with(name) || interned_name.ends_with(name))
}

impl<'a, 'tcx, 'b> SimilarNamesNameVisitor<'a, 'tcx, 'b> {
    fn check_short_ident(&mut self, ident: Ident) {
        // Ignore shadowing
        if self
            .0
            .single_char_names
            .iter()
            .flatten()
            .any(|id| id.name == ident.name)
        {
            return;
        }

        if let Some(scope) = &mut self.0.single_char_names.last_mut() {
            scope.push(ident);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn check_ident(&mut self, ident: Ident) {
        let interned_name = ident.name.as_str();
        if interned_name.chars().any(char::is_uppercase) {
            return;
        }
        if interned_name.chars().all(|c| c.is_digit(10) || c == '_') {
            span_lint(
                self.0.cx,
                JUST_UNDERSCORES_AND_DIGITS,
                ident.span,
                "consider choosing a more descriptive name",
            );
            return;
        }
        let count = interned_name.chars().count();
        if count < 3 {
            if count == 1 {
                self.check_short_ident(ident);
            }
            return;
        }
        for existing_name in &self.0.names {
            if whitelisted(&interned_name, existing_name.whitelist) {
                continue;
            }
            let mut split_at = None;
            match existing_name.len.cmp(&count) {
                Ordering::Greater => {
                    if existing_name.len - count != 1 || levenstein_not_1(&interned_name, &existing_name.interned) {
                        continue;
                    }
                },
                Ordering::Less => {
                    if count - existing_name.len != 1 || levenstein_not_1(&existing_name.interned, &interned_name) {
                        continue;
                    }
                },
                Ordering::Equal => {
                    let mut interned_chars = interned_name.chars();
                    let mut existing_chars = existing_name.interned.chars();
                    let first_i = interned_chars.next().expect("we know we have at least one char");
                    let first_e = existing_chars.next().expect("we know we have at least one char");
                    let eq_or_numeric = |(a, b): (char, char)| a == b || a.is_numeric() && b.is_numeric();

                    if eq_or_numeric((first_i, first_e)) {
                        let last_i = interned_chars.next_back().expect("we know we have at least two chars");
                        let last_e = existing_chars.next_back().expect("we know we have at least two chars");
                        if eq_or_numeric((last_i, last_e)) {
                            if interned_chars
                                .zip(existing_chars)
                                .filter(|&ie| !eq_or_numeric(ie))
                                .count()
                                != 1
                            {
                                continue;
                            }
                        } else {
                            let second_last_i = interned_chars
                                .next_back()
                                .expect("we know we have at least three chars");
                            let second_last_e = existing_chars
                                .next_back()
                                .expect("we know we have at least three chars");
                            if !eq_or_numeric((second_last_i, second_last_e))
                                || second_last_i == '_'
                                || !interned_chars.zip(existing_chars).all(eq_or_numeric)
                            {
                                // allowed similarity foo_x, foo_y
                                // or too many chars differ (foo_x, boo_y) or (foox, booy)
                                continue;
                            }
                            split_at = interned_name.char_indices().rev().next().map(|(i, _)| i);
                        }
                    } else {
                        let second_i = interned_chars.next().expect("we know we have at least two chars");
                        let second_e = existing_chars.next().expect("we know we have at least two chars");
                        if !eq_or_numeric((second_i, second_e))
                            || second_i == '_'
                            || !interned_chars.zip(existing_chars).all(eq_or_numeric)
                        {
                            // allowed similarity x_foo, y_foo
                            // or too many chars differ (x_foo, y_boo) or (xfoo, yboo)
                            continue;
                        }
                        split_at = interned_name.chars().next().map(char::len_utf8);
                    }
                },
            }
            span_lint_and_then(
                self.0.cx,
                SIMILAR_NAMES,
                ident.span,
                "binding's name is too similar to existing binding",
                |diag| {
                    diag.span_note(existing_name.span, "existing binding defined here");
                    if let Some(split) = split_at {
                        diag.span_help(
                            ident.span,
                            &format!(
                                "separate the discriminating character by an \
                                 underscore like: `{}_{}`",
                                &interned_name[..split],
                                &interned_name[split..]
                            ),
                        );
                    }
                },
            );
            return;
        }
        self.0.names.push(ExistingName {
            whitelist: get_whitelist(&interned_name).unwrap_or(&[]),
            interned: interned_name,
            span: ident.span,
            len: count,
        });
    }
}

impl<'a, 'b> SimilarNamesLocalVisitor<'a, 'b> {
    /// ensure scoping rules work
    fn apply<F: for<'c> Fn(&'c mut Self)>(&mut self, f: F) {
        let n = self.names.len();
        let single_char_count = self.single_char_names.len();
        f(self);
        self.names.truncate(n);
        self.single_char_names.truncate(single_char_count);
    }
}

impl<'a, 'tcx> Visitor<'tcx> for SimilarNamesLocalVisitor<'a, 'tcx> {
    fn visit_local(&mut self, local: &'tcx Local) {
        if let Some(ref init) = local.init {
            self.apply(|this| walk_expr(this, &**init));
        }
        // add the pattern after the expression because the bindings aren't available
        // yet in the init
        // expression
        SimilarNamesNameVisitor(self).visit_pat(&*local.pat);
    }
    fn visit_block(&mut self, blk: &'tcx Block) {
        self.single_char_names.push(vec![]);

        self.apply(|this| walk_block(this, blk));

        self.check_single_char_names();
        self.single_char_names.pop();
    }
    fn visit_arm(&mut self, arm: &'tcx Arm) {
        self.single_char_names.push(vec![]);

        self.apply(|this| {
            SimilarNamesNameVisitor(this).visit_pat(&arm.pat);
            this.apply(|this| walk_expr(this, &arm.body));
        });

        self.check_single_char_names();
        self.single_char_names.pop();
    }
    fn visit_item(&mut self, _: &Item) {
        // do not recurse into inner items
    }
    fn visit_mac(&mut self, _mac: &Mac) {
        // do not check macs
    }
}

impl EarlyLintPass for NonExpressiveNames {
    fn check_item(&mut self, cx: &EarlyContext<'_>, item: &Item) {
        if let ItemKind::Fn(ref sig, _, ref blk) = item.kind {
            do_check(self, cx, &item.attrs, &sig.decl, blk);
        }
    }

    fn check_impl_item(&mut self, cx: &EarlyContext<'_>, item: &ImplItem) {
        if let ImplItemKind::Method(ref sig, ref blk) = item.kind {
            do_check(self, cx, &item.attrs, &sig.decl, blk);
        }
    }
}

fn do_check(lint: &mut NonExpressiveNames, cx: &EarlyContext<'_>, attrs: &[Attribute], decl: &FnDecl, blk: &Block) {
    if !attr::contains_name(attrs, sym!(test)) {
        let mut visitor = SimilarNamesLocalVisitor {
            names: Vec::new(),
            cx,
            lint,
            single_char_names: vec![vec![]],
        };

        // initialize with function arguments
        for arg in &decl.inputs {
            SimilarNamesNameVisitor(&mut visitor).visit_pat(&arg.pat);
        }
        // walk all other bindings
        walk_block(&mut visitor, blk);

        visitor.check_single_char_names();
    }
}

/// Precondition: `a_name.chars().count() < b_name.chars().count()`.
#[must_use]
fn levenstein_not_1(a_name: &str, b_name: &str) -> bool {
    debug_assert!(a_name.chars().count() < b_name.chars().count());
    let mut a_chars = a_name.chars();
    let mut b_chars = b_name.chars();
    while let (Some(a), Some(b)) = (a_chars.next(), b_chars.next()) {
        if a == b {
            continue;
        }
        if let Some(b2) = b_chars.next() {
            // check if there's just one character inserted
            return a != b2 || a_chars.ne(b_chars);
        } else {
            // tuple
            // ntuple
            return true;
        }
    }
    // for item in items
    true
}
