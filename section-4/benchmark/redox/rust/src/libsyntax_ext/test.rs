/// The expansion from a test function to the appropriate test struct for libtest
/// Ideally, this code would be in libtest but for efficiency and error messages it lives here.

use crate::util::check_builtin_macro_attribute;

use syntax::ast;
use syntax::attr;
use syntax_expand::base::*;
use syntax::print::pprust;
use syntax::source_map::respan;
use syntax::symbol::{Symbol, sym};
use syntax_pos::Span;

use std::iter;

// #[test_case] is used by custom test authors to mark tests
// When building for test, it needs to make the item public and gensym the name
// Otherwise, we'll omit the item. This behavior means that any item annotated
// with #[test_case] is never addressable.
//
// We mark item with an inert attribute "rustc_test_marker" which the test generation
// logic will pick up on.
pub fn expand_test_case(
    ecx: &mut ExtCtxt<'_>,
    attr_sp: Span,
    meta_item: &ast::MetaItem,
    anno_item: Annotatable
) -> Vec<Annotatable> {
    check_builtin_macro_attribute(ecx, meta_item, sym::test_case);

    if !ecx.ecfg.should_test { return vec![]; }

    let sp = ecx.with_def_site_ctxt(attr_sp);
    let mut item = anno_item.expect_item();
    item = item.map(|mut item| {
        item.vis = respan(item.vis.span, ast::VisibilityKind::Public);
        item.ident.span = item.ident.span.with_ctxt(sp.ctxt());
        item.attrs.push(
            ecx.attribute(ecx.meta_word(sp, sym::rustc_test_marker))
        );
        item
    });

    return vec![Annotatable::Item(item)]
}

pub fn expand_test(
    cx: &mut ExtCtxt<'_>,
    attr_sp: Span,
    meta_item: &ast::MetaItem,
    item: Annotatable,
) -> Vec<Annotatable> {
    check_builtin_macro_attribute(cx, meta_item, sym::test);
    expand_test_or_bench(cx, attr_sp, item, false)
}

pub fn expand_bench(
    cx: &mut ExtCtxt<'_>,
    attr_sp: Span,
    meta_item: &ast::MetaItem,
    item: Annotatable,
) -> Vec<Annotatable> {
    check_builtin_macro_attribute(cx, meta_item, sym::bench);
    expand_test_or_bench(cx, attr_sp, item, true)
}

