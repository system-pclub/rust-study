use crate::base::ExtCtxt;

use syntax::ast::{self, Ident, Expr, BlockCheckMode, UnOp, PatKind};
use syntax::attr;
use syntax::source_map::{respan, Spanned};
use syntax::ptr::P;
use syntax::symbol::{kw, sym, Symbol};
use syntax::ThinVec;

use syntax_pos::{Pos, Span};

impl<'a> ExtCtxt<'a> {
    pub fn path(&self, span: Span, strs: Vec<ast::Ident> ) -> ast::Path {
        self.path_all(span, false, strs, vec![])
    }
    pub fn path_ident(&self, span: Span, id: ast::Ident) -> ast::Path {
        self.path(span, vec![id])
    }
    pub fn path_global(&self, span: Span, strs: Vec<ast::Ident> ) -> ast::Path {
        self.path_all(span, true, strs, vec![])
    }
    pub fn path_all(&self,
                span: Span,
                global: bool,
                mut idents: Vec<ast::Ident> ,
                args: Vec<ast::GenericArg>)
                -> ast::Path {
        assert!(!idents.is_empty());
        let add_root = global && !idents[0].is_path_segment_keyword();
        let mut segments = Vec::with_capacity(idents.len() + add_root as usize);
        if add_root {
            segments.push(ast::PathSegment::path_root(span));
        }
        let last_ident = idents.pop().unwrap();
        segments.extend(idents.into_iter().map(|ident| {
            ast::PathSegment::from_ident(ident.with_span_pos(span))
        }));
        let args = if !args.is_empty() {
            ast::AngleBracketedArgs { args, constraints: Vec::new(), span }.into()
        } else {
            None
        };
        segments.push(ast::PathSegment {
            ident: last_ident.with_span_pos(span),
            id: ast::DUMMY_NODE_ID,
            args,
        });
        ast::Path { span, segments }
    }

    pub fn ty_mt(&self, ty: P<ast::Ty>, mutbl: ast::Mutability) -> ast::MutTy {
        ast::MutTy {
            ty,
            mutbl,
        }
    }

    pub fn ty(&self, span: Span, kind: ast::TyKind) -> P<ast::Ty> {
        P(ast::Ty {
            id: ast::DUMMY_NODE_ID,
            span,
            kind,
        })
    }

    pub fn ty_path(&self, path: ast::Path) -> P<ast::Ty> {
        self.ty(path.span, ast::TyKind::Path(None, path))
    }

    // Might need to take bounds as an argument in the future, if you ever want
    // to generate a bounded existential trait type.
    pub fn ty_ident(&self, span: Span, ident: ast::Ident)
        -> P<ast::Ty> {
        self.ty_path(self.path_ident(span, ident))
    }

    pub fn anon_const(&self, span: Span, kind: ast::ExprKind) -> ast::AnonConst {
        ast::AnonConst {
            id: ast::DUMMY_NODE_ID,
            value: P(ast::Expr {
                id: ast::DUMMY_NODE_ID,
                kind,
                span,
                attrs: ThinVec::new(),
            })
        }
    }

    pub fn const_ident(&self, span: Span, ident: ast::Ident) -> ast::AnonConst {
        self.anon_const(span, ast::ExprKind::Path(None, self.path_ident(span, ident)))
    }

    pub fn ty_rptr(&self,
               span: Span,
               ty: P<ast::Ty>,
               lifetime: Option<ast::Lifetime>,
               mutbl: ast::Mutability)
        -> P<ast::Ty> {
        self.ty(span,
                ast::TyKind::Rptr(lifetime, self.ty_mt(ty, mutbl)))
    }

    pub fn ty_ptr(&self,
              span: Span,
              ty: P<ast::Ty>,
              mutbl: ast::Mutability)
        -> P<ast::Ty> {
        self.ty(span,
                ast::TyKind::Ptr(self.ty_mt(ty, mutbl)))
    }

    pub fn typaram(&self,
               span: Span,
               ident: ast::Ident,
               attrs: Vec<ast::Attribute>,
               bounds: ast::GenericBounds,
               default: Option<P<ast::Ty>>) -> ast::GenericParam {
        ast::GenericParam {
            ident: ident.with_span_pos(span),
            id: ast::DUMMY_NODE_ID,
            attrs: attrs.into(),
            bounds,
            kind: ast::GenericParamKind::Type {
                default,
            },
            is_placeholder: false
        }
    }

