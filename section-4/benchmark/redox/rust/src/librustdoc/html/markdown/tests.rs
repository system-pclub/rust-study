use super::{ErrorCodes, LangString, Markdown, MarkdownHtml, IdMap, Ignore};
use super::plain_summary_line;
use std::cell::RefCell;
use syntax::edition::{Edition, DEFAULT_EDITION};

#[test]
fn test_unique_id() {
    let input = ["foo", "examples", "examples", "method.into_iter","examples",
                 "method.into_iter", "foo", "main", "search", "methods",
                 "examples", "method.into_iter", "assoc_type.Item", "assoc_type.Item"];
    let expected = ["foo", "examples", "examples-1", "method.into_iter", "examples-2",
                    "method.into_iter-1", "foo-1", "main", "search", "methods",
                    "examples-3", "method.into_iter-2", "assoc_type.Item", "assoc_type.Item-1"];

    let map = RefCell::new(IdMap::new());
    let test = || {
        let mut map = map.borrow_mut();
        let actual: Vec<String> = input.iter().map(|s| map.derive(s.to_string())).collect();
        assert_eq!(&actual[..], expected);
    };
    test();
    map.borrow_mut().reset();
    test();
}

#[test]
fn test_lang_string_parse() {
    fn t(s: &str,
        should_panic: bool, no_run: bool, ignore: Ignore, rust: bool, test_harness: bool,
        compile_fail: bool, allow_fail: bool, error_codes: Vec<String>,
        edition: Option<Edition>) {
        assert_eq!(LangString::parse(s, ErrorCodes::Yes, true), LangString {
            should_panic,
            no_run,
            ignore,
            rust,
            test_harness,
            compile_fail,
            error_codes,
            original: s.to_owned(),
            allow_fail,
            edition,
        })
    }
    let ignore_foo = Ignore::Some(vec!("foo".to_string()));

    fn v() -> Vec<String> {
        Vec::new()
    }

    // ignore-tidy-linelength
    // marker                | should_panic | no_run | ignore | rust | test_harness
    //                       | compile_fail | allow_fail | error_codes | edition
    t("",                      false,         false,   Ignore::None,   true,  false, false, false, v(), None);
    t("rust",                  false,         false,   Ignore::None,   true,  false, false, false, v(), None);
    t("sh",                    false,         false,   Ignore::None,   false, false, false, false, v(), None);
    t("ignore",                false,         false,   Ignore::All,    true,  false, false, false, v(), None);
    t("ignore-foo",            false,         false,   ignore_foo,     true,  false, false, false, v(), None);
    t("should_panic",          true,          false,   Ignore::None,   true,  false, false, false, v(), None);
    t("no_run",                false,         true,    Ignore::None,   true,  false, false, false, v(), None);
    t("test_harness",          false,         false,   Ignore::None,   true,  true,  false, false, v(), None);
    t("compile_fail",          false,         true,    Ignore::None,   true,  false, true,  false, v(), None);
    t("allow_fail",            false,         false,   Ignore::None,   true,  false, false, true,  v(), None);
    t("{.no_run .example}",    false,         true,    Ignore::None,   true,  false, false, false, v(), None);
    t("{.sh .should_panic}",   true,          false,   Ignore::None,   false, false, false, false, v(), None);
    t("{.example .rust}",      false,         false,   Ignore::None,   true,  false, false, false, v(), None);
    t("{.test_harness .rust}", false,         false,   Ignore::None,   true,  true,  false, false, v(), None);
    t("text, no_run",          false,         true,    Ignore::None,   false, false, false, false, v(), None);
    t("text,no_run",           false,         true,    Ignore::None,   false, false, false, false, v(), None);
    t("edition2015",           false,         false,   Ignore::None,   true,  false, false, false, v(), Some(Edition::Edition2015));
    t("edition2018",           false,         false,   Ignore::None,   true,  false, false, false, v(), Some(Edition::Edition2018));
}

