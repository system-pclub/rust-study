// run-pass

#![allow(unused_imports)]
// ignore-cross-compile

#![feature(rustc_private)]

extern crate syntax;
extern crate syntax_expand;
extern crate rustc_parse;
extern crate rustc_errors;

use rustc_errors::PResult;
use rustc_parse::parser::attr::*;
use rustc_parse::new_parser_from_source_str;
use rustc_parse::parser::Parser;
use syntax::ast::*;
use syntax::attr::*;
use syntax::ast;
use syntax::sess::ParseSess;
use syntax::source_map::{FilePathMapping, FileName};
use syntax::ptr::P;
use syntax::print::pprust;
use syntax::token;
use std::fmt;

// Copied out of syntax::util::parser_testing

pub fn string_to_parser<'a>(ps: &'a ParseSess, source_str: String) -> Parser<'a> {
    new_parser_from_source_str(ps, FileName::Custom(source_str.clone()), source_str)
}

fn with_error_checking_parse<'a, T, F>(s: String, ps: &'a ParseSess, f: F) -> PResult<'a, T> where
    F: FnOnce(&mut Parser<'a>) -> PResult<'a, T>,
{
    let mut p = string_to_parser(&ps, s);
    let x = f(&mut p);

    if ps.span_diagnostic.has_errors() || p.token != token::Eof {
        if let Err(mut e) = x {
            e.cancel();
        }
        return Err(p.fatal("parse error"));
    }

    x
}

fn expr<'a>(s: &str, ps: &'a ParseSess) -> PResult<'a, P<ast::Expr>> {
    with_error_checking_parse(s.to_string(), ps, |p| {
        p.parse_expr()
    })
}

fn stmt<'a>(s: &str, ps: &'a ParseSess) -> PResult<'a, ast::Stmt> {
    with_error_checking_parse(s.to_string(), ps, |p| {
        p.parse_stmt().map(|s| s.unwrap())
    })
}

fn attr<'a>(s: &str, ps: &'a ParseSess) -> PResult<'a, ast::Attribute> {
    with_error_checking_parse(s.to_string(), ps, |p| {
        p.parse_attribute(true)
    })
}

fn str_compare<T, F: Fn(&T) -> String>(e: &str, expected: &[T], actual: &[T], f: F) {
    let expected: Vec<_> = expected.iter().map(|e| f(e)).collect();
    let actual: Vec<_> = actual.iter().map(|e| f(e)).collect();

    if expected != actual {
        panic!("parsed `{}` as {:?}, expected {:?}", e, actual, expected);
    }
}

fn sess() -> ParseSess {
    ParseSess::new(FilePathMapping::empty())
}

fn check_expr_attrs(es: &str, expected: &[&str]) {
    let ps = sess();
    let e = expr(es, &ps).expect("parse error");
    let actual = &e.attrs;
    str_compare(es,
                &expected.iter().map(|r| attr(r, &ps).unwrap()).collect::<Vec<_>>(),
                &actual,
                pprust::attribute_to_string);
}

fn check_stmt_attrs(es: &str, expected: &[&str]) {
    let ps = sess();
    let e = stmt(es, &ps).expect("parse error");
    let actual = e.kind.attrs();
    str_compare(es,
                &expected.iter().map(|r| attr(r, &ps).unwrap()).collect::<Vec<_>>(),
                actual,
                pprust::attribute_to_string);
}

fn reject_expr_parse(es: &str) {
    let ps = sess();
    match expr(es, &ps) {
        Ok(_) => panic!("parser did not reject `{}`", es),
        Err(mut e) => e.cancel(),
    };
}

fn reject_stmt_parse(es: &str) {
    let ps = sess();
    match stmt(es, &ps) {
        Ok(_) => panic!("parser did not reject `{}`", es),
        Err(mut e) => e.cancel(),
    };
}

fn main() {
    syntax::with_default_globals(|| run());
}