    pub fn trait_ref(&self, path: ast::Path) -> ast::TraitRef {
        ast::TraitRef {
            path,
            ref_id: ast::DUMMY_NODE_ID,
        }
    }

    pub fn poly_trait_ref(&self, span: Span, path: ast::Path) -> ast::PolyTraitRef {
        ast::PolyTraitRef {
            bound_generic_params: Vec::new(),
            trait_ref: self.trait_ref(path),
            span,
        }
    }

    pub fn trait_bound(&self, path: ast::Path) -> ast::GenericBound {
        ast::GenericBound::Trait(self.poly_trait_ref(path.span, path),
                                 ast::TraitBoundModifier::None)
    }

    pub fn lifetime(&self, span: Span, ident: ast::Ident) -> ast::Lifetime {
        ast::Lifetime { id: ast::DUMMY_NODE_ID, ident: ident.with_span_pos(span) }
    }

    pub fn lifetime_def(&self,
                    span: Span,
                    ident: ast::Ident,
                    attrs: Vec<ast::Attribute>,
                    bounds: ast::GenericBounds)
                    -> ast::GenericParam {
        let lifetime = self.lifetime(span, ident);
        ast::GenericParam {
            ident: lifetime.ident,
            id: lifetime.id,
            attrs: attrs.into(),
            bounds,
            kind: ast::GenericParamKind::Lifetime,
            is_placeholder: false
        }
    }

    pub fn stmt_expr(&self, expr: P<ast::Expr>) -> ast::Stmt {
        ast::Stmt {
            id: ast::DUMMY_NODE_ID,
            span: expr.span,
            kind: ast::StmtKind::Expr(expr),
        }
    }

    pub fn stmt_let(&self, sp: Span, mutbl: bool, ident: ast::Ident,
                ex: P<ast::Expr>) -> ast::Stmt {
        let pat = if mutbl {
            let binding_mode = ast::BindingMode::ByValue(ast::Mutability::Mutable);
            self.pat_ident_binding_mode(sp, ident, binding_mode)
        } else {
            self.pat_ident(sp, ident)
        };
        let local = P(ast::Local {
            pat,
            ty: None,
            init: Some(ex),
            id: ast::DUMMY_NODE_ID,
            span: sp,
            attrs: ThinVec::new(),
        });
        ast::Stmt {
            id: ast::DUMMY_NODE_ID,
            kind: ast::StmtKind::Local(local),
            span: sp,
        }
    }

    // Generates `let _: Type;`, which is usually used for type assertions.
    pub fn stmt_let_type_only(&self, span: Span, ty: P<ast::Ty>) -> ast::Stmt {
        let local = P(ast::Local {
            pat: self.pat_wild(span),
            ty: Some(ty),
            init: None,
            id: ast::DUMMY_NODE_ID,
            span,
            attrs: ThinVec::new(),
        });
        ast::Stmt {
            id: ast::DUMMY_NODE_ID,
            kind: ast::StmtKind::Local(local),
            span,
        }
    }

    pub fn stmt_item(&self, sp: Span, item: P<ast::Item>) -> ast::Stmt {
        ast::Stmt {
            id: ast::DUMMY_NODE_ID,
            kind: ast::StmtKind::Item(item),
            span: sp,
        }
    }

    pub fn block_expr(&self, expr: P<ast::Expr>) -> P<ast::Block> {
        self.block(expr.span, vec![ast::Stmt {
            id: ast::DUMMY_NODE_ID,
            span: expr.span,
            kind: ast::StmtKind::Expr(expr),
        }])
    }
    pub fn block(&self, span: Span, stmts: Vec<ast::Stmt>) -> P<ast::Block> {
        P(ast::Block {
           stmts,
           id: ast::DUMMY_NODE_ID,
           rules: BlockCheckMode::Default,
           span,
        })
    }

    pub fn expr(&self, span: Span, kind: ast::ExprKind) -> P<ast::Expr> {
        P(ast::Expr {
            id: ast::DUMMY_NODE_ID,
            kind,
            span,
            attrs: ThinVec::new(),
        })
    }

    pub fn expr_path(&self, path: ast::Path) -> P<ast::Expr> {
        self.expr(path.span, ast::ExprKind::Path(None, path))
    }

