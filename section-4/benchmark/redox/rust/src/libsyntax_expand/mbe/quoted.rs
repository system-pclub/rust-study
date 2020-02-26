use crate::mbe::macro_parser;
use crate::mbe::{TokenTree, KleeneOp, KleeneToken, SequenceRepetition, Delimited};

use syntax::ast;
use syntax::print::pprust;
use syntax::sess::ParseSess;
use syntax::symbol::kw;
use syntax::token::{self, Token};
use syntax::tokenstream;

use syntax_pos::Span;

use rustc_data_structures::sync::Lrc;

/// Takes a `tokenstream::TokenStream` and returns a `Vec<self::TokenTree>`. Specifically, this
/// takes a generic `TokenStream`, such as is used in the rest of the compiler, and returns a
/// collection of `TokenTree` for use in parsing a macro.
///
/// # Parameters
///
/// - `input`: a token stream to read from, the contents of which we are parsing.
/// - `expect_matchers`: `parse` can be used to parse either the "patterns" or the "body" of a
///   macro. Both take roughly the same form _except_ that in a pattern, metavars are declared with
///   their "matcher" type. For example `$var:expr` or `$id:ident`. In this example, `expr` and
///   `ident` are "matchers". They are not present in the body of a macro rule -- just in the
///   pattern, so we pass a parameter to indicate whether to expect them or not.
/// - `sess`: the parsing session. Any errors will be emitted to this session.
/// - `features`, `attrs`: language feature flags and attributes so that we know whether to use
///   unstable features or not.
/// - `edition`: which edition are we in.
/// - `macro_node_id`: the NodeId of the macro we are parsing.
///
/// # Returns
///
/// A collection of `self::TokenTree`. There may also be some errors emitted to `sess`.
pub(super) fn parse(
    input: tokenstream::TokenStream,
    expect_matchers: bool,
    sess: &ParseSess,
) -> Vec<TokenTree> {
    // Will contain the final collection of `self::TokenTree`
    let mut result = Vec::new();

    // For each token tree in `input`, parse the token into a `self::TokenTree`, consuming
    // additional trees if need be.
    let mut trees = input.trees();
    while let Some(tree) = trees.next() {
        // Given the parsed tree, if there is a metavar and we are expecting matchers, actually
        // parse out the matcher (i.e., in `$id:ident` this would parse the `:` and `ident`).
        let tree = parse_tree(
            tree,
            &mut trees,
            expect_matchers,
            sess,
        );
        match tree {
            TokenTree::MetaVar(start_sp, ident) if expect_matchers => {
                let span = match trees.next() {
                    Some(tokenstream::TokenTree::Token(Token { kind: token::Colon, span })) => {
                        match trees.next() {
                            Some(tokenstream::TokenTree::Token(token)) => match token.ident() {
                                Some((kind, _)) => {
                                    let span = token.span.with_lo(start_sp.lo());
                                    result.push(TokenTree::MetaVarDecl(span, ident, kind));
                                    continue;
                                }
                                _ => token.span,
                            },
                            tree => tree.as_ref().map(tokenstream::TokenTree::span).unwrap_or(span),
                        }
                    }
                    tree => tree.as_ref().map(tokenstream::TokenTree::span).unwrap_or(start_sp),
                };
                sess.missing_fragment_specifiers.borrow_mut().insert(span);
                result.push(TokenTree::MetaVarDecl(span, ident, ast::Ident::invalid()));
            }

            // Not a metavar or no matchers allowed, so just return the tree
            _ => result.push(tree),
        }
    }
    result
}

/// Takes a `tokenstream::TokenTree` and returns a `self::TokenTree`. Specifically, this takes a
/// generic `TokenTree`, such as is used in the rest of the compiler, and returns a `TokenTree`
/// for use in parsing a macro.
///
/// Converting the given tree may involve reading more tokens.
///
/// # Parameters
///
/// - `tree`: the tree we wish to convert.
/// - `trees`: an iterator over trees. We may need to read more tokens from it in order to finish
///   converting `tree`
/// - `expect_matchers`: same as for `parse` (see above).
/// - `sess`: the parsing session. Any errors will be emitted to this session.
/// - `features`, `attrs`: language feature flags and attributes so that we know whether to use
///   unstable features or not.
fn parse_tree(
    tree: tokenstream::TokenTree,
    trees: &mut impl Iterator<Item = tokenstream::TokenTree>,
    expect_matchers: bool,
    sess: &ParseSess,
) -> TokenTree {
    // Depending on what `tree` is, we could be parsing different parts of a macro
    match tree {
        // `tree` is a `$` token. Look at the next token in `trees`
        tokenstream::TokenTree::Token(Token { kind: token::Dollar, span }) => match trees.next() {
            // `tree` is followed by a delimited set of token trees. This indicates the beginning
            // of a repetition sequence in the macro (e.g. `$(pat)*`).
            Some(tokenstream::TokenTree::Delimited(span, delim, tts)) => {
                // Must have `(` not `{` or `[`
                if delim != token::Paren {
                    let tok = pprust::token_kind_to_string(&token::OpenDelim(delim));
                    let msg = format!("expected `(`, found `{}`", tok);
                    sess.span_diagnostic.span_err(span.entire(), &msg);
                }
                // Parse the contents of the sequence itself
                let sequence = parse(
                    tts.into(),
                    expect_matchers,
                    sess,
                );
                // Get the Kleene operator and optional separator
                let (separator, kleene) = parse_sep_and_kleene_op(trees, span.entire(), sess);
                // Count the number of captured "names" (i.e., named metavars)
                let name_captures = macro_parser::count_names(&sequence);
                TokenTree::Sequence(
                    span,
                    Lrc::new(SequenceRepetition {
                        tts: sequence,
                        separator,
                        kleene,
                        num_captures: name_captures,
                    }),
                )
            }

            // `tree` is followed by an `ident`. This could be `$meta_var` or the `$crate` special
            // metavariable that names the crate of the invocation.
            Some(tokenstream::TokenTree::Token(token)) if token.is_ident() => {
                let (ident, is_raw) = token.ident().unwrap();
                let span = ident.span.with_lo(span.lo());
                if ident.name == kw::Crate && !is_raw {
                    TokenTree::token(token::Ident(kw::DollarCrate, is_raw), span)
                } else {
                    TokenTree::MetaVar(span, ident)
                }
            }

            // `tree` is followed by a random token. This is an error.
            Some(tokenstream::TokenTree::Token(token)) => {
                let msg =
                    format!("expected identifier, found `{}`", pprust::token_to_string(&token),);
                sess.span_diagnostic.span_err(token.span, &msg);
                TokenTree::MetaVar(token.span, ast::Ident::invalid())
            }

            // There are no more tokens. Just return the `$` we already have.
            None => TokenTree::token(token::Dollar, span),
        },

        // `tree` is an arbitrary token. Keep it.
        tokenstream::TokenTree::Token(token) => TokenTree::Token(token),

        // `tree` is the beginning of a delimited set of tokens (e.g., `(` or `{`). We need to
        // descend into the delimited set and further parse it.
        tokenstream::TokenTree::Delimited(span, delim, tts) => TokenTree::Delimited(
            span,
            Lrc::new(Delimited {
                delim,
                tts: parse(
                    tts.into(),
                    expect_matchers,
                    sess,
                ),
            }),
        ),
    }
}

