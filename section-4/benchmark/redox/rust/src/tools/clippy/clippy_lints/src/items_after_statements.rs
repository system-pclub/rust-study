//! lint when items are used after statements

use crate::utils::span_lint;
use matches::matches;
use rustc::lint::{EarlyContext, EarlyLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};
use syntax::ast::*;

declare_clippy_lint! {
    /// **What it does:** Checks for items declared after some statement in a block.
    ///
    /// **Why is this bad?** Items live for the entire scope they are declared
    /// in. But statements are processed in order. This might cause confusion as
    /// it's hard to figure out which item is meant in a statement.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// fn foo() {
    ///     println!("cake");
    /// }
    ///
    /// fn main() {
    ///     foo(); // prints "foo"
    ///     fn foo() {
    ///         println!("foo");
    ///     }
    ///     foo(); // prints "foo"
    /// }
    /// ```
    pub ITEMS_AFTER_STATEMENTS,
    pedantic,
    "blocks where an item comes after a statement"
}

declare_lint_pass!(ItemsAfterStatements => [ITEMS_AFTER_STATEMENTS]);

impl EarlyLintPass for ItemsAfterStatements {
    fn check_block(&mut self, cx: &EarlyContext<'_>, item: &Block) {
        if item.span.from_expansion() {
            return;
        }

        // skip initial items
        let stmts = item
            .stmts
            .iter()
            .map(|stmt| &stmt.kind)
            .skip_while(|s| matches!(**s, StmtKind::Item(..)));

        // lint on all further items
        for stmt in stmts {
            if let StmtKind::Item(ref it) = *stmt {
                if it.span.from_expansion() {
                    return;
                }
                if let ItemKind::MacroDef(..) = it.kind {
                    // do not lint `macro_rules`, but continue processing further statements
                    continue;
                }
                span_lint(
                    cx,
                    ITEMS_AFTER_STATEMENTS,
                    it.span,
                    "adding items after statements is confusing, since items exist from the \
                     start of the scope",
                );
            }
        }
    }
}