    pub fn expr_ident(&self, span: Span, id: ast::Ident) -> P<ast::Expr> {
        self.expr_path(self.path_ident(span, id))
    }
    pub fn expr_self(&self, span: Span) -> P<ast::Expr> {
        self.expr_ident(span, Ident::with_dummy_span(kw::SelfLower))
    }

    pub fn expr_binary(&self, sp: Span, op: ast::BinOpKind,
                   lhs: P<ast::Expr>, rhs: P<ast::Expr>) -> P<ast::Expr> {
        self.expr(sp, ast::ExprKind::Binary(Spanned { node: op, span: sp }, lhs, rhs))
    }

    pub fn expr_deref(&self, sp: Span, e: P<ast::Expr>) -> P<ast::Expr> {
        self.expr(sp, ast::ExprKind::Unary(UnOp::Deref, e))
    }

    pub fn expr_addr_of(&self, sp: Span, e: P<ast::Expr>) -> P<ast::Expr> {
        self.expr(sp, ast::ExprKind::AddrOf(ast::Mutability::Immutable, e))
    }

    pub fn expr_call(
        &self, span: Span, expr: P<ast::Expr>, args: Vec<P<ast::Expr>>,
    ) -> P<ast::Expr> {
        self.expr(span, ast::ExprKind::Call(expr, args))
    }
    pub fn expr_call_ident(&self, span: Span, id: ast::Ident,
                       args: Vec<P<ast::Expr>>) -> P<ast::Expr> {
        self.expr(span, ast::ExprKind::Call(self.expr_ident(span, id), args))
    }
    pub fn expr_call_global(&self, sp: Span, fn_path: Vec<ast::Ident> ,
                      args: Vec<P<ast::Expr>> ) -> P<ast::Expr> {
        let pathexpr = self.expr_path(self.path_global(sp, fn_path));
        self.expr_call(sp, pathexpr, args)
    }
    pub fn expr_method_call(&self, span: Span,
                        expr: P<ast::Expr>,
                        ident: ast::Ident,
                        mut args: Vec<P<ast::Expr>> ) -> P<ast::Expr> {
        args.insert(0, expr);
        let segment = ast::PathSegment::from_ident(ident.with_span_pos(span));
        self.expr(span, ast::ExprKind::MethodCall(segment, args))
    }
    pub fn expr_block(&self, b: P<ast::Block>) -> P<ast::Expr> {
        self.expr(b.span, ast::ExprKind::Block(b, None))
    }
    pub fn field_imm(&self, span: Span, ident: Ident, e: P<ast::Expr>) -> ast::Field {
        ast::Field {
            ident: ident.with_span_pos(span),
            expr: e,
            span,
            is_shorthand: false,
            attrs: ThinVec::new(),
            id: ast::DUMMY_NODE_ID,
            is_placeholder: false,
        }
    }
    pub fn expr_struct(
        &self, span: Span, path: ast::Path, fields: Vec<ast::Field>
    ) -> P<ast::Expr> {
        self.expr(span, ast::ExprKind::Struct(path, fields, None))
    }
    pub fn expr_struct_ident(&self, span: Span,
                         id: ast::Ident, fields: Vec<ast::Field>) -> P<ast::Expr> {
        self.expr_struct(span, self.path_ident(span, id), fields)
    }

    pub fn expr_lit(&self, span: Span, lit_kind: ast::LitKind) -> P<ast::Expr> {
        let lit = ast::Lit::from_lit_kind(lit_kind, span);
        self.expr(span, ast::ExprKind::Lit(lit))
    }
    pub fn expr_usize(&self, span: Span, i: usize) -> P<ast::Expr> {
        self.expr_lit(span, ast::LitKind::Int(i as u128,
                                              ast::LitIntType::Unsigned(ast::UintTy::Usize)))
    }
    pub fn expr_u32(&self, sp: Span, u: u32) -> P<ast::Expr> {
        self.expr_lit(sp, ast::LitKind::Int(u as u128,
                                            ast::LitIntType::Unsigned(ast::UintTy::U32)))
    }
    pub fn expr_bool(&self, sp: Span, value: bool) -> P<ast::Expr> {
        self.expr_lit(sp, ast::LitKind::Bool(value))
    }