/// Takes a token and returns `Some(KleeneOp)` if the token is `+` `*` or `?`. Otherwise, return
/// `None`.
fn kleene_op(token: &Token) -> Option<KleeneOp> {
    match token.kind {
        token::BinOp(token::Star) => Some(KleeneOp::ZeroOrMore),
        token::BinOp(token::Plus) => Some(KleeneOp::OneOrMore),
        token::Question => Some(KleeneOp::ZeroOrOne),
        _ => None,
    }
}

/// Parse the next token tree of the input looking for a KleeneOp. Returns
///
/// - Ok(Ok((op, span))) if the next token tree is a KleeneOp
/// - Ok(Err(tok, span)) if the next token tree is a token but not a KleeneOp
/// - Err(span) if the next token tree is not a token
fn parse_kleene_op(
    input: &mut impl Iterator<Item = tokenstream::TokenTree>,
    span: Span,
) -> Result<Result<(KleeneOp, Span), Token>, Span> {
    match input.next() {
        Some(tokenstream::TokenTree::Token(token)) => match kleene_op(&token) {
            Some(op) => Ok(Ok((op, token.span))),
            None => Ok(Err(token)),
        },
        tree => Err(tree.as_ref().map(tokenstream::TokenTree::span).unwrap_or(span)),
    }
}

/// Attempt to parse a single Kleene star, possibly with a separator.
///
/// For example, in a pattern such as `$(a),*`, `a` is the pattern to be repeated, `,` is the
/// separator, and `*` is the Kleene operator. This function is specifically concerned with parsing
/// the last two tokens of such a pattern: namely, the optional separator and the Kleene operator
/// itself. Note that here we are parsing the _macro_ itself, rather than trying to match some
/// stream of tokens in an invocation of a macro.
///
/// This function will take some input iterator `input` corresponding to `span` and a parsing
/// session `sess`. If the next one (or possibly two) tokens in `input` correspond to a Kleene
/// operator and separator, then a tuple with `(separator, KleeneOp)` is returned. Otherwise, an
/// error with the appropriate span is emitted to `sess` and a dummy value is returned.
fn parse_sep_and_kleene_op(
    input: &mut impl Iterator<Item = tokenstream::TokenTree>,
    span: Span,
    sess: &ParseSess,
) -> (Option<Token>, KleeneToken) {
    // We basically look at two token trees here, denoted as #1 and #2 below
    let span = match parse_kleene_op(input, span) {
        // #1 is a `?`, `+`, or `*` KleeneOp
        Ok(Ok((op, span))) => return (None, KleeneToken::new(op, span)),

        // #1 is a separator followed by #2, a KleeneOp
        Ok(Err(token)) => match parse_kleene_op(input, token.span) {
            // #2 is the `?` Kleene op, which does not take a separator (error)
            Ok(Ok((KleeneOp::ZeroOrOne, span))) => {
                // Error!
                sess.span_diagnostic.span_err(
                    token.span,
                    "the `?` macro repetition operator does not take a separator",
                );

                // Return a dummy
                return (None, KleeneToken::new(KleeneOp::ZeroOrMore, span));
            }

            // #2 is a KleeneOp :D
            Ok(Ok((op, span))) => return (Some(token), KleeneToken::new(op, span)),

            // #2 is a random token or not a token at all :(
            Ok(Err(Token { span, .. })) | Err(span) => span,
        },

        // #1 is not a token
        Err(span) => span,
    };

    // If we ever get to this point, we have experienced an "unexpected token" error
    sess.span_diagnostic.span_err(span, "expected one of: `*`, `+`, or `?`");

    // Return a dummy
    (None, KleeneToken::new(KleeneOp::ZeroOrMore, span))
}
