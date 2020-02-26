use super::{BlockMode, PathStyle, SemiColonMode, TokenType, TokenExpectType, SeqSep, Parser};

use syntax::ast::{
    self, Param, BinOpKind, BindingMode, BlockCheckMode, Expr, ExprKind, Ident, Item, ItemKind,
    Mutability, Pat, PatKind, PathSegment, QSelf, Ty, TyKind,
};
use syntax::token::{self, TokenKind, token_can_begin_expr};
use syntax::print::pprust;
use syntax::ptr::P;
use syntax::symbol::{kw, sym};
use syntax::ThinVec;
use syntax::util::parser::AssocOp;
use syntax::struct_span_err;

use errors::{PResult, Applicability, DiagnosticBuilder, pluralize};
use rustc_data_structures::fx::FxHashSet;
use syntax_pos::{Span, DUMMY_SP, MultiSpan, SpanSnippetError};
use log::{debug, trace};
use std::mem;

use rustc_error_codes::*;

const TURBOFISH: &'static str = "use `::<...>` instead of `<...>` to specify type arguments";

/// Creates a placeholder argument.
pub(super) fn dummy_arg(ident: Ident) -> Param {
    let pat = P(Pat {
        id: ast::DUMMY_NODE_ID,
        kind: PatKind::Ident(BindingMode::ByValue(Mutability::Immutable), ident, None),
        span: ident.span,
    });
    let ty = Ty {
        kind: TyKind::Err,
        span: ident.span,
        id: ast::DUMMY_NODE_ID
    };
    Param {
        attrs: ThinVec::default(),
        id: ast::DUMMY_NODE_ID,
        pat,
        span: ident.span,
        ty: P(ty),
        is_placeholder: false,
    }
}

pub enum Error {
    FileNotFoundForModule {
        mod_name: String,
        default_path: String,
        secondary_path: String,
        dir_path: String,
    },
    DuplicatePaths {
        mod_name: String,
        default_path: String,
        secondary_path: String,
    },
    UselessDocComment,
    InclusiveRangeWithNoEnd,
}

impl Error {
    fn span_err<S: Into<MultiSpan>>(
        self,
        sp: S,
        handler: &errors::Handler,
    ) -> DiagnosticBuilder<'_> {
        match self {
            Error::FileNotFoundForModule {
                ref mod_name,
                ref default_path,
                ref secondary_path,
                ref dir_path,
            } => {
                let mut err = struct_span_err!(
                    handler,
                    sp,
                    E0583,
                    "file not found for module `{}`",
                    mod_name,
                );
                err.help(&format!(
                    "name the file either {} or {} inside the directory \"{}\"",
                    default_path,
                    secondary_path,
                    dir_path,
                ));
                err
            }
            Error::DuplicatePaths { ref mod_name, ref default_path, ref secondary_path } => {
                let mut err = struct_span_err!(
                    handler,
                    sp,
                    E0584,
                    "file for module `{}` found at both {} and {}",
                    mod_name,
                    default_path,
                    secondary_path,
                );
                err.help("delete or rename one of them to remove the ambiguity");
                err
            }
            Error::UselessDocComment => {
                let mut err = struct_span_err!(
                    handler,
                    sp,
                    E0585,
                    "found a documentation comment that doesn't document anything",
                );
                err.help("doc comments must come before what they document, maybe a comment was \
                          intended with `//`?");
                err
            }
            Error::InclusiveRangeWithNoEnd => {
                let mut err = struct_span_err!(
                    handler,
                    sp,
                    E0586,
                    "inclusive range with no end",
                );
                err.help("inclusive ranges must be bounded at the end (`..=b` or `a..=b`)");
                err
            }
        }
    }
}

pub(super) trait RecoverQPath: Sized + 'static {
    const PATH_STYLE: PathStyle = PathStyle::Expr;
    fn to_ty(&self) -> Option<P<Ty>>;
    fn recovered(qself: Option<QSelf>, path: ast::Path) -> Self;
}

impl RecoverQPath for Ty {
    const PATH_STYLE: PathStyle = PathStyle::Type;
    fn to_ty(&self) -> Option<P<Ty>> {
        Some(P(self.clone()))
    }
    fn recovered(qself: Option<QSelf>, path: ast::Path) -> Self {
        Self {
            span: path.span,
            kind: TyKind::Path(qself, path),
            id: ast::DUMMY_NODE_ID,
        }
    }
}

impl RecoverQPath for Pat {
    fn to_ty(&self) -> Option<P<Ty>> {
        self.to_ty()
    }
    fn recovered(qself: Option<QSelf>, path: ast::Path) -> Self {
        Self {
            span: path.span,
            kind: PatKind::Path(qself, path),
            id: ast::DUMMY_NODE_ID,
        }
    }
}

impl RecoverQPath for Expr {
    fn to_ty(&self) -> Option<P<Ty>> {
        self.to_ty()
    }
    fn recovered(qself: Option<QSelf>, path: ast::Path) -> Self {
        Self {
            span: path.span,
            kind: ExprKind::Path(qself, path),
            attrs: ThinVec::new(),
            id: ast::DUMMY_NODE_ID,
        }
    }
}

/// Control whether the closing delimiter should be consumed when calling `Parser::consume_block`.
crate enum ConsumeClosingDelim {
    Yes,
    No,
}