pub fn expand_test_or_bench(
    cx: &mut ExtCtxt<'_>,
    attr_sp: Span,
    item: Annotatable,
    is_bench: bool
) -> Vec<Annotatable> {
    // If we're not in test configuration, remove the annotated item
    if !cx.ecfg.should_test { return vec![]; }

    let item =
        if let Annotatable::Item(i) = item { i }
        else {
            cx.parse_sess.span_diagnostic.span_fatal(item.span(),
                "`#[test]` attribute is only allowed on non associated functions").raise();
        };

    if let ast::ItemKind::Mac(_) = item.kind {
        cx.parse_sess.span_diagnostic.span_warn(item.span,
            "`#[test]` attribute should not be used on macros. Use `#[cfg(test)]` instead.");
        return vec![Annotatable::Item(item)];
    }

    // has_*_signature will report any errors in the type so compilation
    // will fail. We shouldn't try to expand in this case because the errors
    // would be spurious.
    if (!is_bench && !has_test_signature(cx, &item)) ||
        (is_bench && !has_bench_signature(cx, &item)) {
        return vec![Annotatable::Item(item)];
    }

    let (sp, attr_sp) = (cx.with_def_site_ctxt(item.span), cx.with_def_site_ctxt(attr_sp));

    let test_id = ast::Ident::new(sym::test, attr_sp);

    // creates test::$name
    let test_path = |name| {
        cx.path(sp, vec![test_id, cx.ident_of(name, sp)])
    };

    // creates test::ShouldPanic::$name
    let should_panic_path = |name| {
        cx.path(sp, vec![test_id, cx.ident_of("ShouldPanic", sp), cx.ident_of(name, sp)])
    };

    // creates test::TestType::$name
    let test_type_path = |name| {
        cx.path(sp, vec![test_id, cx.ident_of("TestType", sp), cx.ident_of(name, sp)])
    };

    // creates $name: $expr
    let field = |name, expr| cx.field_imm(sp, cx.ident_of(name, sp), expr);

    let test_fn = if is_bench {
        // A simple ident for a lambda
        let b = cx.ident_of("b", attr_sp);

        cx.expr_call(sp, cx.expr_path(test_path("StaticBenchFn")), vec![
            // |b| self::test::assert_test_result(
            cx.lambda1(sp,
                cx.expr_call(sp, cx.expr_path(test_path("assert_test_result")), vec![
                    // super::$test_fn(b)
                    cx.expr_call(sp,
                        cx.expr_path(cx.path(sp, vec![item.ident])),
                        vec![cx.expr_ident(sp, b)])
                ]),
                b
            )
            // )
        ])
    } else {
        cx.expr_call(sp, cx.expr_path(test_path("StaticTestFn")), vec![
            // || {
            cx.lambda0(sp,
                // test::assert_test_result(
                cx.expr_call(sp, cx.expr_path(test_path("assert_test_result")), vec![
                    // $test_fn()
                    cx.expr_call(sp, cx.expr_path(cx.path(sp, vec![item.ident])), vec![])
                // )
                ])
            // }
            )
        // )
        ])
    };

    let mut test_const = cx.item(sp, ast::Ident::new(item.ident.name, sp),
        vec![
            // #[cfg(test)]
            cx.attribute(attr::mk_list_item(ast::Ident::new(sym::cfg, attr_sp), vec![
                attr::mk_nested_word_item(ast::Ident::new(sym::test, attr_sp))
            ])),
            // #[rustc_test_marker]
            cx.attribute(cx.meta_word(attr_sp, sym::rustc_test_marker)),
        ],
        // const $ident: test::TestDescAndFn =
        ast::ItemKind::Const(cx.ty(sp, ast::TyKind::Path(None, test_path("TestDescAndFn"))),
            // test::TestDescAndFn {
            cx.expr_struct(sp, test_path("TestDescAndFn"), vec![
                // desc: test::TestDesc {
                field("desc", cx.expr_struct(sp, test_path("TestDesc"), vec![
                    // name: "path::to::test"
                    field("name", cx.expr_call(sp, cx.expr_path(test_path("StaticTestName")),
                        vec![
                            cx.expr_str(sp, Symbol::intern(&item_path(
                                // skip the name of the root module
                                &cx.current_expansion.module.mod_path[1..],
                                &item.ident
                            )))
                        ])),
                    // ignore: true | false
                    field("ignore", cx.expr_bool(sp, should_ignore(&item))),
                    // allow_fail: true | false
                    field("allow_fail", cx.expr_bool(sp, should_fail(&item))),
                    // should_panic: ...
                    field("should_panic", match should_panic(cx, &item) {
                        // test::ShouldPanic::No
                        ShouldPanic::No => cx.expr_path(should_panic_path("No")),
                        // test::ShouldPanic::Yes
                        ShouldPanic::Yes(None) => cx.expr_path(should_panic_path("Yes")),
                        // test::ShouldPanic::YesWithMessage("...")
                        ShouldPanic::Yes(Some(sym)) => cx.expr_call(sp,
                            cx.expr_path(should_panic_path("YesWithMessage")),
                            vec![cx.expr_str(sp, sym)]),
                    }),
                    // test_type: ...
                    field("test_type", match test_type(cx) {
                        // test::TestType::UnitTest
                        TestType::UnitTest => cx.expr_path(test_type_path("UnitTest")),
                        // test::TestType::IntegrationTest
                        TestType::IntegrationTest => cx.expr_path(
                            test_type_path("IntegrationTest")
                        ),
                        // test::TestPath::Unknown
                        TestType::Unknown => cx.expr_path(test_type_path("Unknown")),
                    }),
                // },
                ])),
                // testfn: test::StaticTestFn(...) | test::StaticBenchFn(...)
                field("testfn", test_fn)
            // }
            ])
        // }
        ));
    test_const = test_const.map(|mut tc| { tc.vis.node = ast::VisibilityKind::Public; tc});

    // extern crate test
    let test_extern = cx.item(sp,
        test_id,
        vec![],
        ast::ItemKind::ExternCrate(None)
    );

    log::debug!("synthetic test item:\n{}\n", pprust::item_to_string(&test_const));

    vec![
        // Access to libtest under a hygienic name
        Annotatable::Item(test_extern),
        // The generated test case
        Annotatable::Item(test_const),
        // The original item
        Annotatable::Item(item)
    ]
}

fn item_path(mod_path: &[ast::Ident], item_ident: &ast::Ident) -> String {
    mod_path.iter().chain(iter::once(item_ident))
        .map(|x| x.to_string()).collect::<Vec<String>>().join("::")
}

enum ShouldPanic {
    No,
    Yes(Option<Symbol>),
}

