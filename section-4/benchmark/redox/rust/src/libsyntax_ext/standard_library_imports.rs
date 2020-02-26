use syntax::{ast, attr};
use syntax::edition::Edition;
use syntax::ptr::P;
use syntax::sess::ParseSess;
use syntax::symbol::{Ident, Symbol, kw, sym};
use syntax_expand::expand::ExpansionConfig;
use syntax_expand::base::{ExtCtxt, Resolver};
use syntax_pos::DUMMY_SP;
use syntax_pos::hygiene::AstPass;

pub fn inject(
    mut krate: ast::Crate,
    resolver: &mut dyn Resolver,
    sess: &ParseSess,
    alt_std_name: Option<Symbol>,
) -> (ast::Crate, Option<Symbol>) {
    let rust_2018 = sess.edition >= Edition::Edition2018;

    // the first name in this list is the crate name of the crate with the prelude
    let names: &[Symbol] = if attr::contains_name(&krate.attrs, sym::no_core) {
        return (krate, None);
    } else if attr::contains_name(&krate.attrs, sym::no_std) {
        if attr::contains_name(&krate.attrs, sym::compiler_builtins) {
            &[sym::core]
        } else {
            &[sym::core, sym::compiler_builtins]
        }
    } else {
        &[sym::std]
    };

    let expn_id = resolver.expansion_for_ast_pass(
        DUMMY_SP,
        AstPass::StdImports,
        &[sym::prelude_import],
        None,
    );
    let span = DUMMY_SP.with_def_site_ctxt(expn_id);
    let call_site = DUMMY_SP.with_call_site_ctxt(expn_id);

    let ecfg = ExpansionConfig::default("std_lib_injection".to_string());
    let cx = ExtCtxt::new(sess, ecfg, resolver);


    // .rev() to preserve ordering above in combination with insert(0, ...)
    for &name in names.iter().rev() {
        let ident = if rust_2018 {
            Ident::new(name, span)
        } else {
            Ident::new(name, call_site)
        };
        krate.module.items.insert(0, cx.item(
            span,
            ident,
            vec![cx.attribute(cx.meta_word(span, sym::macro_use))],
            ast::ItemKind::ExternCrate(alt_std_name),
        ));
    }

    // The crates have been injected, the assumption is that the first one is
    // the one with the prelude.
    let name = names[0];

    let import_path = if rust_2018 {
        [name, sym::prelude, sym::v1].iter()
            .map(|symbol| ast::Ident::new(*symbol, span)).collect()
    } else {
        [kw::PathRoot, name, sym::prelude, sym::v1].iter()
            .map(|symbol| ast::Ident::new(*symbol, span)).collect()
    };

    let use_item = cx.item(
        span,
        ast::Ident::invalid(),
        vec![cx.attribute(cx.meta_word(span, sym::prelude_import))],
        ast::ItemKind::Use(P(ast::UseTree {
            prefix: cx.path(span, import_path),
            kind: ast::UseTreeKind::Glob,
            span,
        })),
    );

    krate.module.items.insert(0, use_item);

    (krate, Some(name))
}