    pub fn expr_vec(&self, sp: Span, exprs: Vec<P<ast::Expr>>) -> P<ast::Expr> {
        self.expr(sp, ast::ExprKind::Array(exprs))
    }
    pub fn expr_vec_slice(&self, sp: Span, exprs: Vec<P<ast::Expr>>) -> P<ast::Expr> {
        self.expr_addr_of(sp, self.expr_vec(sp, exprs))
    }
    pub fn expr_str(&self, sp: Span, s: Symbol) -> P<ast::Expr> {
        self.expr_lit(sp, ast::LitKind::Str(s, ast::StrStyle::Cooked))
    }

    pub fn expr_cast(&self, sp: Span, expr: P<ast::Expr>, ty: P<ast::Ty>) -> P<ast::Expr> {
        self.expr(sp, ast::ExprKind::Cast(expr, ty))
    }

    pub fn expr_some(&self, sp: Span, expr: P<ast::Expr>) -> P<ast::Expr> {
        let some = self.std_path(&[sym::option, sym::Option, sym::Some]);
        self.expr_call_global(sp, some, vec![expr])
    }

    pub fn expr_tuple(&self, sp: Span, exprs: Vec<P<ast::Expr>>) -> P<ast::Expr> {
        self.expr(sp, ast::ExprKind::Tup(exprs))
    }

    pub fn expr_fail(&self, span: Span, msg: Symbol) -> P<ast::Expr> {
        let loc = self.source_map().lookup_char_pos(span.lo());
        let expr_file = self.expr_str(span, Symbol::intern(&loc.file.name.to_string()));
        let expr_line = self.expr_u32(span, loc.line as u32);
        let expr_col = self.expr_u32(span, loc.col.to_usize() as u32 + 1);
        let expr_loc_tuple = self.expr_tuple(span, vec![expr_file, expr_line, expr_col]);
        let expr_loc_ptr = self.expr_addr_of(span, expr_loc_tuple);
        self.expr_call_global(
            span,
            [sym::std, sym::rt, sym::begin_panic].iter().map(|s| Ident::new(*s, span)).collect(),
            vec![
                self.expr_str(span, msg),
                expr_loc_ptr])
    }

    pub fn expr_unreachable(&self, span: Span) -> P<ast::Expr> {
        self.expr_fail(span, Symbol::intern("internal error: entered unreachable code"))
    }

    pub fn expr_ok(&self, sp: Span, expr: P<ast::Expr>) -> P<ast::Expr> {
        let ok = self.std_path(&[sym::result, sym::Result, sym::Ok]);
        self.expr_call_global(sp, ok, vec![expr])
    }

    pub fn expr_try(&self, sp: Span, head: P<ast::Expr>) -> P<ast::Expr> {
        let ok = self.std_path(&[sym::result, sym::Result, sym::Ok]);
        let ok_path = self.path_global(sp, ok);
        let err = self.std_path(&[sym::result, sym::Result, sym::Err]);
        let err_path = self.path_global(sp, err);

        let binding_variable = self.ident_of("__try_var", sp);
        let binding_pat = self.pat_ident(sp, binding_variable);
        let binding_expr = self.expr_ident(sp, binding_variable);

        // `Ok(__try_var)` pattern
        let ok_pat = self.pat_tuple_struct(sp, ok_path, vec![binding_pat.clone()]);

        // `Err(__try_var)` (pattern and expression respectively)
        let err_pat = self.pat_tuple_struct(sp, err_path.clone(), vec![binding_pat]);
        let err_inner_expr = self.expr_call(sp, self.expr_path(err_path),
                                            vec![binding_expr.clone()]);
        // `return Err(__try_var)`
        let err_expr = self.expr(sp, ast::ExprKind::Ret(Some(err_inner_expr)));

        // `Ok(__try_var) => __try_var`
        let ok_arm = self.arm(sp, ok_pat, binding_expr);
        // `Err(__try_var) => return Err(__try_var)`
        let err_arm = self.arm(sp, err_pat, err_expr);

        // `match head { Ok() => ..., Err() => ... }`
        self.expr_match(sp, head, vec![ok_arm, err_arm])
    }


    pub fn pat(&self, span: Span, kind: PatKind) -> P<ast::Pat> {
        P(ast::Pat { id: ast::DUMMY_NODE_ID, kind, span })
    }
    pub fn pat_wild(&self, span: Span) -> P<ast::Pat> {
        self.pat(span, PatKind::Wild)
    }
    pub fn pat_lit(&self, span: Span, expr: P<ast::Expr>) -> P<ast::Pat> {
        self.pat(span, PatKind::Lit(expr))
    }
    pub fn pat_ident(&self, span: Span, ident: ast::Ident) -> P<ast::Pat> {
        let binding_mode = ast::BindingMode::ByValue(ast::Mutability::Immutable);
        self.pat_ident_binding_mode(span, ident, binding_mode)
    }