fn run() {
    let both = &["#[attr]", "#![attr]"];
    let outer = &["#[attr]"];
    let none = &[];

    check_expr_attrs("#[attr] box 0", outer);
    reject_expr_parse("box #![attr] 0");

    check_expr_attrs("#[attr] [#![attr]]", both);
    check_expr_attrs("#[attr] [#![attr] 0]", both);
    check_expr_attrs("#[attr] [#![attr] 0; 0]", both);
    check_expr_attrs("#[attr] [#![attr] 0, 0, 0]", both);
    reject_expr_parse("[#[attr]]");

    check_expr_attrs("#[attr] foo()", outer);
    check_expr_attrs("#[attr] x.foo()", outer);
    reject_expr_parse("foo#[attr]()");
    reject_expr_parse("foo(#![attr])");
    reject_expr_parse("x.foo(#![attr])");
    reject_expr_parse("x.#[attr]foo()");
    reject_expr_parse("x.#![attr]foo()");

    check_expr_attrs("#[attr] (#![attr])", both);
    check_expr_attrs("#[attr] (#![attr] #[attr] 0,)", both);
    check_expr_attrs("#[attr] (#![attr] #[attr] 0, 0)", both);

    check_expr_attrs("#[attr] 0 + #[attr] 0", none);
    check_expr_attrs("#[attr] 0 / #[attr] 0", none);
    check_expr_attrs("#[attr] 0 & #[attr] 0", none);
    check_expr_attrs("#[attr] 0 % #[attr] 0", none);
    check_expr_attrs("#[attr] (0 + 0)", outer);
    reject_expr_parse("0 + #![attr] 0");

    check_expr_attrs("#[attr] !0", outer);
    check_expr_attrs("#[attr] -0", outer);
    reject_expr_parse("!#![attr] 0");
    reject_expr_parse("-#![attr] 0");

    check_expr_attrs("#[attr] false", outer);
    check_expr_attrs("#[attr] 0", outer);
    check_expr_attrs("#[attr] 'c'", outer);

    check_expr_attrs("#[attr] x as Y", none);
    check_expr_attrs("#[attr] (x as Y)", outer);
    reject_expr_parse("x #![attr] as Y");

    reject_expr_parse("#[attr] if false {}");
    reject_expr_parse("if false #[attr] {}");
    reject_expr_parse("if false {#![attr]}");
    reject_expr_parse("if false {} #[attr] else {}");
    reject_expr_parse("if false {} else #[attr] {}");
    reject_expr_parse("if false {} else {#![attr]}");
    reject_expr_parse("if false {} else #[attr] if true {}");
    reject_expr_parse("if false {} else if true #[attr] {}");
    reject_expr_parse("if false {} else if true {#![attr]}");

    reject_expr_parse("#[attr] if let Some(false) = false {}");
    reject_expr_parse("if let Some(false) = false #[attr] {}");
    reject_expr_parse("if let Some(false) = false {#![attr]}");
    reject_expr_parse("if let Some(false) = false {} #[attr] else {}");
    reject_expr_parse("if let Some(false) = false {} else #[attr] {}");
    reject_expr_parse("if let Some(false) = false {} else {#![attr]}");
    reject_expr_parse("if let Some(false) = false {} else #[attr] if let Some(false) = true {}");
    reject_expr_parse("if let Some(false) = false {} else if let Some(false) = true #[attr] {}");
    reject_expr_parse("if let Some(false) = false {} else if let Some(false) = true {#![attr]}");

    check_expr_attrs("#[attr] while true {#![attr]}", both);

    check_expr_attrs("#[attr] while let Some(false) = true {#![attr]}", both);

    check_expr_attrs("#[attr] for x in y {#![attr]}", both);

    check_expr_attrs("#[attr] loop {#![attr]}", both);

    check_expr_attrs("#[attr] match true {#![attr] #[attr] _ => false}", both);

    check_expr_attrs("#[attr]      || #[attr] foo", outer);
    check_expr_attrs("#[attr] move || #[attr] foo", outer);
    check_expr_attrs("#[attr]      || #[attr] { #![attr] foo }", outer);
    check_expr_attrs("#[attr] move || #[attr] { #![attr] foo }", outer);
    check_expr_attrs("#[attr]      || { #![attr] foo }", outer);
    check_expr_attrs("#[attr] move || { #![attr] foo }", outer);
    reject_expr_parse("|| #![attr] foo");
    reject_expr_parse("move || #![attr] foo");
    reject_expr_parse("|| #![attr] {foo}");
    reject_expr_parse("move || #![attr] {foo}");

    check_expr_attrs("#[attr] { #![attr] }", both);
    check_expr_attrs("#[attr] { #![attr] let _ = (); }", both);
    check_expr_attrs("#[attr] { #![attr] let _ = (); foo }", both);

    check_expr_attrs("#[attr] x = y", none);
    check_expr_attrs("#[attr] (x = y)", outer);

    check_expr_attrs("#[attr] x += y", none);
    check_expr_attrs("#[attr] (x += y)", outer);

    check_expr_attrs("#[attr] foo.bar", outer);
    check_expr_attrs("(#[attr] foo).bar", none);

    check_expr_attrs("#[attr] foo.0", outer);
    check_expr_attrs("(#[attr] foo).0", none);

    check_expr_attrs("#[attr] foo[bar]", outer);
    check_expr_attrs("(#[attr] foo)[bar]", none);

    check_expr_attrs("#[attr] 0..#[attr] 0", none);
    check_expr_attrs("#[attr] 0..", none);
    reject_expr_parse("#[attr] ..#[attr] 0");
    reject_expr_parse("#[attr] ..");

    check_expr_attrs("#[attr] (0..0)", outer);
    check_expr_attrs("#[attr] (0..)", outer);
    check_expr_attrs("#[attr] (..0)", outer);
    check_expr_attrs("#[attr] (..)", outer);

    check_expr_attrs("#[attr] foo::bar::baz", outer);

    check_expr_attrs("#[attr] &0", outer);
    check_expr_attrs("#[attr] &mut 0", outer);
    check_expr_attrs("#[attr] & #[attr] 0", outer);
    check_expr_attrs("#[attr] &mut #[attr] 0", outer);
    reject_expr_parse("#[attr] &#![attr] 0");
    reject_expr_parse("#[attr] &mut #![attr] 0");

    check_expr_attrs("#[attr] break", outer);
    check_expr_attrs("#[attr] continue", outer);
    check_expr_attrs("#[attr] return", outer);

    check_expr_attrs("#[attr] foo!()", outer);
    check_expr_attrs("#[attr] foo!(#![attr])", outer);
    check_expr_attrs("#[attr] foo![]", outer);
    check_expr_attrs("#[attr] foo![#![attr]]", outer);
    check_expr_attrs("#[attr] foo!{}", outer);
    check_expr_attrs("#[attr] foo!{#![attr]}", outer);

    check_expr_attrs("#[attr] Foo { #![attr] bar: baz }", both);
    check_expr_attrs("#[attr] Foo { #![attr] ..foo }", both);
    check_expr_attrs("#[attr] Foo { #![attr] bar: baz, ..foo }", both);

    check_expr_attrs("#[attr] (#![attr] 0)", both);

    // Look at statements in their natural habitat...
    check_expr_attrs("{
        #[attr] let _ = 0;
        #[attr] 0;
        #[attr] foo!();
        #[attr] foo!{}
        #[attr] foo![];
    }", none);

    check_stmt_attrs("#[attr] let _ = 0", outer);
    check_stmt_attrs("#[attr] 0",         outer);
    check_stmt_attrs("#[attr] {#![attr]}", both);
    check_stmt_attrs("#[attr] foo!()",    outer);
    check_stmt_attrs("#[attr] foo![]",    outer);
    check_stmt_attrs("#[attr] foo!{}",    outer);

    reject_stmt_parse("#[attr] #![attr] let _ = 0");
    reject_stmt_parse("#[attr] #![attr] 0");
    reject_stmt_parse("#[attr] #![attr] foo!()");
    reject_stmt_parse("#[attr] #![attr] foo![]");
    reject_stmt_parse("#[attr] #![attr] foo!{}");

    // FIXME: Allow attributes in pattern constexprs?
    // note: requires parens in patterns to allow disambiguation

    reject_expr_parse("match 0 {
        0..=#[attr] 10 => ()
    }");
    reject_expr_parse("match 0 {
        0..=#[attr] -10 => ()
    }");
    reject_expr_parse("match 0 {
        0..=-#[attr] 10 => ()
    }");
    reject_expr_parse("match 0 {
        0..=#[attr] FOO => ()
    }");

    // make sure we don't catch this bug again...
    reject_expr_parse("{
        fn foo() {
            #[attr];
        }
    }");
    reject_expr_parse("{
        fn foo() {
            #[attr]
        }
    }");
}
