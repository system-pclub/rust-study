use crate::base::{self, *};
use crate::proc_macro_server;

use syntax::ast::{self, ItemKind};
use syntax::errors::{Applicability, FatalError};
use syntax::symbol::sym;
use syntax::token;
use syntax::tokenstream::{self, TokenStream};

use rustc_data_structures::sync::Lrc;
use syntax_pos::{Span, DUMMY_SP};

const EXEC_STRATEGY: pm::bridge::server::SameThread = pm::bridge::server::SameThread;

pub struct BangProcMacro {
    pub client: pm::bridge::client::Client<
        fn(pm::TokenStream) -> pm::TokenStream,
    >,
}

impl base::ProcMacro for BangProcMacro {
    fn expand<'cx>(&self,
                   ecx: &'cx mut ExtCtxt<'_>,
                   span: Span,
                   input: TokenStream)
                   -> TokenStream {
        let server = proc_macro_server::Rustc::new(ecx);
        match self.client.run(&EXEC_STRATEGY, server, input) {
            Ok(stream) => stream,
            Err(e) => {
                let msg = "proc macro panicked";
                let mut err = ecx.struct_span_fatal(span, msg);
                if let Some(s) = e.as_str() {
                    err.help(&format!("message: {}", s));
                }

                err.emit();
                FatalError.raise();
            }
        }
    }
}

pub struct AttrProcMacro {
    pub client: pm::bridge::client::Client<fn(pm::TokenStream, pm::TokenStream) -> pm::TokenStream>,
}

impl base::AttrProcMacro for AttrProcMacro {
    fn expand<'cx>(&self,
                   ecx: &'cx mut ExtCtxt<'_>,
                   span: Span,
                   annotation: TokenStream,
                   annotated: TokenStream)
                   -> TokenStream {
        let server = proc_macro_server::Rustc::new(ecx);
        match self.client.run(&EXEC_STRATEGY, server, annotation, annotated) {
            Ok(stream) => stream,
            Err(e) => {
                let msg = "custom attribute panicked";
                let mut err = ecx.struct_span_fatal(span, msg);
                if let Some(s) = e.as_str() {
                    err.help(&format!("message: {}", s));
                }

                err.emit();
                FatalError.raise();
            }
        }
    }
}

pub struct ProcMacroDerive {
    pub client: pm::bridge::client::Client<fn(pm::TokenStream) -> pm::TokenStream>,
}

impl MultiItemModifier for ProcMacroDerive {
    fn expand(&self,
              ecx: &mut ExtCtxt<'_>,
              span: Span,
              _meta_item: &ast::MetaItem,
              item: Annotatable)
              -> Vec<Annotatable> {
        let item = match item {
            Annotatable::Arm(..) |
            Annotatable::Field(..) |
            Annotatable::FieldPat(..) |
            Annotatable::GenericParam(..) |
            Annotatable::Param(..) |
            Annotatable::StructField(..) |
            Annotatable::Variant(..)
                => panic!("unexpected annotatable"),
            Annotatable::Item(item) => item,
            Annotatable::ImplItem(_) |
            Annotatable::TraitItem(_) |
            Annotatable::ForeignItem(_) |
            Annotatable::Stmt(_) |
            Annotatable::Expr(_) => {
                ecx.span_err(span, "proc-macro derives may only be \
                                    applied to a struct, enum, or union");
                return Vec::new()
            }
        };
        match item.kind {
            ItemKind::Struct(..) |
            ItemKind::Enum(..) |
            ItemKind::Union(..) => {},
            _ => {
                ecx.span_err(span, "proc-macro derives may only be \
                                    applied to a struct, enum, or union");
                return Vec::new()
            }
        }

        let token = token::Interpolated(Lrc::new(token::NtItem(item)));
        let input = tokenstream::TokenTree::token(token, DUMMY_SP).into();

        let server = proc_macro_server::Rustc::new(ecx);
        let stream = match self.client.run(&EXEC_STRATEGY, server, input) {
            Ok(stream) => stream,
            Err(e) => {
                let msg = "proc-macro derive panicked";
                let mut err = ecx.struct_span_fatal(span, msg);
                if let Some(s) = e.as_str() {
                    err.help(&format!("message: {}", s));
                }

                err.emit();
                FatalError.raise();
            }
        };

        let error_count_before = ecx.parse_sess.span_diagnostic.err_count();
        let msg = "proc-macro derive produced unparseable tokens";

        let mut parser = rustc_parse::stream_to_parser(
            ecx.parse_sess,
            stream,
            Some("proc-macro derive"),
        );
        let mut items = vec![];

        loop {
            match parser.parse_item() {
                Ok(None) => break,
                Ok(Some(item)) => {
                    items.push(Annotatable::Item(item))
                }
                Err(mut err) => {
                    // FIXME: handle this better
                    err.cancel();
                    ecx.struct_span_fatal(span, msg).emit();
                    FatalError.raise();
                }
            }
        }


        // fail if there have been errors emitted
        if ecx.parse_sess.span_diagnostic.err_count() > error_count_before {
            ecx.struct_span_fatal(span, msg).emit();
            FatalError.raise();
        }

        items
    }
}

crate fn collect_derives(cx: &mut ExtCtxt<'_>, attrs: &mut Vec<ast::Attribute>) -> Vec<ast::Path> {
    let mut result = Vec::new();
    attrs.retain(|attr| {
        if !attr.has_name(sym::derive) {
            return true;
        }
        if !attr.is_meta_item_list() {
            cx.struct_span_err(attr.span, "malformed `derive` attribute input")
                .span_suggestion(
                    attr.span,
                    "missing traits to be derived",
                    "#[derive(Trait1, Trait2, ...)]".to_owned(),
                    Applicability::HasPlaceholders,
                ).emit();
            return false;
        }

        let parse_derive_paths = |attr: &ast::Attribute| {
            if attr.get_normal_item().tokens.is_empty() {
                return Ok(Vec::new());
            }
            rustc_parse::parse_in_attr(cx.parse_sess, attr, |p| p.parse_derive_paths())
        };

        match parse_derive_paths(attr) {
            Ok(traits) => {
                result.extend(traits);
                true
            }
            Err(mut e) => {
                e.emit();
                false
            }
        }
    });
    result
}