    pub fn pat_ident_binding_mode(&self,
                              span: Span,
                              ident: ast::Ident,
                              bm: ast::BindingMode) -> P<ast::Pat> {
        let pat = PatKind::Ident(bm, ident.with_span_pos(span), None);
        self.pat(span, pat)
    }
    pub fn pat_path(&self, span: Span, path: ast::Path) -> P<ast::Pat> {
        self.pat(span, PatKind::Path(None, path))
    }
    pub fn pat_tuple_struct(&self, span: Span, path: ast::Path,
                        subpats: Vec<P<ast::Pat>>) -> P<ast::Pat> {
        self.pat(span, PatKind::TupleStruct(path, subpats))
    }
    pub fn pat_struct(&self, span: Span, path: ast::Path,
                      field_pats: Vec<ast::FieldPat>) -> P<ast::Pat> {
        self.pat(span, PatKind::Struct(path, field_pats, false))
    }
    pub fn pat_tuple(&self, span: Span, pats: Vec<P<ast::Pat>>) -> P<ast::Pat> {
        self.pat(span, PatKind::Tuple(pats))
    }

    pub fn pat_some(&self, span: Span, pat: P<ast::Pat>) -> P<ast::Pat> {
        let some = self.std_path(&[sym::option, sym::Option, sym::Some]);
        let path = self.path_global(span, some);
        self.pat_tuple_struct(span, path, vec![pat])
    }

    pub fn pat_none(&self, span: Span) -> P<ast::Pat> {
        let some = self.std_path(&[sym::option, sym::Option, sym::None]);
        let path = self.path_global(span, some);
        self.pat_path(span, path)
    }

    pub fn pat_ok(&self, span: Span, pat: P<ast::Pat>) -> P<ast::Pat> {
        let some = self.std_path(&[sym::result, sym::Result, sym::Ok]);
        let path = self.path_global(span, some);
        self.pat_tuple_struct(span, path, vec![pat])
    }

    pub fn pat_err(&self, span: Span, pat: P<ast::Pat>) -> P<ast::Pat> {
        let some = self.std_path(&[sym::result, sym::Result, sym::Err]);
        let path = self.path_global(span, some);
        self.pat_tuple_struct(span, path, vec![pat])
    }

    pub fn arm(&self, span: Span, pat: P<ast::Pat>, expr: P<ast::Expr>) -> ast::Arm {
        ast::Arm {
            attrs: vec![],
            pat,
            guard: None,
            body: expr,
            span,
            id: ast::DUMMY_NODE_ID,
            is_placeholder: false,
        }
    }

    pub fn arm_unreachable(&self, span: Span) -> ast::Arm {
        self.arm(span, self.pat_wild(span), self.expr_unreachable(span))
    }

    pub fn expr_match(&self, span: Span, arg: P<ast::Expr>, arms: Vec<ast::Arm>) -> P<Expr> {
        self.expr(span, ast::ExprKind::Match(arg, arms))
    }

    pub fn expr_if(&self, span: Span, cond: P<ast::Expr>,
               then: P<ast::Expr>, els: Option<P<ast::Expr>>) -> P<ast::Expr> {
        let els = els.map(|x| self.expr_block(self.block_expr(x)));
        self.expr(span, ast::ExprKind::If(cond, self.block_expr(then), els))
    }

    pub fn lambda_fn_decl(&self,
                      span: Span,
                      fn_decl: P<ast::FnDecl>,
                      body: P<ast::Expr>,
                      fn_decl_span: Span) // span of the `|...|` part
                      -> P<ast::Expr> {
        self.expr(span, ast::ExprKind::Closure(ast::CaptureBy::Ref,
                                               ast::IsAsync::NotAsync,
                                               ast::Movability::Movable,
                                               fn_decl,
                                               body,
                                               fn_decl_span))
    }

