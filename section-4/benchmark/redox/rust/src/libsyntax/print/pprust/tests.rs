use super::*;

use crate::ast;
use crate::source_map;
use crate::with_default_globals;
use syntax_pos;

fn fun_to_string(
    decl: &ast::FnDecl, header: ast::FnHeader, name: ast::Ident, generics: &ast::Generics
) -> String {
    to_string(|s| {
        s.head("");
        s.print_fn(decl, header, Some(name),
                   generics, &source_map::dummy_spanned(ast::VisibilityKind::Inherited));
        s.end(); // Close the head box.
        s.end(); // Close the outer box.
    })
}

fn variant_to_string(var: &ast::Variant) -> String {
    to_string(|s| s.print_variant(var))
}

#[test]
fn test_fun_to_string() {
    with_default_globals(|| {
        let abba_ident = ast::Ident::from_str("abba");

        let decl = ast::FnDecl {
            inputs: Vec::new(),
            output: ast::FunctionRetTy::Default(syntax_pos::DUMMY_SP),
        };
        let generics = ast::Generics::default();
        assert_eq!(
            fun_to_string(
                &decl,
                ast::FnHeader::default(),
                abba_ident,
                &generics
            ),
            "fn abba()"
        );
    })
}

#[test]
fn test_variant_to_string() {
    with_default_globals(|| {
        let ident = ast::Ident::from_str("principal_skinner");

        let var = ast::Variant {
            ident,
            vis: source_map::respan(syntax_pos::DUMMY_SP, ast::VisibilityKind::Inherited),
            attrs: Vec::new(),
            id: ast::DUMMY_NODE_ID,
            data: ast::VariantData::Unit(ast::DUMMY_NODE_ID),
            disr_expr: None,
            span: syntax_pos::DUMMY_SP,
            is_placeholder: false,
        };

        let varstr = variant_to_string(&var);
        assert_eq!(varstr, "principal_skinner");
    })
}