impl<'a> Parser<'a> {
    pub fn fatal(&self, m: &str) -> DiagnosticBuilder<'a> {
        self.span_fatal(self.token.span, m)
    }

    crate fn span_fatal<S: Into<MultiSpan>>(&self, sp: S, m: &str) -> DiagnosticBuilder<'a> {
        self.sess.span_diagnostic.struct_span_fatal(sp, m)
    }

    pub(super) fn span_fatal_err<S: Into<MultiSpan>>(
        &self,
        sp: S,
        err: Error,
    ) -> DiagnosticBuilder<'a> {
        err.span_err(sp, self.diagnostic())
    }

    pub(super) fn bug(&self, m: &str) -> ! {
        self.sess.span_diagnostic.span_bug(self.token.span, m)
    }

    pub(super) fn span_err<S: Into<MultiSpan>>(&self, sp: S, m: &str) {
        self.sess.span_diagnostic.span_err(sp, m)
    }

    pub fn struct_span_err<S: Into<MultiSpan>>(&self, sp: S, m: &str) -> DiagnosticBuilder<'a> {
        self.sess.span_diagnostic.struct_span_err(sp, m)
    }

    pub fn span_bug<S: Into<MultiSpan>>(&self, sp: S, m: &str) -> ! {
        self.sess.span_diagnostic.span_bug(sp, m)
    }

    pub(super) fn diagnostic(&self) -> &'a errors::Handler {
        &self.sess.span_diagnostic
    }

    pub(super) fn span_to_snippet(&self, span: Span) -> Result<String, SpanSnippetError> {
        self.sess.source_map().span_to_snippet(span)
    }

    pub(super) fn expected_ident_found(&self) -> DiagnosticBuilder<'a> {
        let mut err = self.struct_span_err(
            self.token.span,
            &format!("expected identifier, found {}", self.this_token_descr()),
        );
        let valid_follow = &[
            TokenKind::Eq,
            TokenKind::Colon,
            TokenKind::Comma,
            TokenKind::Semi,
            TokenKind::ModSep,
            TokenKind::OpenDelim(token::DelimToken::Brace),
            TokenKind::OpenDelim(token::DelimToken::Paren),
            TokenKind::CloseDelim(token::DelimToken::Brace),
            TokenKind::CloseDelim(token::DelimToken::Paren),
        ];
        if let token::Ident(name, false) = self.token.kind {
            if Ident::new(name, self.token.span).is_raw_guess() &&
                self.look_ahead(1, |t| valid_follow.contains(&t.kind))
            {
                err.span_suggestion(
                    self.token.span,
                    "you can escape reserved keywords to use them as identifiers",
                    format!("r#{}", name),
                    Applicability::MaybeIncorrect,
                );
            }
        }
        if let Some(token_descr) = self.token_descr() {
            err.span_label(self.token.span, format!("expected identifier, found {}", token_descr));
        } else {
            err.span_label(self.token.span, "expected identifier");
            if self.token == token::Comma && self.look_ahead(1, |t| t.is_ident()) {
                err.span_suggestion(
                    self.token.span,
                    "remove this comma",
                    String::new(),
                    Applicability::MachineApplicable,
                );
            }
        }
        err
    }

    pub(super) fn expected_one_of_not_found(
        &mut self,
        edible: &[TokenKind],
        inedible: &[TokenKind],
    ) -> PResult<'a, bool /* recovered */> {
        fn tokens_to_string(tokens: &[TokenType]) -> String {
            let mut i = tokens.iter();
            // This might be a sign we need a connect method on `Iterator`.
            let b = i.next()
                     .map_or(String::new(), |t| t.to_string());
            i.enumerate().fold(b, |mut b, (i, a)| {
                if tokens.len() > 2 && i == tokens.len() - 2 {
                    b.push_str(", or ");
                } else if tokens.len() == 2 && i == tokens.len() - 2 {
                    b.push_str(" or ");
                } else {
                    b.push_str(", ");
                }
                b.push_str(&a.to_string());
                b
            })
        }

        let mut expected = edible.iter()
            .map(|x| TokenType::Token(x.clone()))
            .chain(inedible.iter().map(|x| TokenType::Token(x.clone())))
            .chain(self.expected_tokens.iter().cloned())
            .collect::<Vec<_>>();
        expected.sort_by_cached_key(|x| x.to_string());
        expected.dedup();
        let expect = tokens_to_string(&expected[..]);
        let actual = self.this_token_descr();
        let (msg_exp, (label_sp, label_exp)) = if expected.len() > 1 {
            let short_expect = if expected.len() > 6 {
                format!("{} possible tokens", expected.len())
            } else {
                expect.clone()
            };
            (format!("expected one of {}, found {}", expect, actual),
                (self.sess.source_map().next_point(self.prev_span),
                format!("expected one of {}", short_expect)))
        } else if expected.is_empty() {
            (format!("unexpected token: {}", actual),
                (self.prev_span, "unexpected token after this".to_string()))
        } else {
            (format!("expected {}, found {}", expect, actual),
                (self.sess.source_map().next_point(self.prev_span),
                format!("expected {}", expect)))
        };
        self.last_unexpected_token_span = Some(self.token.span);
        let mut err = self.fatal(&msg_exp);
        if self.token.is_ident_named(sym::and) {
            err.span_suggestion_short(
                self.token.span,
                "use `&&` instead of `and` for the boolean operator",
                "&&".to_string(),
                Applicability::MaybeIncorrect,
            );
        }
        if self.token.is_ident_named(sym::or) {
            err.span_suggestion_short(
                self.token.span,
                "use `||` instead of `or` for the boolean operator",
                "||".to_string(),
                Applicability::MaybeIncorrect,
            );
        }
        let sp = if self.token == token::Eof {
            // This is EOF; don't want to point at the following char, but rather the last token.
            self.prev_span
        } else {
            label_sp
        };
        match self.recover_closing_delimiter(&expected.iter().filter_map(|tt| match tt {
            TokenType::Token(t) => Some(t.clone()),
            _ => None,
        }).collect::<Vec<_>>(), err) {
            Err(e) => err = e,
            Ok(recovered) => {
                return Ok(recovered);
            }
        }

        let sm = self.sess.source_map();
        if self.prev_span == DUMMY_SP {
            // Account for macro context where the previous span might not be
            // available to avoid incorrect output (#54841).
            err.span_label(self.token.span, label_exp);
        } else if !sm.is_multiline(self.token.span.shrink_to_hi().until(sp.shrink_to_lo())) {
            // When the spans are in the same line, it means that the only content between
            // them is whitespace, point at the found token in that case:
            //
            // X |     () => { syntax error };
            //   |                    ^^^^^ expected one of 8 possible tokens here
            //
            // instead of having:
            //
            // X |     () => { syntax error };
            //   |                   -^^^^^ unexpected token
            //   |                   |
            //   |                   expected one of 8 possible tokens here
            err.span_label(self.token.span, label_exp);
        } else {
            err.span_label(sp, label_exp);
            err.span_label(self.token.span, "unexpected token");
        }
        self.maybe_annotate_with_ascription(&mut err, false);
        Err(err)
    }

    pub fn maybe_annotate_with_ascription(
        &mut self,
        err: &mut DiagnosticBuilder<'_>,
        maybe_expected_semicolon: bool,
    ) {
        if let Some((sp, likely_path)) = self.last_type_ascription.take() {
            let sm = self.sess.source_map();
            let next_pos = sm.lookup_char_pos(self.token.span.lo());
            let op_pos = sm.lookup_char_pos(sp.hi());

            let allow_unstable = self.sess.unstable_features.is_nightly_build();

            if likely_path {
                err.span_suggestion(
                    sp,
                    "maybe write a path separator here",
                    "::".to_string(),
                    if allow_unstable {
                        Applicability::MaybeIncorrect
                    } else {
                        Applicability::MachineApplicable
                    },
                );
            } else if op_pos.line != next_pos.line && maybe_expected_semicolon {
                err.span_suggestion(
                    sp,
                    "try using a semicolon",
                    ";".to_string(),
                    Applicability::MaybeIncorrect,
                );
            } else if allow_unstable {
                err.span_label(sp, "tried to parse a type due to this type ascription");
            } else {
                err.span_label(sp, "tried to parse a type due to this");
            }
            if allow_unstable {
                // Give extra information about type ascription only if it's a nightly compiler.
                err.note("`#![feature(type_ascription)]` lets you annotate an expression with a \
                          type: `<expr>: <type>`");
                err.note("for more information, see \
                          https://github.com/rust-lang/rust/issues/23416");
            }
        }
    }

    /// Eats and discards tokens until one of `kets` is encountered. Respects token trees,
    /// passes through any errors encountered. Used for error recovery.
    pub(super) fn eat_to_tokens(&mut self, kets: &[&TokenKind]) {
        if let Err(ref mut err) = self.parse_seq_to_before_tokens(
            kets,
            SeqSep::none(),
            TokenExpectType::Expect,
            |p| Ok(p.parse_token_tree()),
        ) {
            err.cancel();
        }
    }

    /// This function checks if there are trailing angle brackets and produces
    /// a diagnostic to suggest removing them.
    ///
    /// ```ignore (diagnostic)
    /// let _ = vec![1, 2, 3].into_iter().collect::<Vec<usize>>>>();
    ///                                                        ^^ help: remove extra angle brackets
    /// ```
    pub(super) fn check_trailing_angle_brackets(&mut self, segment: &PathSegment, end: TokenKind) {
        // This function is intended to be invoked after parsing a path segment where there are two
        // cases:
        //
        // 1. A specific token is expected after the path segment.
        //    eg. `x.foo(`, `x.foo::<u32>(` (parenthesis - method call),
        //        `Foo::`, or `Foo::<Bar>::` (mod sep - continued path).
        // 2. No specific token is expected after the path segment.
        //    eg. `x.foo` (field access)
        //
        // This function is called after parsing `.foo` and before parsing the token `end` (if
        // present). This includes any angle bracket arguments, such as `.foo::<u32>` or
        // `Foo::<Bar>`.

        // We only care about trailing angle brackets if we previously parsed angle bracket
        // arguments. This helps stop us incorrectly suggesting that extra angle brackets be
        // removed in this case:
        //
        // `x.foo >> (3)` (where `x.foo` is a `u32` for example)
        //
        // This case is particularly tricky as we won't notice it just looking at the tokens -
        // it will appear the same (in terms of upcoming tokens) as below (since the `::<u32>` will
        // have already been parsed):
        //
        // `x.foo::<u32>>>(3)`
        let parsed_angle_bracket_args = segment.args
            .as_ref()
            .map(|args| args.is_angle_bracketed())
            .unwrap_or(false);

        debug!(
            "check_trailing_angle_brackets: parsed_angle_bracket_args={:?}",
            parsed_angle_bracket_args,
        );
        if !parsed_angle_bracket_args {
            return;
        }

        // Keep the span at the start so we can highlight the sequence of `>` characters to be
        // removed.
        let lo = self.token.span;

        // We need to look-ahead to see if we have `>` characters without moving the cursor forward
        // (since we might have the field access case and the characters we're eating are
        // actual operators and not trailing characters - ie `x.foo >> 3`).
        let mut position = 0;

        // We can encounter `>` or `>>` tokens in any order, so we need to keep track of how
        // many of each (so we can correctly pluralize our error messages) and continue to
        // advance.
        let mut number_of_shr = 0;
        let mut number_of_gt = 0;
        while self.look_ahead(position, |t| {
            trace!("check_trailing_angle_brackets: t={:?}", t);
            if *t == token::BinOp(token::BinOpToken::Shr) {
                number_of_shr += 1;
                true
            } else if *t == token::Gt {
                number_of_gt += 1;
                true
            } else {
                false
            }
        }) {
            position += 1;
        }

        // If we didn't find any trailing `>` characters, then we have nothing to error about.
        debug!(
            "check_trailing_angle_brackets: number_of_gt={:?} number_of_shr={:?}",
            number_of_gt, number_of_shr,
        );
        if number_of_gt < 1 && number_of_shr < 1 {
            return;
        }

        // Finally, double check that we have our end token as otherwise this is the
        // second case.
        if self.look_ahead(position, |t| {
            trace!("check_trailing_angle_brackets: t={:?}", t);
            *t == end
        }) {
            // Eat from where we started until the end token so that parsing can continue
            // as if we didn't have those extra angle brackets.
            self.eat_to_tokens(&[&end]);
            let span = lo.until(self.token.span);

            let total_num_of_gt = number_of_gt + number_of_shr * 2;
            self.diagnostic()
                .struct_span_err(
                    span,
                    &format!("unmatched angle bracket{}", pluralize!(total_num_of_gt)),
                )
                .span_suggestion(
                    span,
                    &format!("remove extra angle bracket{}", pluralize!(total_num_of_gt)),
                    String::new(),
                    Applicability::MachineApplicable,
                )
                .emit();
        }
    }

    /// Produces an error if comparison operators are chained (RFC #558).
    /// We only need to check the LHS, not the RHS, because all comparison ops have same
    /// precedence (see `fn precedence`) and are left-associative (see `fn fixity`).
    ///
    /// This can also be hit if someone incorrectly writes `foo<bar>()` when they should have used
    /// the turbofish (`foo::<bar>()`) syntax. We attempt some heuristic recovery if that is the
    /// case.
    ///
    /// Keep in mind that given that `outer_op.is_comparison()` holds and comparison ops are left
    /// associative we can infer that we have:
    ///
    ///           outer_op
    ///           /   \
    ///     inner_op   r2
    ///        /  \
    ///     l1    r1
    pub(super) fn check_no_chained_comparison(
        &mut self,
        lhs: &Expr,
        outer_op: &AssocOp,
    ) -> PResult<'a, Option<P<Expr>>> {
        debug_assert!(
            outer_op.is_comparison(),
            "check_no_chained_comparison: {:?} is not comparison",
            outer_op,
        );

        let mk_err_expr = |this: &Self, span| {
            Ok(Some(this.mk_expr(span, ExprKind::Err, ThinVec::new())))
        };

        match lhs.kind {
            ExprKind::Binary(op, _, _) if op.node.is_comparison() => {
                // Respan to include both operators.
                let op_span = op.span.to(self.prev_span);
                let mut err = self.struct_span_err(
                    op_span,
                    "chained comparison operators require parentheses",
                );

                let suggest = |err: &mut DiagnosticBuilder<'_>| {
                    err.span_suggestion_verbose(
                        op_span.shrink_to_lo(),
                        TURBOFISH,
                        "::".to_string(),
                        Applicability::MaybeIncorrect,
                    );
                };

                if op.node == BinOpKind::Lt &&
                    *outer_op == AssocOp::Less ||  // Include `<` to provide this recommendation
                    *outer_op == AssocOp::Greater  // even in a case like the following:
                {                                  //     Foo<Bar<Baz<Qux, ()>>>
                    if *outer_op == AssocOp::Less {
                        let snapshot = self.clone();
                        self.bump();
                        // So far we have parsed `foo<bar<`, consume the rest of the type args.
                        let modifiers = [
                            (token::Lt, 1),
                            (token::Gt, -1),
                            (token::BinOp(token::Shr), -2),
                        ];
                        self.consume_tts(1, &modifiers[..]);

                        if !&[
                            token::OpenDelim(token::Paren),
                            token::ModSep,
                        ].contains(&self.token.kind) {
                            // We don't have `foo< bar >(` or `foo< bar >::`, so we rewind the
                            // parser and bail out.
                            mem::replace(self, snapshot.clone());
                        }
                    }
                    return if token::ModSep == self.token.kind {
                        // We have some certainty that this was a bad turbofish at this point.
                        // `foo< bar >::`
                        suggest(&mut err);

                        let snapshot = self.clone();
                        self.bump(); // `::`

                        // Consume the rest of the likely `foo<bar>::new()` or return at `foo<bar>`.
                        match self.parse_expr() {
                            Ok(_) => {
                                // 99% certain that the suggestion is correct, continue parsing.
                                err.emit();
                                // FIXME: actually check that the two expressions in the binop are
                                // paths and resynthesize new fn call expression instead of using
                                // `ExprKind::Err` placeholder.
                                mk_err_expr(self, lhs.span.to(self.prev_span))
                            }
                            Err(mut expr_err) => {
                                expr_err.cancel();
                                // Not entirely sure now, but we bubble the error up with the
                                // suggestion.
                                mem::replace(self, snapshot);
                                Err(err)
                            }
                        }
                    } else if token::OpenDelim(token::Paren) == self.token.kind {
                        // We have high certainty that this was a bad turbofish at this point.
                        // `foo< bar >(`
                        suggest(&mut err);
                        // Consume the fn call arguments.
                        match self.consume_fn_args() {
                            Err(()) => Err(err),
                            Ok(()) => {
                                err.emit();
                                // FIXME: actually check that the two expressions in the binop are
                                // paths and resynthesize new fn call expression instead of using
                                // `ExprKind::Err` placeholder.
                                mk_err_expr(self, lhs.span.to(self.prev_span))
                            }
                        }
                    } else {
                        // All we know is that this is `foo < bar >` and *nothing* else. Try to
                        // be helpful, but don't attempt to recover.
                        err.help(TURBOFISH);
                        err.help("or use `(...)` if you meant to specify fn arguments");
                        // These cases cause too many knock-down errors, bail out (#61329).
                        Err(err)
                    };
                }
                err.emit();
            }
            _ => {}
        }
        Ok(None)
    }

    fn consume_fn_args(&mut self) -> Result<(), ()> {
        let snapshot = self.clone();
        self.bump(); // `(`

        // Consume the fn call arguments.
        let modifiers = [
            (token::OpenDelim(token::Paren), 1),
            (token::CloseDelim(token::Paren), -1),
        ];
        self.consume_tts(1, &modifiers[..]);

        if self.token.kind == token::Eof {
            // Not entirely sure that what we consumed were fn arguments, rollback.
            mem::replace(self, snapshot);
            Err(())
        } else {
            // 99% certain that the suggestion is correct, continue parsing.
            Ok(())
        }
    }

    pub(super) fn maybe_report_ambiguous_plus(
        &mut self,
        allow_plus: bool,
        impl_dyn_multi: bool,
        ty: &Ty,
    ) {
        if !allow_plus && impl_dyn_multi {
            let sum_with_parens = format!("({})", pprust::ty_to_string(&ty));
            self.struct_span_err(ty.span, "ambiguous `+` in a type")
                .span_suggestion(
                    ty.span,
                    "use parentheses to disambiguate",
                    sum_with_parens,
                    Applicability::MachineApplicable,
                )
                .emit();
        }
    }

    pub(super) fn maybe_recover_from_bad_type_plus(
        &mut self,
        allow_plus: bool,
        ty: &Ty,
    ) -> PResult<'a, ()> {
        // Do not add `+` to expected tokens.
        if !allow_plus || !self.token.is_like_plus() {
            return Ok(());
        }

        self.bump(); // `+`
        let bounds = self.parse_generic_bounds(None)?;
        let sum_span = ty.span.to(self.prev_span);

        let mut err = struct_span_err!(
            self.sess.span_diagnostic,
            sum_span,
            E0178,
            "expected a path on the left-hand side of `+`, not `{}`",
            pprust::ty_to_string(ty)
        );

        match ty.kind {
            TyKind::Rptr(ref lifetime, ref mut_ty) => {
                let sum_with_parens = pprust::to_string(|s| {
                    s.s.word("&");
                    s.print_opt_lifetime(lifetime);
                    s.print_mutability(mut_ty.mutbl);
                    s.popen();
                    s.print_type(&mut_ty.ty);
                    s.print_type_bounds(" +", &bounds);
                    s.pclose()
                });
                err.span_suggestion(
                    sum_span,
                    "try adding parentheses",
                    sum_with_parens,
                    Applicability::MachineApplicable,
                );
            }
            TyKind::Ptr(..) | TyKind::BareFn(..) => {
                err.span_label(sum_span, "perhaps you forgot parentheses?");
            }
            _ => {
                err.span_label(sum_span, "expected a path");
            }
        }
        err.emit();
        Ok(())
    }

    /// Tries to recover from associated item paths like `[T]::AssocItem` / `(T, U)::AssocItem`.
    /// Attempts to convert the base expression/pattern/type into a type, parses the `::AssocItem`
    /// tail, and combines them into a `<Ty>::AssocItem` expression/pattern/type.
    pub(super) fn maybe_recover_from_bad_qpath<T: RecoverQPath>(
        &mut self,
        base: P<T>,
        allow_recovery: bool,
    ) -> PResult<'a, P<T>> {
        // Do not add `::` to expected tokens.
        if allow_recovery && self.token == token::ModSep {
            if let Some(ty) = base.to_ty() {
                return self.maybe_recover_from_bad_qpath_stage_2(ty.span, ty);
            }
        }
        Ok(base)
    }

    /// Given an already parsed `Ty`, parses the `::AssocItem` tail and
    /// combines them into a `<Ty>::AssocItem` expression/pattern/type.
    pub(super) fn maybe_recover_from_bad_qpath_stage_2<T: RecoverQPath>(
        &mut self,
        ty_span: Span,
        ty: P<Ty>,
    ) -> PResult<'a, P<T>> {
        self.expect(&token::ModSep)?;

        let mut path = ast::Path {
            segments: Vec::new(),
            span: DUMMY_SP,
        };
        self.parse_path_segments(&mut path.segments, T::PATH_STYLE)?;
        path.span = ty_span.to(self.prev_span);

        let ty_str = self
            .span_to_snippet(ty_span)
            .unwrap_or_else(|_| pprust::ty_to_string(&ty));
        self.diagnostic()
            .struct_span_err(path.span, "missing angle brackets in associated item path")
            .span_suggestion(
                // This is a best-effort recovery.
                path.span,
                "try",
                format!("<{}>::{}", ty_str, pprust::path_to_string(&path)),
                Applicability::MaybeIncorrect,
            )
            .emit();

        let path_span = ty_span.shrink_to_hi(); // Use an empty path since `position == 0`.
        Ok(P(T::recovered(
            Some(QSelf {
                ty,
                path_span,
                position: 0,
            }),
            path,
        )))
    }

    pub(super) fn maybe_consume_incorrect_semicolon(&mut self, items: &[P<Item>]) -> bool {
        if self.eat(&token::Semi) {
            let mut err = self.struct_span_err(self.prev_span, "expected item, found `;`");
            err.span_suggestion_short(
                self.prev_span,
                "remove this semicolon",
                String::new(),
                Applicability::MachineApplicable,
            );
            if !items.is_empty() {
                let previous_item = &items[items.len() - 1];
                let previous_item_kind_name = match previous_item.kind {
                    // Say "braced struct" because tuple-structs and
                    // braceless-empty-struct declarations do take a semicolon.
                    ItemKind::Struct(..) => Some("braced struct"),
                    ItemKind::Enum(..) => Some("enum"),
                    ItemKind::Trait(..) => Some("trait"),
                    ItemKind::Union(..) => Some("union"),
                    _ => None,
                };
                if let Some(name) = previous_item_kind_name {
                    err.help(&format!(
                        "{} declarations are not followed by a semicolon",
                        name
                    ));
                }
            }
            err.emit();
            true
        } else {
            false
        }
    }

    /// Creates a `DiagnosticBuilder` for an unexpected token `t` and tries to recover if it is a
    /// closing delimiter.
    pub(super) fn unexpected_try_recover(
        &mut self,
        t: &TokenKind,
    ) -> PResult<'a, bool /* recovered */> {
        let token_str = pprust::token_kind_to_string(t);
        let this_token_str = self.this_token_descr();
        let (prev_sp, sp) = match (&self.token.kind, self.subparser_name) {
            // Point at the end of the macro call when reaching end of macro arguments.
            (token::Eof, Some(_)) => {
                let sp = self.sess.source_map().next_point(self.token.span);
                (sp, sp)
            }
            // We don't want to point at the following span after DUMMY_SP.
            // This happens when the parser finds an empty TokenStream.
            _ if self.prev_span == DUMMY_SP => (self.token.span, self.token.span),
            // EOF, don't want to point at the following char, but rather the last token.
            (token::Eof, None) => (self.prev_span, self.token.span),
            _ => (self.sess.source_map().next_point(self.prev_span), self.token.span),
        };
        let msg = format!(
            "expected `{}`, found {}",
            token_str,
            match (&self.token.kind, self.subparser_name) {
                (token::Eof, Some(origin)) => format!("end of {}", origin),
                _ => this_token_str,
            },
        );
        let mut err = self.struct_span_err(sp, &msg);
        let label_exp = format!("expected `{}`", token_str);
        match self.recover_closing_delimiter(&[t.clone()], err) {
            Err(e) => err = e,
            Ok(recovered) => {
                return Ok(recovered);
            }
        }
        let sm = self.sess.source_map();
        if !sm.is_multiline(prev_sp.until(sp)) {
            // When the spans are in the same line, it means that the only content
            // between them is whitespace, point only at the found token.
            err.span_label(sp, label_exp);
        } else {
            err.span_label(prev_sp, label_exp);
            err.span_label(sp, "unexpected token");
        }
        Err(err)
    }

    pub(super) fn expect_semi(&mut self) -> PResult<'a, ()> {
        if self.eat(&token::Semi) {
            return Ok(());
        }
        let sm = self.sess.source_map();
        let msg = format!("expected `;`, found `{}`", self.this_token_descr());
        let appl = Applicability::MachineApplicable;
        if self.token.span == DUMMY_SP || self.prev_span == DUMMY_SP {
            // Likely inside a macro, can't provide meaninful suggestions.
            return self.expect(&token::Semi).map(|_| ());
        } else if !sm.is_multiline(self.prev_span.until(self.token.span)) {
            // The current token is in the same line as the prior token, not recoverable.
        } else if self.look_ahead(1, |t| t == &token::CloseDelim(token::Brace)
            || token_can_begin_expr(t) && t.kind != token::Colon
        ) && [token::Comma, token::Colon].contains(&self.token.kind) {
            // Likely typo: `,` → `;` or `:` → `;`. This is triggered if the current token is
            // either `,` or `:`, and the next token could either start a new statement or is a
            // block close. For example:
            //
            //   let x = 32:
            //   let y = 42;
            self.bump();
            let sp = self.prev_span;
            self.struct_span_err(sp, &msg)
                .span_suggestion(sp, "change this to `;`", ";".to_string(), appl)
                .emit();
            return Ok(())
        } else if self.look_ahead(0, |t| t == &token::CloseDelim(token::Brace) || (
                token_can_begin_expr(t)
                && t != &token::Semi
                && t != &token::Pound // Avoid triggering with too many trailing `#` in raw string.
        )) {
            // Missing semicolon typo. This is triggered if the next token could either start a
            // new statement or is a block close. For example:
            //
            //   let x = 32
            //   let y = 42;
            let sp = self.prev_span.shrink_to_hi();
            self.struct_span_err(sp, &msg)
                .span_label(self.token.span, "unexpected token")
                .span_suggestion_short(sp, "add `;` here", ";".to_string(), appl)
                .emit();
            return Ok(())
        }
        self.expect(&token::Semi).map(|_| ()) // Error unconditionally
    }

    pub(super) fn parse_semi_or_incorrect_foreign_fn_body(
        &mut self,
        ident: &Ident,
        extern_sp: Span,
    ) -> PResult<'a, ()> {
        if self.token != token::Semi {
            // This might be an incorrect fn definition (#62109).
            let parser_snapshot = self.clone();
            match self.parse_inner_attrs_and_block() {
                Ok((_, body)) => {
                    self.struct_span_err(ident.span, "incorrect `fn` inside `extern` block")
                        .span_label(ident.span, "can't have a body")
                        .span_label(body.span, "this body is invalid here")
                        .span_label(
                            extern_sp,
                            "`extern` blocks define existing foreign functions and `fn`s \
                             inside of them cannot have a body")
                        .help("you might have meant to write a function accessible through ffi, \
                               which can be done by writing `extern fn` outside of the \
                               `extern` block")
                        .note("for more information, visit \
                               https://doc.rust-lang.org/std/keyword.extern.html")
                        .emit();
                }
                Err(mut err) => {
                    err.cancel();
                    mem::replace(self, parser_snapshot);
                    self.expect_semi()?;
                }
            }
        } else {
            self.bump();
        }
        Ok(())
    }

    /// Consumes alternative await syntaxes like `await!(<expr>)`, `await <expr>`,
    /// `await? <expr>`, `await(<expr>)`, and `await { <expr> }`.
    pub(super) fn parse_incorrect_await_syntax(
        &mut self,
        lo: Span,
        await_sp: Span,
    ) -> PResult<'a, (Span, ExprKind)> {
        if self.token == token::Not {
            // Handle `await!(<expr>)`.
            self.expect(&token::Not)?;
            self.expect(&token::OpenDelim(token::Paren))?;
            let expr = self.parse_expr()?;
            self.expect(&token::CloseDelim(token::Paren))?;
            let sp = self.error_on_incorrect_await(lo, self.prev_span, &expr, false);
            return Ok((sp, ExprKind::Await(expr)))
        }

        let is_question = self.eat(&token::Question); // Handle `await? <expr>`.
        let expr = if self.token == token::OpenDelim(token::Brace) {
            // Handle `await { <expr> }`.
            // This needs to be handled separatedly from the next arm to avoid
            // interpreting `await { <expr> }?` as `<expr>?.await`.
            self.parse_block_expr(
                None,
                self.token.span,
                BlockCheckMode::Default,
                ThinVec::new(),
            )
        } else {
            self.parse_expr()
        }.map_err(|mut err| {
            err.span_label(await_sp, "while parsing this incorrect await expression");
            err
        })?;
        let sp = self.error_on_incorrect_await(lo, expr.span, &expr, is_question);
        Ok((sp, ExprKind::Await(expr)))
    }

    fn error_on_incorrect_await(&self, lo: Span, hi: Span, expr: &Expr, is_question: bool) -> Span {
        let expr_str = self.span_to_snippet(expr.span)
            .unwrap_or_else(|_| pprust::expr_to_string(&expr));
        let suggestion = format!("{}.await{}", expr_str, if is_question { "?" } else { "" });
        let sp = lo.to(hi);
        let app = match expr.kind {
            ExprKind::Try(_) => Applicability::MaybeIncorrect, // `await <expr>?`
            _ => Applicability::MachineApplicable,
        };
        self.struct_span_err(sp, "incorrect use of `await`")
            .span_suggestion(sp, "`await` is a postfix operation", suggestion, app)
            .emit();
        sp
    }

    /// If encountering `future.await()`, consumes and emits an error.
    pub(super) fn recover_from_await_method_call(&mut self) {
        if self.token == token::OpenDelim(token::Paren) &&
            self.look_ahead(1, |t| t == &token::CloseDelim(token::Paren))
        {
            // future.await()
            let lo = self.token.span;
            self.bump(); // (
            let sp = lo.to(self.token.span);
            self.bump(); // )
            self.struct_span_err(sp, "incorrect use of `await`")
                .span_suggestion(
                    sp,
                    "`await` is not a method call, remove the parentheses",
                    String::new(),
                    Applicability::MachineApplicable,
                ).emit()
        }
    }

    /// Recovers a situation like `for ( $pat in $expr )`
    /// and suggest writing `for $pat in $expr` instead.
    ///
    /// This should be called before parsing the `$block`.
    pub(super) fn recover_parens_around_for_head(
        &mut self,
        pat: P<Pat>,
        expr: &Expr,
        begin_paren: Option<Span>,
    ) -> P<Pat> {
        match (&self.token.kind, begin_paren) {
            (token::CloseDelim(token::Paren), Some(begin_par_sp)) => {
                self.bump();

                let pat_str = self
                    // Remove the `(` from the span of the pattern:
                    .span_to_snippet(pat.span.trim_start(begin_par_sp).unwrap())
                    .unwrap_or_else(|_| pprust::pat_to_string(&pat));

                self.struct_span_err(self.prev_span, "unexpected closing `)`")
                    .span_label(begin_par_sp, "opening `(`")
                    .span_suggestion(
                        begin_par_sp.to(self.prev_span),
                        "remove parenthesis in `for` loop",
                        format!("{} in {}", pat_str, pprust::expr_to_string(&expr)),
                        // With e.g. `for (x) in y)` this would replace `(x) in y)`
                        // with `x) in y)` which is syntactically invalid.
                        // However, this is prevented before we get here.
                        Applicability::MachineApplicable,
                    )
                    .emit();

                // Unwrap `(pat)` into `pat` to avoid the `unused_parens` lint.
                pat.and_then(|pat| match pat.kind {
                    PatKind::Paren(pat) => pat,
                    _ => P(pat),
                })
            }
            _ => pat,
        }
    }

    pub(super) fn could_ascription_be_path(&self, node: &ast::ExprKind) -> bool {
        (self.token == token::Lt && // `foo:<bar`, likely a typoed turbofish.
            self.look_ahead(1, |t| t.is_ident() && !t.is_reserved_ident())
        ) ||
            self.token.is_ident() &&
            match node {
                // `foo::` → `foo:` or `foo.bar::` → `foo.bar:`
                ast::ExprKind::Path(..) | ast::ExprKind::Field(..) => true,
                _ => false,
            } &&
            !self.token.is_reserved_ident() &&           // v `foo:bar(baz)`
            self.look_ahead(1, |t| t == &token::OpenDelim(token::Paren)) ||
            self.look_ahead(1, |t| t == &token::Lt) &&     // `foo:bar<baz`
            self.look_ahead(2, |t| t.is_ident()) ||
            self.look_ahead(1, |t| t == &token::Colon) &&  // `foo:bar:baz`
            self.look_ahead(2, |t| t.is_ident()) ||
            self.look_ahead(1, |t| t == &token::ModSep) &&
            (self.look_ahead(2, |t| t.is_ident()) ||   // `foo:bar::baz`
             self.look_ahead(2, |t| t == &token::Lt))  // `foo:bar::<baz>`
    }

    pub(super) fn recover_seq_parse_error(
        &mut self,
        delim: token::DelimToken,
        lo: Span,
        result: PResult<'a, P<Expr>>,
    ) -> P<Expr> {
        match result {
            Ok(x) => x,
            Err(mut err) => {
                err.emit();
                // Recover from parse error, callers expect the closing delim to be consumed.
                self.consume_block(delim, ConsumeClosingDelim::Yes);
                self.mk_expr(lo.to(self.prev_span), ExprKind::Err, ThinVec::new())
            }
        }
    }

    pub(super) fn recover_closing_delimiter(
        &mut self,
        tokens: &[TokenKind],
        mut err: DiagnosticBuilder<'a>,
    ) -> PResult<'a, bool> {
        let mut pos = None;
        // We want to use the last closing delim that would apply.
        for (i, unmatched) in self.unclosed_delims.iter().enumerate().rev() {
            if tokens.contains(&token::CloseDelim(unmatched.expected_delim))
                && Some(self.token.span) > unmatched.unclosed_span
            {
                pos = Some(i);
            }
        }
        match pos {
            Some(pos) => {
                // Recover and assume that the detected unclosed delimiter was meant for
                // this location. Emit the diagnostic and act as if the delimiter was
                // present for the parser's sake.

                 // Don't attempt to recover from this unclosed delimiter more than once.
                let unmatched = self.unclosed_delims.remove(pos);
                let delim = TokenType::Token(token::CloseDelim(unmatched.expected_delim));
                if unmatched.found_delim.is_none() {
                    // We encountered `Eof`, set this fact here to avoid complaining about missing
                    // `fn main()` when we found place to suggest the closing brace.
                    *self.sess.reached_eof.borrow_mut() = true;
                }

                // We want to suggest the inclusion of the closing delimiter where it makes
                // the most sense, which is immediately after the last token:
                //
                //  {foo(bar {}}
                //      -      ^
                //      |      |
                //      |      help: `)` may belong here
                //      |
                //      unclosed delimiter
                if let Some(sp) = unmatched.unclosed_span {
                    err.span_label(sp, "unclosed delimiter");
                }
                err.span_suggestion_short(
                    self.sess.source_map().next_point(self.prev_span),
                    &format!("{} may belong here", delim.to_string()),
                    delim.to_string(),
                    Applicability::MaybeIncorrect,
                );
                if unmatched.found_delim.is_none() {
                    // Encountered `Eof` when lexing blocks. Do not recover here to avoid knockdown
                    // errors which would be emitted elsewhere in the parser and let other error
                    // recovery consume the rest of the file.
                    Err(err)
                } else {
                    err.emit();
                    self.expected_tokens.clear();  // Reduce the number of errors.
                    Ok(true)
                }
            }
            _ => Err(err),
        }
    }

    /// Eats tokens until we can be relatively sure we reached the end of the
    /// statement. This is something of a best-effort heuristic.
    ///
    /// We terminate when we find an unmatched `}` (without consuming it).
    pub(super) fn recover_stmt(&mut self) {
        self.recover_stmt_(SemiColonMode::Ignore, BlockMode::Ignore)
    }

    /// If `break_on_semi` is `Break`, then we will stop consuming tokens after
    /// finding (and consuming) a `;` outside of `{}` or `[]` (note that this is
    /// approximate -- it can mean we break too early due to macros, but that
    /// should only lead to sub-optimal recovery, not inaccurate parsing).
    ///
    /// If `break_on_block` is `Break`, then we will stop consuming tokens
    /// after finding (and consuming) a brace-delimited block.
    pub(super) fn recover_stmt_(
        &mut self,
        break_on_semi: SemiColonMode,
        break_on_block: BlockMode,
    ) {
        let mut brace_depth = 0;
        let mut bracket_depth = 0;
        let mut in_block = false;
        debug!("recover_stmt_ enter loop (semi={:?}, block={:?})",
               break_on_semi, break_on_block);
        loop {
            debug!("recover_stmt_ loop {:?}", self.token);
            match self.token.kind {
                token::OpenDelim(token::DelimToken::Brace) => {
                    brace_depth += 1;
                    self.bump();
                    if break_on_block == BlockMode::Break &&
                       brace_depth == 1 &&
                       bracket_depth == 0 {
                        in_block = true;
                    }
                }
                token::OpenDelim(token::DelimToken::Bracket) => {
                    bracket_depth += 1;
                    self.bump();
                }
                token::CloseDelim(token::DelimToken::Brace) => {
                    if brace_depth == 0 {
                        debug!("recover_stmt_ return - close delim {:?}", self.token);
                        break;
                    }
                    brace_depth -= 1;
                    self.bump();
                    if in_block && bracket_depth == 0 && brace_depth == 0 {
                        debug!("recover_stmt_ return - block end {:?}", self.token);
                        break;
                    }
                }
                token::CloseDelim(token::DelimToken::Bracket) => {
                    bracket_depth -= 1;
                    if bracket_depth < 0 {
                        bracket_depth = 0;
                    }
                    self.bump();
                }
                token::Eof => {
                    debug!("recover_stmt_ return - Eof");
                    break;
                }
                token::Semi => {
                    self.bump();
                    if break_on_semi == SemiColonMode::Break &&
                       brace_depth == 0 &&
                       bracket_depth == 0 {
                        debug!("recover_stmt_ return - Semi");
                        break;
                    }
                }
                token::Comma if break_on_semi == SemiColonMode::Comma &&
                       brace_depth == 0 &&
                       bracket_depth == 0 =>
                {
                    debug!("recover_stmt_ return - Semi");
                    break;
                }
                _ => {
                    self.bump()
                }
            }
        }
    }

    pub(super) fn check_for_for_in_in_typo(&mut self, in_span: Span) {
        if self.eat_keyword(kw::In) {
            // a common typo: `for _ in in bar {}`
            self.struct_span_err(self.prev_span, "expected iterable, found keyword `in`")
                .span_suggestion_short(
                    in_span.until(self.prev_span),
                    "remove the duplicated `in`",
                    String::new(),
                    Applicability::MachineApplicable,
                )
                .emit();
        }
    }

    pub(super) fn expected_semi_or_open_brace<T>(&mut self) -> PResult<'a, T> {
        let token_str = self.this_token_descr();
        let mut err = self.fatal(&format!("expected `;` or `{{`, found {}", token_str));
        err.span_label(self.token.span, "expected `;` or `{`");
        Err(err)
    }

    pub(super) fn eat_incorrect_doc_comment_for_param_type(&mut self) {
        if let token::DocComment(_) = self.token.kind {
            self.struct_span_err(
                self.token.span,
                "documentation comments cannot be applied to a function parameter's type",
            )
            .span_label(self.token.span, "doc comments are not allowed here")
            .emit();
            self.bump();
        } else if self.token == token::Pound && self.look_ahead(1, |t| {
            *t == token::OpenDelim(token::Bracket)
        }) {
            let lo = self.token.span;
            // Skip every token until next possible arg.
            while self.token != token::CloseDelim(token::Bracket) {
                self.bump();
            }
            let sp = lo.to(self.token.span);
            self.bump();
            self.struct_span_err(
                sp,
                "attributes cannot be applied to a function parameter's type",
            )
            .span_label(sp, "attributes are not allowed here")
            .emit();
        }
    }

    pub(super) fn parameter_without_type(
        &mut self,
        err: &mut DiagnosticBuilder<'_>,
        pat: P<ast::Pat>,
        require_name: bool,
        is_self_allowed: bool,
        is_trait_item: bool,
    ) -> Option<Ident> {
        // If we find a pattern followed by an identifier, it could be an (incorrect)
        // C-style parameter declaration.
        if self.check_ident() && self.look_ahead(1, |t| {
            *t == token::Comma || *t == token::CloseDelim(token::Paren)
        }) { // `fn foo(String s) {}`
            let ident = self.parse_ident().unwrap();
            let span = pat.span.with_hi(ident.span.hi());

            err.span_suggestion(
                span,
                "declare the type after the parameter binding",
                String::from("<identifier>: <type>"),
                Applicability::HasPlaceholders,
            );
            return Some(ident);
        } else if let PatKind::Ident(_, ident, _) = pat.kind {
            if require_name && (
                is_trait_item ||
                self.token == token::Comma ||
                self.token == token::Lt ||
                self.token == token::CloseDelim(token::Paren)
            ) { // `fn foo(a, b) {}`, `fn foo(a<x>, b<y>) {}` or `fn foo(usize, usize) {}`
                if is_self_allowed {
                    err.span_suggestion(
                        pat.span,
                        "if this is a `self` type, give it a parameter name",
                        format!("self: {}", ident),
                        Applicability::MaybeIncorrect,
                    );
                }
                // Avoid suggesting that `fn foo(HashMap<u32>)` is fixed with a change to
                // `fn foo(HashMap: TypeName<u32>)`.
                if self.token != token::Lt {
                    err.span_suggestion(
                        pat.span,
                        "if this was a parameter name, give it a type",
                        format!("{}: TypeName", ident),
                        Applicability::HasPlaceholders,
                    );
                }
                err.span_suggestion(
                    pat.span,
                    "if this is a type, explicitly ignore the parameter name",
                    format!("_: {}", ident),
                    Applicability::MachineApplicable,
                );
                err.note("anonymous parameters are removed in the 2018 edition (see RFC 1685)");

                // Don't attempt to recover by using the `X` in `X<Y>` as the parameter name.
                return if self.token == token::Lt { None } else { Some(ident) };
            }
        }
        None
    }

    pub(super) fn recover_arg_parse(&mut self) -> PResult<'a, (P<ast::Pat>, P<ast::Ty>)> {
        let pat = self.parse_pat(Some("argument name"))?;
        self.expect(&token::Colon)?;
        let ty = self.parse_ty()?;

        struct_span_err!(
            self.diagnostic(),
            pat.span,
            E0642,
            "patterns aren't allowed in methods without bodies",
        )
        .span_suggestion_short(
            pat.span,
            "give this argument a name or use an underscore to ignore it",
            "_".to_owned(),
            Applicability::MachineApplicable,
        )
        .emit();

        // Pretend the pattern is `_`, to avoid duplicate errors from AST validation.
        let pat = P(Pat {
            kind: PatKind::Wild,
            span: pat.span,
            id: ast::DUMMY_NODE_ID
        });
        Ok((pat, ty))
    }

    pub(super) fn recover_bad_self_param(
        &mut self,
        mut param: ast::Param,
        is_trait_item: bool,
    ) -> PResult<'a, ast::Param> {
        let sp = param.pat.span;
        param.ty.kind = TyKind::Err;
        let mut err = self.struct_span_err(sp, "unexpected `self` parameter in function");
        if is_trait_item {
            err.span_label(sp, "must be the first associated function parameter");
        } else {
            err.span_label(sp, "not valid as function parameter");
            err.note("`self` is only valid as the first parameter of an associated function");
        }
        err.emit();
        Ok(param)
    }

    pub(super) fn consume_block(
        &mut self,
        delim: token::DelimToken,
        consume_close: ConsumeClosingDelim,
    ) {
        let mut brace_depth = 0;
        loop {
            if self.eat(&token::OpenDelim(delim)) {
                brace_depth += 1;
            } else if self.check(&token::CloseDelim(delim)) {
                if brace_depth == 0 {
                    if let ConsumeClosingDelim::Yes = consume_close {
                        // Some of the callers of this method expect to be able to parse the
                        // closing delimiter themselves, so we leave it alone. Otherwise we advance
                        // the parser.
                        self.bump();
                    }
                    return;
                } else {
                    self.bump();
                    brace_depth -= 1;
                    continue;
                }
            } else if self.token == token::Eof || self.eat(&token::CloseDelim(token::NoDelim)) {
                return;
            } else {
                self.bump();
            }
        }
    }

    pub(super) fn expected_expression_found(&self) -> DiagnosticBuilder<'a> {
        let (span, msg) = match (&self.token.kind, self.subparser_name) {
            (&token::Eof, Some(origin)) => {
                let sp = self.sess.source_map().next_point(self.token.span);
                (sp, format!("expected expression, found end of {}", origin))
            }
            _ => (self.token.span, format!(
                "expected expression, found {}",
                self.this_token_descr(),
            )),
        };
        let mut err = self.struct_span_err(span, &msg);
        let sp = self.sess.source_map().start_point(self.token.span);
        if let Some(sp) = self.sess.ambiguous_block_expr_parse.borrow().get(&sp) {
            self.sess.expr_parentheses_needed(&mut err, *sp, None);
        }
        err.span_label(span, "expected expression");
        err
    }

    fn consume_tts(
        &mut self,
        mut acc: i64, // `i64` because malformed code can have more closing delims than opening.
        // Not using `FxHashMap` due to `token::TokenKind: !Eq + !Hash`.
        modifier: &[(token::TokenKind, i64)],
    ) {
        while acc > 0 {
            if let Some((_, val)) = modifier.iter().find(|(t, _)| *t == self.token.kind) {
                acc += *val;
            }
            if self.token.kind == token::Eof {
                break;
            }
            self.bump();
        }
    }

    /// Replace duplicated recovered parameters with `_` pattern to avoid unecessary errors.
    ///
    /// This is necessary because at this point we don't know whether we parsed a function with
    /// anonymous parameters or a function with names but no types. In order to minimize
    /// unecessary errors, we assume the parameters are in the shape of `fn foo(a, b, c)` where
    /// the parameters are *names* (so we don't emit errors about not being able to find `b` in
    /// the local scope), but if we find the same name multiple times, like in `fn foo(i8, i8)`,
    /// we deduplicate them to not complain about duplicated parameter names.
    pub(super) fn deduplicate_recovered_params_names(&self, fn_inputs: &mut Vec<Param>) {
        let mut seen_inputs = FxHashSet::default();
        for input in fn_inputs.iter_mut() {
            let opt_ident = if let (PatKind::Ident(_, ident, _), TyKind::Err) = (
                &input.pat.kind, &input.ty.kind,
            ) {
                Some(*ident)
            } else {
                None
            };
            if let Some(ident) = opt_ident {
                if seen_inputs.contains(&ident) {
                    input.pat.kind = PatKind::Wild;
                }
                seen_inputs.insert(ident);
            }
        }
    }
}