#[test]
fn test_header() {
    fn t(input: &str, expect: &str) {
        let mut map = IdMap::new();
        let output = Markdown(
            input, &[], &mut map, ErrorCodes::Yes, DEFAULT_EDITION, &None).to_string();
        assert_eq!(output, expect, "original: {}", input);
    }

    t("# Foo bar", "<h1 id=\"foo-bar\" class=\"section-header\">\
      <a href=\"#foo-bar\">Foo bar</a></h1>");
    t("## Foo-bar_baz qux", "<h2 id=\"foo-bar_baz-qux\" class=\"section-\
      header\"><a href=\"#foo-bar_baz-qux\">Foo-bar_baz qux</a></h2>");
    t("### **Foo** *bar* baz!?!& -_qux_-%",
      "<h3 id=\"foo-bar-baz--qux-\" class=\"section-header\">\
      <a href=\"#foo-bar-baz--qux-\"><strong>Foo</strong> \
      <em>bar</em> baz!?!&amp; -<em>qux</em>-%</a></h3>");
    t("#### **Foo?** & \\*bar?!*  _`baz`_ ❤ #qux",
      "<h4 id=\"foo--bar--baz--qux\" class=\"section-header\">\
      <a href=\"#foo--bar--baz--qux\"><strong>Foo?</strong> &amp; *bar?!*  \
      <em><code>baz</code></em> ❤ #qux</a></h4>");
}

#[test]
fn test_header_ids_multiple_blocks() {
    let mut map = IdMap::new();
    fn t(map: &mut IdMap, input: &str, expect: &str) {
        let output = Markdown(input, &[], map,
                              ErrorCodes::Yes, DEFAULT_EDITION, &None).to_string();
        assert_eq!(output, expect, "original: {}", input);
    }

    t(&mut map, "# Example", "<h1 id=\"example\" class=\"section-header\">\
        <a href=\"#example\">Example</a></h1>");
    t(&mut map, "# Panics", "<h1 id=\"panics\" class=\"section-header\">\
        <a href=\"#panics\">Panics</a></h1>");
    t(&mut map, "# Example", "<h1 id=\"example-1\" class=\"section-header\">\
        <a href=\"#example-1\">Example</a></h1>");
    t(&mut map, "# Main", "<h1 id=\"main\" class=\"section-header\">\
        <a href=\"#main\">Main</a></h1>");
    t(&mut map, "# Example", "<h1 id=\"example-2\" class=\"section-header\">\
        <a href=\"#example-2\">Example</a></h1>");
    t(&mut map, "# Panics", "<h1 id=\"panics-1\" class=\"section-header\">\
        <a href=\"#panics-1\">Panics</a></h1>");
}

#[test]
fn test_plain_summary_line() {
    fn t(input: &str, expect: &str) {
        let output = plain_summary_line(input);
        assert_eq!(output, expect, "original: {}", input);
    }

    t("hello [Rust](https://www.rust-lang.org) :)", "hello Rust :)");
    t("hello [Rust](https://www.rust-lang.org \"Rust\") :)", "hello Rust :)");
    t("code `let x = i32;` ...", "code `let x = i32;` ...");
    t("type `Type<'static>` ...", "type `Type<'static>` ...");
    t("# top header", "top header");
    t("## header", "header");
}

#[test]
fn test_markdown_html_escape() {
    fn t(input: &str, expect: &str) {
        let mut idmap = IdMap::new();
        let output = MarkdownHtml(input, &mut idmap,
                                  ErrorCodes::Yes, DEFAULT_EDITION, &None).to_string();
        assert_eq!(output, expect, "original: {}", input);
    }

    t("`Struct<'a, T>`", "<p><code>Struct&lt;'a, T&gt;</code></p>\n");
    t("Struct<'a, T>", "<p>Struct&lt;'a, T&gt;</p>\n");
    t("Struct<br>", "<p>Struct&lt;br&gt;</p>\n");
}