    pub fn lambda(&self,
              span: Span,
              ids: Vec<ast::Ident>,
              body: P<ast::Expr>)
              -> P<ast::Expr> {
        let fn_decl = self.fn_decl(
            ids.iter().map(|id| self.param(span, *id, self.ty(span, ast::TyKind::Infer))).collect(),
            ast::FunctionRetTy::Default(span));

        // FIXME -- We are using `span` as the span of the `|...|`
        // part of the lambda, but it probably (maybe?) corresponds to
        // the entire lambda body. Probably we should extend the API
        // here, but that's not entirely clear.
        self.expr(span, ast::ExprKind::Closure(ast::CaptureBy::Ref,
                                               ast::IsAsync::NotAsync,
                                               ast::Movability::Movable,
                                               fn_decl,
                                               body,
                                               span))
    }

    pub fn lambda0(&self, span: Span, body: P<ast::Expr>) -> P<ast::Expr> {
        self.lambda(span, Vec::new(), body)
    }

    pub fn lambda1(&self, span: Span, body: P<ast::Expr>, ident: ast::Ident) -> P<ast::Expr> {
        self.lambda(span, vec![ident], body)
    }

    pub fn lambda_stmts_1(&self, span: Span, stmts: Vec<ast::Stmt>,
                      ident: ast::Ident) -> P<ast::Expr> {
        self.lambda1(span, self.expr_block(self.block(span, stmts)), ident)
    }

    pub fn param(&self, span: Span, ident: ast::Ident, ty: P<ast::Ty>) -> ast::Param {
        let arg_pat = self.pat_ident(span, ident);
        ast::Param {
            attrs: ThinVec::default(),
            id: ast::DUMMY_NODE_ID,
            pat: arg_pat,
            span,
            ty,
            is_placeholder: false,
        }
    }

    // FIXME: unused `self`
    pub fn fn_decl(&self, inputs: Vec<ast::Param>, output: ast::FunctionRetTy) -> P<ast::FnDecl> {
        P(ast::FnDecl {
            inputs,
            output,
        })
    }

    pub fn item(&self, span: Span, name: Ident,
            attrs: Vec<ast::Attribute>, kind: ast::ItemKind) -> P<ast::Item> {
        // FIXME: Would be nice if our generated code didn't violate
        // Rust coding conventions
        P(ast::Item {
            ident: name,
            attrs,
            id: ast::DUMMY_NODE_ID,
            kind,
            vis: respan(span.shrink_to_lo(), ast::VisibilityKind::Inherited),
            span,
            tokens: None,
        })
    }

    pub fn variant(&self, span: Span, ident: Ident, tys: Vec<P<ast::Ty>> ) -> ast::Variant {
        let vis_span = span.shrink_to_lo();
        let fields: Vec<_> = tys.into_iter().map(|ty| {
            ast::StructField {
                span: ty.span,
                ty,
                ident: None,
                vis: respan(vis_span, ast::VisibilityKind::Inherited),
                attrs: Vec::new(),
                id: ast::DUMMY_NODE_ID,
                is_placeholder: false,
            }
        }).collect();

        let vdata = if fields.is_empty() {
            ast::VariantData::Unit(ast::DUMMY_NODE_ID)
        } else {
            ast::VariantData::Tuple(fields, ast::DUMMY_NODE_ID)
        };

        ast::Variant {
            attrs: Vec::new(),
            data: vdata,
            disr_expr: None,
            id: ast::DUMMY_NODE_ID,
            ident,
            vis: respan(vis_span, ast::VisibilityKind::Inherited),
            span,
            is_placeholder: false,
        }
    }

    pub fn item_static(&self,
                   span: Span,
                   name: Ident,
                   ty: P<ast::Ty>,
                   mutbl: ast::Mutability,
                   expr: P<ast::Expr>)
                   -> P<ast::Item> {
        self.item(span, name, Vec::new(), ast::ItemKind::Static(ty, mutbl, expr))
    }

    pub fn item_const(&self,
                  span: Span,
                  name: Ident,
                  ty: P<ast::Ty>,
                  expr: P<ast::Expr>)
                  -> P<ast::Item> {
        self.item(span, name, Vec::new(), ast::ItemKind::Const(ty, expr))
    }

    pub fn attribute(&self, mi: ast::MetaItem) -> ast::Attribute {
        attr::mk_attr_outer(mi)
    }

    pub fn meta_word(&self, sp: Span, w: ast::Name) -> ast::MetaItem {
        attr::mk_word_item(Ident::new(w, sp))
    }
}