fn should_ignore(i: &ast::Item) -> bool {
    attr::contains_name(&i.attrs, sym::ignore)
}

fn should_fail(i: &ast::Item) -> bool {
    attr::contains_name(&i.attrs, sym::allow_fail)
}

fn should_panic(cx: &ExtCtxt<'_>, i: &ast::Item) -> ShouldPanic {
    match attr::find_by_name(&i.attrs, sym::should_panic) {
        Some(attr) => {
            let ref sd = cx.parse_sess.span_diagnostic;

            match attr.meta_item_list() {
                // Handle #[should_panic(expected = "foo")]
                Some(list) => {
                    let msg = list.iter()
                        .find(|mi| mi.check_name(sym::expected))
                        .and_then(|mi| mi.meta_item())
                        .and_then(|mi| mi.value_str());
                    if list.len() != 1 || msg.is_none() {
                        sd.struct_span_warn(
                            attr.span,
                            "argument must be of the form: \
                             `expected = \"error message\"`"
                        ).note("Errors in this attribute were erroneously \
                                allowed and will become a hard error in a \
                                future release.").emit();
                        ShouldPanic::Yes(None)
                    } else {
                        ShouldPanic::Yes(msg)
                    }
                },
                // Handle #[should_panic] and #[should_panic = "expected"]
                None => ShouldPanic::Yes(attr.value_str())
            }
        }
        None => ShouldPanic::No,
    }
}

enum TestType {
    UnitTest,
    IntegrationTest,
    Unknown,
}

/// Attempts to determine the type of test.
/// Since doctests are created without macro expanding, only possible variants here
/// are `UnitTest`, `IntegrationTest` or `Unknown`.
fn test_type(cx: &ExtCtxt<'_>) -> TestType {
    // Root path from context contains the topmost sources directory of the crate.
    // I.e., for `project` with sources in `src` and tests in `tests` folders
    // (no matter how many nested folders lie inside),
    // there will be two different root paths: `/project/src` and `/project/tests`.
    let crate_path = cx.root_path.as_path();

    if crate_path.ends_with("src") {
        // `/src` folder contains unit-tests.
        TestType::UnitTest
    } else if crate_path.ends_with("tests") {
        // `/tests` folder contains integration tests.
        TestType::IntegrationTest
    } else {
        // Crate layout doesn't match expected one, test type is unknown.
        TestType::Unknown
    }
}

fn has_test_signature(cx: &ExtCtxt<'_>, i: &ast::Item) -> bool {
    let has_should_panic_attr = attr::contains_name(&i.attrs, sym::should_panic);
    let ref sd = cx.parse_sess.span_diagnostic;
    if let ast::ItemKind::Fn(ref sig, ref generics, _) = i.kind {
        if sig.header.unsafety == ast::Unsafety::Unsafe {
            sd.span_err(
                i.span,
                "unsafe functions cannot be used for tests"
            );
            return false
        }
        if sig.header.asyncness.node.is_async() {
            sd.span_err(
                i.span,
                "async functions cannot be used for tests"
            );
            return false
        }


        // If the termination trait is active, the compiler will check that the output
        // type implements the `Termination` trait as `libtest` enforces that.
        let has_output = match sig.decl.output {
            ast::FunctionRetTy::Default(..) => false,
            ast::FunctionRetTy::Ty(ref t) if t.kind.is_unit() => false,
            _ => true
        };

        if !sig.decl.inputs.is_empty() {
            sd.span_err(i.span, "functions used as tests can not have any arguments");
            return false;
        }

        match (has_output, has_should_panic_attr) {
            (true, true) => {
                sd.span_err(i.span, "functions using `#[should_panic]` must return `()`");
                false
            },
            (true, false) => if !generics.params.is_empty() {
                sd.span_err(i.span,
                                "functions used as tests must have signature fn() -> ()");
                false
            } else {
                true
            },
            (false, _) => true
        }
    } else {
        sd.span_err(i.span, "only functions may be used as tests");
        false
    }
}

fn has_bench_signature(cx: &ExtCtxt<'_>, i: &ast::Item) -> bool {
    let has_sig = if let ast::ItemKind::Fn(ref sig, _, _) = i.kind {
        // N.B., inadequate check, but we're running
        // well before resolve, can't get too deep.
        sig.decl.inputs.len() == 1
    } else {
        false
    };

    if !has_sig {
        cx.parse_sess.span_diagnostic.span_err(i.span, "functions used as benches must have \
            signature `fn(&mut Bencher) -> impl Termination`");
    }

    has_sig
}
