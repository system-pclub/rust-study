use crate::utils::{is_allowed, snippet, span_lint_and_sugg};
use rustc::hir::*;
use rustc::lint::{LateContext, LateLintPass, LintArray, LintPass};
use rustc::{declare_lint_pass, declare_tool_lint};
use rustc_errors::Applicability;
use syntax::ast::LitKind;
use syntax::source_map::Span;
use unicode_normalization::UnicodeNormalization;

declare_clippy_lint! {
    /// **What it does:** Checks for the Unicode zero-width space in the code.
    ///
    /// **Why is this bad?** Having an invisible character in the code makes for all
    /// sorts of April fools, but otherwise is very much frowned upon.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:** You don't see it, but there may be a zero-width space
    /// somewhere in this text.
    pub ZERO_WIDTH_SPACE,
    correctness,
    "using a zero-width space in a string literal, which is confusing"
}

declare_clippy_lint! {
    /// **What it does:** Checks for non-ASCII characters in string literals.
    ///
    /// **Why is this bad?** Yeah, we know, the 90's called and wanted their charset
    /// back. Even so, there still are editors and other programs out there that
    /// don't work well with Unicode. So if the code is meant to be used
    /// internationally, on multiple operating systems, or has other portability
    /// requirements, activating this lint could be useful.
    ///
    /// **Known problems:** None.
    ///
    /// **Example:**
    /// ```rust
    /// let x = String::from("€");
    /// ```
    /// Could be written as:
    /// ```rust
    /// let x = String::from("\u{20ac}");
    /// ```
    pub NON_ASCII_LITERAL,
    pedantic,
    "using any literal non-ASCII chars in a string literal instead of using the `\\u` escape"
}

declare_clippy_lint! {
    /// **What it does:** Checks for string literals that contain Unicode in a form
    /// that is not equal to its
    /// [NFC-recomposition](http://www.unicode.org/reports/tr15/#Norm_Forms).
    ///
    /// **Why is this bad?** If such a string is compared to another, the results
    /// may be surprising.
    ///
    /// **Known problems** None.
    ///
    /// **Example:** You may not see it, but "à"" and "à"" aren't the same string. The
    /// former when escaped is actually `"a\u{300}"` while the latter is `"\u{e0}"`.
    pub UNICODE_NOT_NFC,
    pedantic,
    "using a Unicode literal not in NFC normal form (see [Unicode tr15](http://www.unicode.org/reports/tr15/) for further information)"
}

declare_lint_pass!(Unicode => [ZERO_WIDTH_SPACE, NON_ASCII_LITERAL, UNICODE_NOT_NFC]);

impl LateLintPass<'_, '_> for Unicode {
    fn check_expr(&mut self, cx: &LateContext<'_, '_>, expr: &'_ Expr) {
        if let ExprKind::Lit(ref lit) = expr.kind {
            if let LitKind::Str(_, _) = lit.node {
                check_str(cx, lit.span, expr.hir_id)
            }
        }
    }
}

fn escape<T: Iterator<Item = char>>(s: T) -> String {
    let mut result = String::new();
    for c in s {
        if c as u32 > 0x7F {
            for d in c.escape_unicode() {
                result.push(d)
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn check_str(cx: &LateContext<'_, '_>, span: Span, id: HirId) {
    let string = snippet(cx, span, "");
    if string.contains('\u{200B}') {
        span_lint_and_sugg(
            cx,
            ZERO_WIDTH_SPACE,
            span,
            "zero-width space detected",
            "consider replacing the string with",
            string.replace("\u{200B}", "\\u{200B}"),
            Applicability::MachineApplicable,
        );
    }
    if string.chars().any(|c| c as u32 > 0x7F) {
        span_lint_and_sugg(
            cx,
            NON_ASCII_LITERAL,
            span,
            "literal non-ASCII character detected",
            "consider replacing the string with",
            if is_allowed(cx, UNICODE_NOT_NFC, id) {
                escape(string.chars())
            } else {
                escape(string.nfc())
            },
            Applicability::MachineApplicable,
        );
    }
    if is_allowed(cx, NON_ASCII_LITERAL, id) && string.chars().zip(string.nfc()).any(|(a, b)| a != b) {
        span_lint_and_sugg(
            cx,
            UNICODE_NOT_NFC,
            span,
            "non-NFC Unicode sequence detected",
            "consider replacing the string with",
            string.nfc().collect::<String>(),
            Applicability::MachineApplicable,
        );
    }
}
