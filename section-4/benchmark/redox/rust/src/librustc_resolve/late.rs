//! "Late resolution" is the pass that resolves most of names in a crate beside imports and macros.
//! It runs when the crate is fully expanded and its module structure is fully built.
//! So it just walks through the crate and resolves all the expressions, types, etc.
//!
//! If you wonder why there's no `early.rs`, that's because it's split into three files -
//! `build_reduced_graph.rs`, `macros.rs` and `resolve_imports.rs`.

use RibKind::*;

use crate::{path_names_to_string, BindingError, CrateLint, LexicalScopeBinding};
use crate::{Module, ModuleOrUniformRoot, NameBindingKind, ParentScope, PathResult};
use crate::{ResolutionError, Resolver, Segment, UseError};

use log::debug;
use rustc::{bug, lint, span_bug};
use rustc::hir::def::{self, PartialRes, DefKind, CtorKind, PerNS};
use rustc::hir::def::Namespace::{self, *};
use rustc::hir::def_id::{DefId, CRATE_DEF_INDEX};
use rustc::hir::TraitCandidate;
use rustc::util::nodemap::{FxHashMap, FxHashSet};
use smallvec::{smallvec, SmallVec};
use syntax::{unwrap_or, walk_list};
use syntax::ast::*;
use syntax::ptr::P;
use syntax::symbol::{kw, sym};
use syntax::util::lev_distance::find_best_match_for_name;
use syntax::visit::{self, Visitor, FnKind};
use syntax_pos::Span;

use std::collections::BTreeSet;
use std::mem::replace;

use rustc_error_codes::*;

mod diagnostics;

type Res = def::Res<NodeId>;

type IdentMap<T> = FxHashMap<Ident, T>;

/// Map from the name in a pattern to its binding mode.
type BindingMap = IdentMap<BindingInfo>;

#[derive(Copy, Clone, Debug)]
struct BindingInfo {
    span: Span,
    binding_mode: BindingMode,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum PatternSource {
    Match,
    Let,
    For,
    FnParam,
}

impl PatternSource {
    fn descr(self) -> &'static str {
        match self {
            PatternSource::Match => "match binding",
            PatternSource::Let => "let binding",
            PatternSource::For => "for binding",
            PatternSource::FnParam => "function parameter",
        }
    }
}

/// Denotes whether the context for the set of already bound bindings is a `Product`
/// or `Or` context. This is used in e.g., `fresh_binding` and `resolve_pattern_inner`.
/// See those functions for more information.
#[derive(PartialEq)]
enum PatBoundCtx {
    /// A product pattern context, e.g., `Variant(a, b)`.
    Product,
    /// An or-pattern context, e.g., `p_0 | ... | p_n`.
    Or,
}

/// Does this the item (from the item rib scope) allow generic parameters?
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
crate enum HasGenericParams { Yes, No }

/// The rib kind restricts certain accesses,
/// e.g. to a `Res::Local` of an outer item.
#[derive(Copy, Clone, Debug)]
crate enum RibKind<'a> {
    /// No restriction needs to be applied.
    NormalRibKind,

    /// We passed through an impl or trait and are now in one of its
    /// methods or associated types. Allow references to ty params that impl or trait
    /// binds. Disallow any other upvars (including other ty params that are
    /// upvars).
    AssocItemRibKind,

    /// We passed through a function definition. Disallow upvars.
    /// Permit only those const parameters that are specified in the function's generics.
    FnItemRibKind,

    /// We passed through an item scope. Disallow upvars.
    ItemRibKind(HasGenericParams),

    /// We're in a constant item. Can't refer to dynamic stuff.
    ConstantItemRibKind,

    /// We passed through a module.
    ModuleRibKind(Module<'a>),

    /// We passed through a `macro_rules!` statement
    MacroDefinition(DefId),

    /// All bindings in this rib are type parameters that can't be used
    /// from the default of a type parameter because they're not declared
    /// before said type parameter. Also see the `visit_generics` override.
    ForwardTyParamBanRibKind,
}

impl RibKind<'_> {
    // Whether this rib kind contains generic parameters, as opposed to local
    // variables.
    crate fn contains_params(&self) -> bool {
        match self {
            NormalRibKind
            | FnItemRibKind
            | ConstantItemRibKind
            | ModuleRibKind(_)
            | MacroDefinition(_) => false,
            AssocItemRibKind
            | ItemRibKind(_)
            | ForwardTyParamBanRibKind => true,
        }
    }
}

/// A single local scope.
///
/// A rib represents a scope names can live in. Note that these appear in many places, not just
/// around braces. At any place where the list of accessible names (of the given namespace)
/// changes or a new restrictions on the name accessibility are introduced, a new rib is put onto a
/// stack. This may be, for example, a `let` statement (because it introduces variables), a macro,
/// etc.
///
/// Different [rib kinds](enum.RibKind) are transparent for different names.
///
/// The resolution keeps a separate stack of ribs as it traverses the AST for each namespace. When
/// resolving, the name is looked up from inside out.
#[derive(Debug)]
crate struct Rib<'a, R = Res> {
    pub bindings: IdentMap<R>,
    pub kind: RibKind<'a>,
}

impl<'a, R> Rib<'a, R> {
    fn new(kind: RibKind<'a>) -> Rib<'a, R> {
        Rib {
            bindings: Default::default(),
            kind,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
crate enum AliasPossibility {
    No,
    Maybe,
}

#[derive(Copy, Clone, Debug)]
crate enum PathSource<'a> {
    // Type paths `Path`.
    Type,
    // Trait paths in bounds or impls.
    Trait(AliasPossibility),
    // Expression paths `path`, with optional parent context.
    Expr(Option<&'a Expr>),
    // Paths in path patterns `Path`.
    Pat,
    // Paths in struct expressions and patterns `Path { .. }`.
    Struct,
    // Paths in tuple struct patterns `Path(..)`.
    TupleStruct,
    // `m::A::B` in `<T as m::A>::B::C`.
    TraitItem(Namespace),
}

impl<'a> PathSource<'a> {
    fn namespace(self) -> Namespace {
        match self {
            PathSource::Type | PathSource::Trait(_) | PathSource::Struct => TypeNS,
            PathSource::Expr(..) | PathSource::Pat | PathSource::TupleStruct => ValueNS,
            PathSource::TraitItem(ns) => ns,
        }
    }

    fn defer_to_typeck(self) -> bool {
        match self {
            PathSource::Type | PathSource::Expr(..) | PathSource::Pat |
            PathSource::Struct | PathSource::TupleStruct => true,
            PathSource::Trait(_) | PathSource::TraitItem(..) => false,
        }
    }

    fn descr_expected(self) -> &'static str {
        match &self {
            PathSource::Type => "type",
            PathSource::Trait(_) => "trait",
            PathSource::Pat => "unit struct, unit variant or constant",
            PathSource::Struct => "struct, variant or union type",
            PathSource::TupleStruct => "tuple struct or tuple variant",
            PathSource::TraitItem(ns) => match ns {
                TypeNS => "associated type",
                ValueNS => "method or associated constant",
                MacroNS => bug!("associated macro"),
            },
            PathSource::Expr(parent) => match &parent.as_ref().map(|p| &p.kind) {
                // "function" here means "anything callable" rather than `DefKind::Fn`,
                // this is not precise but usually more helpful than just "value".
                Some(ExprKind::Call(call_expr, _)) => {
                    match &call_expr.kind {
                        ExprKind::Path(_, path) => {
                            let mut msg = "function";
                            if let Some(segment) = path.segments.iter().last() {
                                if let Some(c) = segment.ident.to_string().chars().next() {
                                    if c.is_uppercase() {
                                        msg = "function, tuple struct or tuple variant";
                                    }
                                }
                            }
                            msg
                        }
                        _ => "function"
                    }
                }
                _ => "value",
            },
        }
    }

    crate fn is_expected(self, res: Res) -> bool {
        match self {
            PathSource::Type => match res {
                Res::Def(DefKind::Struct, _)
                | Res::Def(DefKind::Union, _)
                | Res::Def(DefKind::Enum, _)
                | Res::Def(DefKind::Trait, _)
                | Res::Def(DefKind::TraitAlias, _)
                | Res::Def(DefKind::TyAlias, _)
                | Res::Def(DefKind::AssocTy, _)
                | Res::PrimTy(..)
                | Res::Def(DefKind::TyParam, _)
                | Res::SelfTy(..)
                | Res::Def(DefKind::OpaqueTy, _)
                | Res::Def(DefKind::ForeignTy, _) => true,
                _ => false,
            },
            PathSource::Trait(AliasPossibility::No) => match res {
                Res::Def(DefKind::Trait, _) => true,
                _ => false,
            },
            PathSource::Trait(AliasPossibility::Maybe) => match res {
                Res::Def(DefKind::Trait, _) => true,
                Res::Def(DefKind::TraitAlias, _) => true,
                _ => false,
            },
            PathSource::Expr(..) => match res {
                Res::Def(DefKind::Ctor(_, CtorKind::Const), _)
                | Res::Def(DefKind::Ctor(_, CtorKind::Fn), _)
                | Res::Def(DefKind::Const, _)
                | Res::Def(DefKind::Static, _)
                | Res::Local(..)
                | Res::Def(DefKind::Fn, _)
                | Res::Def(DefKind::Method, _)
                | Res::Def(DefKind::AssocConst, _)
                | Res::SelfCtor(..)
                | Res::Def(DefKind::ConstParam, _) => true,
                _ => false,
            },
            PathSource::Pat => match res {
                Res::Def(DefKind::Ctor(_, CtorKind::Const), _) |
                Res::Def(DefKind::Const, _) | Res::Def(DefKind::AssocConst, _) |
                Res::SelfCtor(..) => true,
                _ => false,
            },
            PathSource::TupleStruct => match res {
                Res::Def(DefKind::Ctor(_, CtorKind::Fn), _) | Res::SelfCtor(..) => true,
                _ => false,
            },
            PathSource::Struct => match res {
                Res::Def(DefKind::Struct, _)
                | Res::Def(DefKind::Union, _)
                | Res::Def(DefKind::Variant, _)
                | Res::Def(DefKind::TyAlias, _)
                | Res::Def(DefKind::AssocTy, _)
                | Res::SelfTy(..) => true,
                _ => false,
            },
            PathSource::TraitItem(ns) => match res {
                Res::Def(DefKind::AssocConst, _)
                | Res::Def(DefKind::Method, _) if ns == ValueNS => true,
                Res::Def(DefKind::AssocTy, _) if ns == TypeNS => true,
                _ => false,
            },
        }
    }

    fn error_code(self, has_unexpected_resolution: bool) -> &'static str {
        syntax::diagnostic_used!(E0404);
        syntax::diagnostic_used!(E0405);
        syntax::diagnostic_used!(E0412);
        syntax::diagnostic_used!(E0422);
        syntax::diagnostic_used!(E0423);
        syntax::diagnostic_used!(E0425);
        syntax::diagnostic_used!(E0531);
        syntax::diagnostic_used!(E0532);
        syntax::diagnostic_used!(E0573);
        syntax::diagnostic_used!(E0574);
        syntax::diagnostic_used!(E0575);
        syntax::diagnostic_used!(E0576);
        match (self, has_unexpected_resolution) {
            (PathSource::Trait(_), true) => "E0404",
            (PathSource::Trait(_), false) => "E0405",
            (PathSource::Type, true) => "E0573",
            (PathSource::Type, false) => "E0412",
            (PathSource::Struct, true) => "E0574",
            (PathSource::Struct, false) => "E0422",
            (PathSource::Expr(..), true) => "E0423",
            (PathSource::Expr(..), false) => "E0425",
            (PathSource::Pat, true) | (PathSource::TupleStruct, true) => "E0532",
            (PathSource::Pat, false) | (PathSource::TupleStruct, false) => "E0531",
            (PathSource::TraitItem(..), true) => "E0575",
            (PathSource::TraitItem(..), false) => "E0576",
        }
    }
}

#[derive(Default)]
struct DiagnosticMetadata {
    /// The current trait's associated types' ident, used for diagnostic suggestions.
    current_trait_assoc_types: Vec<Ident>,

    /// The current self type if inside an impl (used for better errors).
    current_self_type: Option<Ty>,

    /// The current self item if inside an ADT (used for better errors).
    current_self_item: Option<NodeId>,

    /// The current enclosing funciton (used for better errors).
    current_function: Option<Span>,

    /// A list of labels as of yet unused. Labels will be removed from this map when
    /// they are used (in a `break` or `continue` statement)
    unused_labels: FxHashMap<NodeId, Span>,

    /// Only used for better errors on `fn(): fn()`.
    current_type_ascription: Vec<Span>,

    /// Only used for better errors on `let <pat>: <expr, not type>;`.
    current_let_binding: Option<(Span, Option<Span>, Option<Span>)>,
}

struct LateResolutionVisitor<'a, 'b> {
    r: &'b mut Resolver<'a>,

    /// The module that represents the current item scope.
    parent_scope: ParentScope<'a>,

    /// The current set of local scopes for types and values.
    /// FIXME #4948: Reuse ribs to avoid allocation.
    ribs: PerNS<Vec<Rib<'a>>>,

    /// The current set of local scopes, for labels.
    label_ribs: Vec<Rib<'a, NodeId>>,

    /// The trait that the current context can refer to.
    current_trait_ref: Option<(Module<'a>, TraitRef)>,

    /// Fields used to add information to diagnostic errors.
    diagnostic_metadata: DiagnosticMetadata,
}

/// Walks the whole crate in DFS order, visiting each item, resolving names as it goes.
impl<'a, 'tcx> Visitor<'tcx> for LateResolutionVisitor<'a, '_> {
    fn visit_item(&mut self, item: &'tcx Item) {
        self.resolve_item(item);
    }
    fn visit_arm(&mut self, arm: &'tcx Arm) {
        self.resolve_arm(arm);
    }
    fn visit_block(&mut self, block: &'tcx Block) {
        self.resolve_block(block);
    }
    fn visit_anon_const(&mut self, constant: &'tcx AnonConst) {
        debug!("visit_anon_const {:?}", constant);
        self.with_constant_rib(|this| {
            visit::walk_anon_const(this, constant);
        });
    }
    fn visit_expr(&mut self, expr: &'tcx Expr) {
        self.resolve_expr(expr, None);
    }
    fn visit_local(&mut self, local: &'tcx Local) {
        let local_spans = match local.pat.kind {
            // We check for this to avoid tuple struct fields.
            PatKind::Wild => None,
            _ => Some((
                local.pat.span,
                local.ty.as_ref().map(|ty| ty.span),
                local.init.as_ref().map(|init| init.span),
            )),
        };
        let original = replace(&mut self.diagnostic_metadata.current_let_binding, local_spans);
        self.resolve_local(local);
        self.diagnostic_metadata.current_let_binding = original;
    }
    fn visit_ty(&mut self, ty: &'tcx Ty) {
        match ty.kind {
            TyKind::Path(ref qself, ref path) => {
                self.smart_resolve_path(ty.id, qself.as_ref(), path, PathSource::Type);
            }
            TyKind::ImplicitSelf => {
                let self_ty = Ident::with_dummy_span(kw::SelfUpper);
                let res = self.resolve_ident_in_lexical_scope(self_ty, TypeNS, Some(ty.id), ty.span)
                              .map_or(Res::Err, |d| d.res());
                self.r.record_partial_res(ty.id, PartialRes::new(res));
            }
            _ => (),
        }
        visit::walk_ty(self, ty);
    }
    fn visit_poly_trait_ref(&mut self,
                            tref: &'tcx PolyTraitRef,
                            m: &'tcx TraitBoundModifier) {
        self.smart_resolve_path(tref.trait_ref.ref_id, None,
                                &tref.trait_ref.path, PathSource::Trait(AliasPossibility::Maybe));
        visit::walk_poly_trait_ref(self, tref, m);
    }
    fn visit_foreign_item(&mut self, foreign_item: &'tcx ForeignItem) {
        match foreign_item.kind {
            ForeignItemKind::Fn(_, ref generics) => {
                self.with_generic_param_rib(generics, ItemRibKind(HasGenericParams::Yes), |this| {
                    visit::walk_foreign_item(this, foreign_item);
                });
            }
            ForeignItemKind::Static(..) => {
                self.with_item_rib(HasGenericParams::No, |this| {
                    visit::walk_foreign_item(this, foreign_item);
                });
            }
            ForeignItemKind::Ty | ForeignItemKind::Macro(..) => {
                visit::walk_foreign_item(self, foreign_item);
            }
        }
    }
    fn visit_fn(&mut self, fn_kind: FnKind<'tcx>, declaration: &'tcx FnDecl, sp: Span, _: NodeId) {
        let previous_value = replace(&mut self.diagnostic_metadata.current_function, Some(sp));
        debug!("(resolving function) entering function");
        let rib_kind = match fn_kind {
            FnKind::ItemFn(..) => FnItemRibKind,
            FnKind::Method(..) | FnKind::Closure(_) => NormalRibKind,
        };

        // Create a value rib for the function.
        self.with_rib(ValueNS, rib_kind, |this| {
            // Create a label rib for the function.
            this.with_label_rib(rib_kind, |this| {
                // Add each argument to the rib.
                this.resolve_params(&declaration.inputs);

                visit::walk_fn_ret_ty(this, &declaration.output);

                // Resolve the function body, potentially inside the body of an async closure
                match fn_kind {
                    FnKind::ItemFn(.., body) |
                    FnKind::Method(.., body) => this.visit_block(body),
                    FnKind::Closure(body) => this.visit_expr(body),
                };

                debug!("(resolving function) leaving function");
            })
        });
        self.diagnostic_metadata.current_function = previous_value;
    }

    fn visit_generics(&mut self, generics: &'tcx Generics) {
        // For type parameter defaults, we have to ban access
        // to following type parameters, as the InternalSubsts can only
        // provide previous type parameters as they're built. We
        // put all the parameters on the ban list and then remove
        // them one by one as they are processed and become available.
        let mut default_ban_rib = Rib::new(ForwardTyParamBanRibKind);
        let mut found_default = false;
        default_ban_rib.bindings.extend(generics.params.iter()
            .filter_map(|param| match param.kind {
                GenericParamKind::Const { .. } |
                GenericParamKind::Lifetime { .. } => None,
                GenericParamKind::Type { ref default, .. } => {
                    found_default |= default.is_some();
                    if found_default {
                        Some((Ident::with_dummy_span(param.ident.name), Res::Err))
                    } else {
                        None
                    }
                }
            }));

        // rust-lang/rust#61631: The type `Self` is essentially
        // another type parameter. For ADTs, we consider it
        // well-defined only after all of the ADT type parameters have
        // been provided. Therefore, we do not allow use of `Self`
        // anywhere in ADT type parameter defaults.
        //
        // (We however cannot ban `Self` for defaults on *all* generic
        // lists; e.g. trait generics can usefully refer to `Self`,
        // such as in the case of `trait Add<Rhs = Self>`.)
        if self.diagnostic_metadata.current_self_item.is_some() {
            // (`Some` if + only if we are in ADT's generics.)
            default_ban_rib.bindings.insert(Ident::with_dummy_span(kw::SelfUpper), Res::Err);
        }

        for param in &generics.params {
            match param.kind {
                GenericParamKind::Lifetime { .. } => self.visit_generic_param(param),
                GenericParamKind::Type { ref default, .. } => {
                    for bound in &param.bounds {
                        self.visit_param_bound(bound);
                    }

                    if let Some(ref ty) = default {
                        self.ribs[TypeNS].push(default_ban_rib);
                        self.visit_ty(ty);
                        default_ban_rib = self.ribs[TypeNS].pop().unwrap();
                    }

                    // Allow all following defaults to refer to this type parameter.
                    default_ban_rib.bindings.remove(&Ident::with_dummy_span(param.ident.name));
                }
                GenericParamKind::Const { ref ty } => {
                    for bound in &param.bounds {
                        self.visit_param_bound(bound);
                    }
                    self.visit_ty(ty);
                }
            }
        }
        for p in &generics.where_clause.predicates {
            self.visit_where_predicate(p);
        }
    }

    fn visit_generic_arg(&mut self, arg: &'tcx GenericArg) {
        debug!("visit_generic_arg({:?})", arg);
        match arg {
            GenericArg::Type(ref ty) => {
                // We parse const arguments as path types as we cannot distiguish them durring
                // parsing. We try to resolve that ambiguity by attempting resolution the type
                // namespace first, and if that fails we try again in the value namespace. If
                // resolution in the value namespace succeeds, we have an generic const argument on
                // our hands.
                if let TyKind::Path(ref qself, ref path) = ty.kind {
                    // We cannot disambiguate multi-segment paths right now as that requires type
                    // checking.
                    if path.segments.len() == 1 && path.segments[0].args.is_none() {
                        let mut check_ns = |ns| self.resolve_ident_in_lexical_scope(
                            path.segments[0].ident, ns, None, path.span
                        ).is_some();

                        if !check_ns(TypeNS) && check_ns(ValueNS) {
                            // This must be equivalent to `visit_anon_const`, but we cannot call it
                            // directly due to visitor lifetimes so we have to copy-paste some code.
                            self.with_constant_rib(|this| {
                                this.smart_resolve_path(
                                    ty.id,
                                    qself.as_ref(),
                                    path,
                                    PathSource::Expr(None)
                                );

                                if let Some(ref qself) = *qself {
                                    this.visit_ty(&qself.ty);
                                }
                                this.visit_path(path, ty.id);
                            });

                            return;
                        }
                    }
                }

                self.visit_ty(ty);
            }
            GenericArg::Lifetime(lt) => self.visit_lifetime(lt),
            GenericArg::Const(ct) => self.visit_anon_const(ct),
        }
    }
}

impl<'a, 'b> LateResolutionVisitor<'a, '_> {
    fn new(resolver: &'b mut Resolver<'a>) -> LateResolutionVisitor<'a, 'b> {
        // During late resolution we only track the module component of the parent scope,
        // although it may be useful to track other components as well for diagnostics.
        let graph_root = resolver.graph_root;
        let parent_scope = ParentScope::module(graph_root);
        let start_rib_kind = ModuleRibKind(graph_root);
        LateResolutionVisitor {
            r: resolver,
            parent_scope,
            ribs: PerNS {
                value_ns: vec![Rib::new(start_rib_kind)],
                type_ns: vec![Rib::new(start_rib_kind)],
                macro_ns: vec![Rib::new(start_rib_kind)],
            },
            label_ribs: Vec::new(),
            current_trait_ref: None,
            diagnostic_metadata: DiagnosticMetadata::default(),
        }
    }

    fn resolve_ident_in_lexical_scope(&mut self,
                                      ident: Ident,
                                      ns: Namespace,
                                      record_used_id: Option<NodeId>,
                                      path_span: Span)
                                      -> Option<LexicalScopeBinding<'a>> {
        self.r.resolve_ident_in_lexical_scope(
            ident, ns, &self.parent_scope, record_used_id, path_span, &self.ribs[ns]
        )
    }

    fn resolve_path(
        &mut self,
        path: &[Segment],
        opt_ns: Option<Namespace>, // `None` indicates a module path in import
        record_used: bool,
        path_span: Span,
        crate_lint: CrateLint,
    ) -> PathResult<'a> {
        self.r.resolve_path_with_ribs(
            path, opt_ns, &self.parent_scope, record_used, path_span, crate_lint, Some(&self.ribs)
        )
    }

    // AST resolution
    //
    // We maintain a list of value ribs and type ribs.
    //
    // Simultaneously, we keep track of the current position in the module
    // graph in the `parent_scope.module` pointer. When we go to resolve a name in
    // the value or type namespaces, we first look through all the ribs and
    // then query the module graph. When we resolve a name in the module
    // namespace, we can skip all the ribs (since nested modules are not
    // allowed within blocks in Rust) and jump straight to the current module
    // graph node.
    //
    // Named implementations are handled separately. When we find a method
    // call, we consult the module node to find all of the implementations in
    // scope. This information is lazily cached in the module node. We then
    // generate a fake "implementation scope" containing all the
    // implementations thus found, for compatibility with old resolve pass.

    /// Do some `work` within a new innermost rib of the given `kind` in the given namespace (`ns`).
    fn with_rib<T>(
        &mut self,
        ns: Namespace,
        kind: RibKind<'a>,
        work: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.ribs[ns].push(Rib::new(kind));
        let ret = work(self);
        self.ribs[ns].pop();
        ret
    }

    fn with_scope<T>(&mut self, id: NodeId, f: impl FnOnce(&mut Self) -> T) -> T {
        let id = self.r.definitions.local_def_id(id);
        let module = self.r.module_map.get(&id).cloned(); // clones a reference
        if let Some(module) = module {
            // Move down in the graph.
            let orig_module = replace(&mut self.parent_scope.module, module);
            self.with_rib(ValueNS, ModuleRibKind(module), |this| {
                this.with_rib(TypeNS, ModuleRibKind(module), |this| {
                    let ret = f(this);
                    this.parent_scope.module = orig_module;
                    ret
                })
            })
        } else {
            f(self)
        }
    }

    /// Searches the current set of local scopes for labels. Returns the first non-`None` label that
    /// is returned by the given predicate function
    ///
    /// Stops after meeting a closure.
    fn search_label<P, R>(&self, mut ident: Ident, pred: P) -> Option<R>
        where P: Fn(&Rib<'_, NodeId>, Ident) -> Option<R>
    {
        for rib in self.label_ribs.iter().rev() {
            match rib.kind {
                NormalRibKind => {}
                // If an invocation of this macro created `ident`, give up on `ident`
                // and switch to `ident`'s source from the macro definition.
                MacroDefinition(def) => {
                    if def == self.r.macro_def(ident.span.ctxt()) {
                        ident.span.remove_mark();
                    }
                }
                _ => {
                    // Do not resolve labels across function boundary
                    return None;
                }
            }
            let r = pred(rib, ident);
            if r.is_some() {
                return r;
            }
        }
        None
    }

    fn resolve_adt(&mut self, item: &Item, generics: &Generics) {
        debug!("resolve_adt");
        self.with_current_self_item(item, |this| {
            this.with_generic_param_rib(generics, ItemRibKind(HasGenericParams::Yes), |this| {
                let item_def_id = this.r.definitions.local_def_id(item.id);
                this.with_self_rib(Res::SelfTy(None, Some(item_def_id)), |this| {
                    visit::walk_item(this, item);
                });
            });
        });
    }

    fn future_proof_import(&mut self, use_tree: &UseTree) {
        let segments = &use_tree.prefix.segments;
        if !segments.is_empty() {
            let ident = segments[0].ident;
            if ident.is_path_segment_keyword() || ident.span.rust_2015() {
                return;
            }

            let nss = match use_tree.kind {
                UseTreeKind::Simple(..) if segments.len() == 1 => &[TypeNS, ValueNS][..],
                _ => &[TypeNS],
            };
            let report_error = |this: &Self, ns| {
                let what = if ns == TypeNS { "type parameters" } else { "local variables" };
                this.r.session.span_err(ident.span, &format!("imports cannot refer to {}", what));
            };

            for &ns in nss {
                match self.resolve_ident_in_lexical_scope(ident, ns, None, use_tree.prefix.span) {
                    Some(LexicalScopeBinding::Res(..)) => {
                        report_error(self, ns);
                    }
                    Some(LexicalScopeBinding::Item(binding)) => {
                        let orig_blacklisted_binding =
                            replace(&mut self.r.blacklisted_binding, Some(binding));
                        if let Some(LexicalScopeBinding::Res(..)) =
                                self.resolve_ident_in_lexical_scope(ident, ns, None,
                                                                    use_tree.prefix.span) {
                            report_error(self, ns);
                        }
                        self.r.blacklisted_binding = orig_blacklisted_binding;
                    }
                    None => {}
                }
            }
        } else if let UseTreeKind::Nested(use_trees) = &use_tree.kind {
            for (use_tree, _) in use_trees {
                self.future_proof_import(use_tree);
            }
        }
    }

    fn resolve_item(&mut self, item: &Item) {
        let name = item.ident.name;
        debug!("(resolving item) resolving {} ({:?})", name, item.kind);

        match item.kind {
            ItemKind::TyAlias(_, ref generics) |
            ItemKind::Fn(_, ref generics, _) => {
                self.with_generic_param_rib(generics, ItemRibKind(HasGenericParams::Yes),
                                            |this| visit::walk_item(this, item));
            }

            ItemKind::Enum(_, ref generics) |
            ItemKind::Struct(_, ref generics) |
            ItemKind::Union(_, ref generics) => {
                self.resolve_adt(item, generics);
            }

            ItemKind::Impl(.., ref generics, ref opt_trait_ref, ref self_type, ref impl_items) =>
                self.resolve_implementation(generics,
                                            opt_trait_ref,
                                            &self_type,
                                            item.id,
                                            impl_items),

            ItemKind::Trait(.., ref generics, ref bounds, ref trait_items) => {
                // Create a new rib for the trait-wide type parameters.
                self.with_generic_param_rib(generics, ItemRibKind(HasGenericParams::Yes), |this| {
                    let local_def_id = this.r.definitions.local_def_id(item.id);
                    this.with_self_rib(Res::SelfTy(Some(local_def_id), None), |this| {
                        this.visit_generics(generics);
                        walk_list!(this, visit_param_bound, bounds);

                        for trait_item in trait_items {
                            this.with_trait_items(trait_items, |this| {
                                this.with_generic_param_rib(&trait_item.generics, AssocItemRibKind,
                                    |this| {
                                        match trait_item.kind {
                                            TraitItemKind::Const(ref ty, ref default) => {
                                                this.visit_ty(ty);

                                                // Only impose the restrictions of
                                                // ConstRibKind for an actual constant
                                                // expression in a provided default.
                                                if let Some(ref expr) = *default{
                                                    this.with_constant_rib(|this| {
                                                        this.visit_expr(expr);
                                                    });
                                                }
                                            }
                                            TraitItemKind::Method(_, _) => {
                                                visit::walk_trait_item(this, trait_item)
                                            }
                                            TraitItemKind::Type(..) => {
                                                visit::walk_trait_item(this, trait_item)
                                            }
                                            TraitItemKind::Macro(_) => {
                                                panic!("unexpanded macro in resolve!")
                                            }
                                        };
                                    });
                            });
                        }
                    });
                });
            }

            ItemKind::TraitAlias(ref generics, ref bounds) => {
                // Create a new rib for the trait-wide type parameters.
                self.with_generic_param_rib(generics, ItemRibKind(HasGenericParams::Yes), |this| {
                    let local_def_id = this.r.definitions.local_def_id(item.id);
                    this.with_self_rib(Res::SelfTy(Some(local_def_id), None), |this| {
                        this.visit_generics(generics);
                        walk_list!(this, visit_param_bound, bounds);
                    });
                });
            }

            ItemKind::Mod(_) | ItemKind::ForeignMod(_) => {
                self.with_scope(item.id, |this| {
                    visit::walk_item(this, item);
                });
            }

            ItemKind::Static(ref ty, _, ref expr) |
            ItemKind::Const(ref ty, ref expr) => {
                debug!("resolve_item ItemKind::Const");
                self.with_item_rib(HasGenericParams::No, |this| {
                    this.visit_ty(ty);
                    this.with_constant_rib(|this| {
                        this.visit_expr(expr);
                    });
                });
            }

            ItemKind::Use(ref use_tree) => {
                self.future_proof_import(use_tree);
            }

            ItemKind::ExternCrate(..) |
            ItemKind::MacroDef(..) | ItemKind::GlobalAsm(..) => {
                // do nothing, these are just around to be encoded
            }

            ItemKind::Mac(_) => panic!("unexpanded macro in resolve!"),
        }
    }

    fn with_generic_param_rib<'c, F>(&'c mut self, generics: &'c Generics, kind: RibKind<'a>, f: F)
        where F: FnOnce(&mut Self)
    {
        debug!("with_generic_param_rib");
        let mut function_type_rib = Rib::new(kind);
        let mut function_value_rib = Rib::new(kind);
        let mut seen_bindings = FxHashMap::default();

        // We also can't shadow bindings from the parent item
        if let AssocItemRibKind = kind {
            let mut add_bindings_for_ns = |ns| {
                let parent_rib = self.ribs[ns].iter()
                    .rfind(|r| if let ItemRibKind(_) = r.kind { true } else { false })
                    .expect("associated item outside of an item");
                seen_bindings.extend(
                    parent_rib.bindings.iter().map(|(ident, _)| (*ident, ident.span)),
                );
            };
            add_bindings_for_ns(ValueNS);
            add_bindings_for_ns(TypeNS);
        }

        for param in &generics.params {
            if let GenericParamKind::Lifetime { .. } = param.kind {
                continue;
            }

            let def_kind = match param.kind {
                GenericParamKind::Type { .. } => DefKind::TyParam,
                GenericParamKind::Const { .. } => DefKind::ConstParam,
                _ => unreachable!(),
            };

            let ident = param.ident.modern();
            debug!("with_generic_param_rib: {}", param.id);

            if seen_bindings.contains_key(&ident) {
                let span = seen_bindings.get(&ident).unwrap();
                let err = ResolutionError::NameAlreadyUsedInParameterList(
                    ident.name,
                    *span,
                );
                self.r.report_error(param.ident.span, err);
            }
            seen_bindings.entry(ident).or_insert(param.ident.span);

            // Plain insert (no renaming).
            let res = Res::Def(def_kind, self.r.definitions.local_def_id(param.id));

            match param.kind {
                GenericParamKind::Type { .. } => {
                    function_type_rib.bindings.insert(ident, res);
                    self.r.record_partial_res(param.id, PartialRes::new(res));
                }
                GenericParamKind::Const { .. } => {
                    function_value_rib.bindings.insert(ident, res);
                    self.r.record_partial_res(param.id, PartialRes::new(res));
                }
                _ => unreachable!(),
            }
        }

        self.ribs[ValueNS].push(function_value_rib);
        self.ribs[TypeNS].push(function_type_rib);

        f(self);

        self.ribs[TypeNS].pop();
        self.ribs[ValueNS].pop();
    }

    fn with_label_rib(&mut self, kind: RibKind<'a>, f: impl FnOnce(&mut Self)) {
        self.label_ribs.push(Rib::new(kind));
        f(self);
        self.label_ribs.pop();
    }

    fn with_item_rib(&mut self, has_generic_params: HasGenericParams, f: impl FnOnce(&mut Self)) {
        let kind = ItemRibKind(has_generic_params);
        self.with_rib(ValueNS, kind, |this| this.with_rib(TypeNS, kind, f))
    }

    fn with_constant_rib(&mut self, f: impl FnOnce(&mut Self)) {
        debug!("with_constant_rib");
        self.with_rib(ValueNS, ConstantItemRibKind, |this| {
            this.with_label_rib(ConstantItemRibKind, f);
        });
    }

    fn with_current_self_type<T>(&mut self, self_type: &Ty, f: impl FnOnce(&mut Self) -> T) -> T {
        // Handle nested impls (inside fn bodies)
        let previous_value = replace(
            &mut self.diagnostic_metadata.current_self_type,
            Some(self_type.clone()),
        );
        let result = f(self);
        self.diagnostic_metadata.current_self_type = previous_value;
        result
    }

    fn with_current_self_item<T>(&mut self, self_item: &Item, f: impl FnOnce(&mut Self) -> T) -> T {
        let previous_value = replace(
            &mut self.diagnostic_metadata.current_self_item,
            Some(self_item.id),
        );
        let result = f(self);
        self.diagnostic_metadata.current_self_item = previous_value;
        result
    }

    /// When evaluating a `trait` use its associated types' idents for suggestionsa in E0412.
    fn with_trait_items<T>(
        &mut self,
        trait_items: &Vec<TraitItem>,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let trait_assoc_types = replace(
            &mut self.diagnostic_metadata.current_trait_assoc_types,
            trait_items.iter().filter_map(|item| match &item.kind {
                TraitItemKind::Type(bounds, _) if bounds.len() == 0 => Some(item.ident),
                _ => None,
            }).collect(),
        );
        let result = f(self);
        self.diagnostic_metadata.current_trait_assoc_types = trait_assoc_types;
        result
    }

    /// This is called to resolve a trait reference from an `impl` (i.e., `impl Trait for Foo`).
    fn with_optional_trait_ref<T>(
        &mut self,
        opt_trait_ref: Option<&TraitRef>,
        f: impl FnOnce(&mut Self, Option<DefId>) -> T
    ) -> T {
        let mut new_val = None;
        let mut new_id = None;
        if let Some(trait_ref) = opt_trait_ref {
            let path: Vec<_> = Segment::from_path(&trait_ref.path);
            let res = self.smart_resolve_path_fragment(
                trait_ref.ref_id,
                None,
                &path,
                trait_ref.path.span,
                PathSource::Trait(AliasPossibility::No),
                CrateLint::SimplePath(trait_ref.ref_id),
            ).base_res();
            if res != Res::Err {
                new_id = Some(res.def_id());
                let span = trait_ref.path.span;
                if let PathResult::Module(ModuleOrUniformRoot::Module(module)) =
                    self.resolve_path(
                        &path,
                        Some(TypeNS),
                        false,
                        span,
                        CrateLint::SimplePath(trait_ref.ref_id),
                    )
                {
                    new_val = Some((module, trait_ref.clone()));
                }
            }
        }
        let original_trait_ref = replace(&mut self.current_trait_ref, new_val);
        let result = f(self, new_id);
        self.current_trait_ref = original_trait_ref;
        result
    }

    fn with_self_rib_ns(&mut self, ns: Namespace, self_res: Res, f: impl FnOnce(&mut Self)) {
        let mut self_type_rib = Rib::new(NormalRibKind);

        // Plain insert (no renaming, since types are not currently hygienic)
        self_type_rib.bindings.insert(Ident::with_dummy_span(kw::SelfUpper), self_res);
        self.ribs[ns].push(self_type_rib);
        f(self);
        self.ribs[ns].pop();
    }

    fn with_self_rib(&mut self, self_res: Res, f: impl FnOnce(&mut Self)) {
        self.with_self_rib_ns(TypeNS, self_res, f)
    }

    fn resolve_implementation(&mut self,
                              generics: &Generics,
                              opt_trait_reference: &Option<TraitRef>,
                              self_type: &Ty,
                              item_id: NodeId,
                              impl_items: &[ImplItem]) {
        debug!("resolve_implementation");
        // If applicable, create a rib for the type parameters.
        self.with_generic_param_rib(generics, ItemRibKind(HasGenericParams::Yes), |this| {
            // Dummy self type for better errors if `Self` is used in the trait path.
            this.with_self_rib(Res::SelfTy(None, None), |this| {
                // Resolve the trait reference, if necessary.
                this.with_optional_trait_ref(opt_trait_reference.as_ref(), |this, trait_id| {
                    let item_def_id = this.r.definitions.local_def_id(item_id);
                    this.with_self_rib(Res::SelfTy(trait_id, Some(item_def_id)), |this| {
                        if let Some(trait_ref) = opt_trait_reference.as_ref() {
                            // Resolve type arguments in the trait path.
                            visit::walk_trait_ref(this, trait_ref);
                        }
                        // Resolve the self type.
                        this.visit_ty(self_type);
                        // Resolve the generic parameters.
                        this.visit_generics(generics);
                        // Resolve the items within the impl.
                        this.with_current_self_type(self_type, |this| {
                            this.with_self_rib_ns(ValueNS, Res::SelfCtor(item_def_id), |this| {
                                debug!("resolve_implementation with_self_rib_ns(ValueNS, ...)");
                                for impl_item in impl_items {
                                    // We also need a new scope for the impl item type parameters.
                                    this.with_generic_param_rib(&impl_item.generics,
                                                                AssocItemRibKind,
                                                                |this| {
                                        use crate::ResolutionError::*;
                                        match impl_item.kind {
                                            ImplItemKind::Const(..) => {
                                                debug!(
                                                    "resolve_implementation ImplItemKind::Const",
                                                );
                                                // If this is a trait impl, ensure the const
                                                // exists in trait
                                                this.check_trait_item(
                                                    impl_item.ident,
                                                    ValueNS,
                                                    impl_item.span,
                                                    |n, s| ConstNotMemberOfTrait(n, s),
                                                );

                                                this.with_constant_rib(|this| {
                                                    visit::walk_impl_item(this, impl_item)
                                                });
                                            }
                                            ImplItemKind::Method(..) => {
                                                // If this is a trait impl, ensure the method
                                                // exists in trait
                                                this.check_trait_item(impl_item.ident,
                                                                      ValueNS,
                                                                      impl_item.span,
                                                    |n, s| MethodNotMemberOfTrait(n, s));

                                                visit::walk_impl_item(this, impl_item);
                                            }
                                            ImplItemKind::TyAlias(ref ty) => {
                                                // If this is a trait impl, ensure the type
                                                // exists in trait
                                                this.check_trait_item(impl_item.ident,
                                                                      TypeNS,
                                                                      impl_item.span,
                                                    |n, s| TypeNotMemberOfTrait(n, s));

                                                this.visit_ty(ty);
                                            }
                                            ImplItemKind::Macro(_) =>
                                                panic!("unexpanded macro in resolve!"),
                                        }
                                    });
                                }
                            });
                        });
                    });
                });
            });
        });
    }

    fn check_trait_item<F>(&mut self, ident: Ident, ns: Namespace, span: Span, err: F)
        where F: FnOnce(Name, &str) -> ResolutionError<'_>
    {
        // If there is a TraitRef in scope for an impl, then the method must be in the
        // trait.
        if let Some((module, _)) = self.current_trait_ref {
            if self.r.resolve_ident_in_module(
                ModuleOrUniformRoot::Module(module),
                ident,
                ns,
                &self.parent_scope,
                false,
                span,
            ).is_err() {
                let path = &self.current_trait_ref.as_ref().unwrap().1.path;
                self.r.report_error(span, err(ident.name, &path_names_to_string(path)));
            }
        }
    }

    fn resolve_params(&mut self, params: &[Param]) {
        let mut bindings = smallvec![(PatBoundCtx::Product, Default::default())];
        for Param { pat, ty, .. } in params {
            self.resolve_pattern(pat, PatternSource::FnParam, &mut bindings);
            self.visit_ty(ty);
            debug!("(resolving function / closure) recorded parameter");
        }
    }

    fn resolve_local(&mut self, local: &Local) {
        // Resolve the type.
        walk_list!(self, visit_ty, &local.ty);

        // Resolve the initializer.
        walk_list!(self, visit_expr, &local.init);

        // Resolve the pattern.
        self.resolve_pattern_top(&local.pat, PatternSource::Let);
    }

    /// build a map from pattern identifiers to binding-info's.
    /// this is done hygienically. This could arise for a macro
    /// that expands into an or-pattern where one 'x' was from the
    /// user and one 'x' came from the macro.
    fn binding_mode_map(&mut self, pat: &Pat) -> BindingMap {
        let mut binding_map = FxHashMap::default();

        pat.walk(&mut |pat| {
            match pat.kind {
                PatKind::Ident(binding_mode, ident, ref sub_pat)
                    if sub_pat.is_some() || self.is_base_res_local(pat.id) =>
                {
                    binding_map.insert(ident, BindingInfo { span: ident.span, binding_mode });
                }
                PatKind::Or(ref ps) => {
                    // Check the consistency of this or-pattern and
                    // then add all bindings to the larger map.
                    for bm in self.check_consistent_bindings(ps) {
                        binding_map.extend(bm);
                    }
                    return false;
                }
                _ => {}
            }

            true
        });

        binding_map
    }

    fn is_base_res_local(&self, nid: NodeId) -> bool {
        match self.r.partial_res_map.get(&nid).map(|res| res.base_res()) {
            Some(Res::Local(..)) => true,
            _ => false,
        }
    }

    /// Checks that all of the arms in an or-pattern have exactly the
    /// same set of bindings, with the same binding modes for each.
    fn check_consistent_bindings(&mut self, pats: &[P<Pat>]) -> Vec<BindingMap> {
        let mut missing_vars = FxHashMap::default();
        let mut inconsistent_vars = FxHashMap::default();

        // 1) Compute the binding maps of all arms.
        let maps = pats.iter()
            .map(|pat| self.binding_mode_map(pat))
            .collect::<Vec<_>>();

        // 2) Record any missing bindings or binding mode inconsistencies.
        for (map_outer, pat_outer) in pats.iter().enumerate().map(|(idx, pat)| (&maps[idx], pat)) {
            // Check against all arms except for the same pattern which is always self-consistent.
            let inners = pats.iter().enumerate()
                .filter(|(_, pat)| pat.id != pat_outer.id)
                .flat_map(|(idx, _)| maps[idx].iter())
                .map(|(key, binding)| (key.name, map_outer.get(&key), binding));

            for (name, info, &binding_inner) in inners {
                match info {
                    None => { // The inner binding is missing in the outer.
                        let binding_error = missing_vars
                            .entry(name)
                            .or_insert_with(|| BindingError {
                                name,
                                origin: BTreeSet::new(),
                                target: BTreeSet::new(),
                                could_be_path: name.as_str().starts_with(char::is_uppercase),
                            });
                        binding_error.origin.insert(binding_inner.span);
                        binding_error.target.insert(pat_outer.span);
                    }
                    Some(binding_outer) => {
                        if binding_outer.binding_mode != binding_inner.binding_mode {
                            // The binding modes in the outer and inner bindings differ.
                            inconsistent_vars
                                .entry(name)
                                .or_insert((binding_inner.span, binding_outer.span));
                        }
                    }
                }
            }
        }

        // 3) Report all missing variables we found.
        let mut missing_vars = missing_vars.iter_mut().collect::<Vec<_>>();
        missing_vars.sort();
        for (name, mut v) in missing_vars {
            if inconsistent_vars.contains_key(name) {
                v.could_be_path = false;
            }
            self.r.report_error(
                *v.origin.iter().next().unwrap(),
                ResolutionError::VariableNotBoundInPattern(v));
        }

        // 4) Report all inconsistencies in binding modes we found.
        let mut inconsistent_vars = inconsistent_vars.iter().collect::<Vec<_>>();
        inconsistent_vars.sort();
        for (name, v) in inconsistent_vars {
            self.r.report_error(v.0, ResolutionError::VariableBoundWithDifferentMode(*name, v.1));
        }

        // 5) Finally bubble up all the binding maps.
        maps
    }

    /// Check the consistency of the outermost or-patterns.
    fn check_consistent_bindings_top(&mut self, pat: &Pat) {
        pat.walk(&mut |pat| match pat.kind {
            PatKind::Or(ref ps) => {
                self.check_consistent_bindings(ps);
                false
            },
            _ => true,
        })
    }

    fn resolve_arm(&mut self, arm: &Arm) {
        self.with_rib(ValueNS, NormalRibKind, |this| {
            this.resolve_pattern_top(&arm.pat, PatternSource::Match);
            walk_list!(this, visit_expr, &arm.guard);
            this.visit_expr(&arm.body);
        });
    }

    /// Arising from `source`, resolve a top level pattern.
    fn resolve_pattern_top(&mut self, pat: &Pat, pat_src: PatternSource) {
        let mut bindings = smallvec![(PatBoundCtx::Product, Default::default())];
        self.resolve_pattern(pat, pat_src, &mut bindings);
    }

    fn resolve_pattern(
        &mut self,
        pat: &Pat,
        pat_src: PatternSource,
        bindings: &mut SmallVec<[(PatBoundCtx, FxHashSet<Ident>); 1]>,
    ) {
        self.resolve_pattern_inner(pat, pat_src, bindings);
        // This has to happen *after* we determine which pat_idents are variants:
        self.check_consistent_bindings_top(pat);
        visit::walk_pat(self, pat);
    }

    /// Resolve bindings in a pattern. This is a helper to `resolve_pattern`.
    ///
    /// ### `bindings`
    ///
    /// A stack of sets of bindings accumulated.
    ///
    /// In each set, `PatBoundCtx::Product` denotes that a found binding in it should
    /// be interpreted as re-binding an already bound binding. This results in an error.
    /// Meanwhile, `PatBound::Or` denotes that a found binding in the set should result
    /// in reusing this binding rather than creating a fresh one.
    ///
    /// When called at the top level, the stack must have a single element
    /// with `PatBound::Product`. Otherwise, pushing to the stack happens as
    /// or-patterns (`p_0 | ... | p_n`) are encountered and the context needs
    /// to be switched to `PatBoundCtx::Or` and then `PatBoundCtx::Product` for each `p_i`.
    /// When each `p_i` has been dealt with, the top set is merged with its parent.
    /// When a whole or-pattern has been dealt with, the thing happens.
    ///
    /// See the implementation and `fresh_binding` for more details.
    fn resolve_pattern_inner(
        &mut self,
        pat: &Pat,
        pat_src: PatternSource,
        bindings: &mut SmallVec<[(PatBoundCtx, FxHashSet<Ident>); 1]>,
    ) {
        // Visit all direct subpatterns of this pattern.
        pat.walk(&mut |pat| {
            debug!("resolve_pattern pat={:?} node={:?}", pat, pat.kind);
            match pat.kind {
                PatKind::Ident(bmode, ident, ref sub) => {
                    // First try to resolve the identifier as some existing entity,
                    // then fall back to a fresh binding.
                    let has_sub = sub.is_some();
                    let res = self.try_resolve_as_non_binding(pat_src, pat, bmode, ident, has_sub)
                        .unwrap_or_else(|| self.fresh_binding(ident, pat.id, pat_src, bindings));
                    self.r.record_partial_res(pat.id, PartialRes::new(res));
                }
                PatKind::TupleStruct(ref path, ..) => {
                    self.smart_resolve_path(pat.id, None, path, PathSource::TupleStruct);
                }
                PatKind::Path(ref qself, ref path) => {
                    self.smart_resolve_path(pat.id, qself.as_ref(), path, PathSource::Pat);
                }
                PatKind::Struct(ref path, ..) => {
                    self.smart_resolve_path(pat.id, None, path, PathSource::Struct);
                }
                PatKind::Or(ref ps) => {
                    // Add a new set of bindings to the stack. `Or` here records that when a
                    // binding already exists in this set, it should not result in an error because
                    // `V1(a) | V2(a)` must be allowed and are checked for consistency later.
                    bindings.push((PatBoundCtx::Or, Default::default()));
                    for p in ps {
                        // Now we need to switch back to a product context so that each
                        // part of the or-pattern internally rejects already bound names.
                        // For example, `V1(a) | V2(a, a)` and `V1(a, a) | V2(a)` are bad.
                        bindings.push((PatBoundCtx::Product, Default::default()));
                        self.resolve_pattern_inner(p, pat_src, bindings);
                        // Move up the non-overlapping bindings to the or-pattern.
                        // Existing bindings just get "merged".
                        let collected = bindings.pop().unwrap().1;
                        bindings.last_mut().unwrap().1.extend(collected);
                    }
                    // This or-pattern itself can itself be part of a product,
                    // e.g. `(V1(a) | V2(a), a)` or `(a, V1(a) | V2(a))`.
                    // Both cases bind `a` again in a product pattern and must be rejected.
                    let collected = bindings.pop().unwrap().1;
                    bindings.last_mut().unwrap().1.extend(collected);

                    // Prevent visiting `ps` as we've already done so above.
                    return false;
                }
                _ => {}
            }
            true
        });
    }

    fn fresh_binding(
        &mut self,
        ident: Ident,
        pat_id: NodeId,
        pat_src: PatternSource,
        bindings: &mut SmallVec<[(PatBoundCtx, FxHashSet<Ident>); 1]>,
    ) -> Res {
        // Add the binding to the local ribs, if it doesn't already exist in the bindings map.
        // (We must not add it if it's in the bindings map because that breaks the assumptions
        // later passes make about or-patterns.)
        let ident = ident.modern_and_legacy();

        let mut bound_iter = bindings.iter().filter(|(_, set)| set.contains(&ident));
        // Already bound in a product pattern? e.g. `(a, a)` which is not allowed.
        let already_bound_and = bound_iter.clone().any(|(ctx, _)| *ctx == PatBoundCtx::Product);
        // Already bound in an or-pattern? e.g. `V1(a) | V2(a)`.
        // This is *required* for consistency which is checked later.
        let already_bound_or = bound_iter.any(|(ctx, _)| *ctx == PatBoundCtx::Or);

        if already_bound_and {
            // Overlap in a product pattern somewhere; report an error.
            use ResolutionError::*;
            let error = match pat_src {
                // `fn f(a: u8, a: u8)`:
                PatternSource::FnParam => IdentifierBoundMoreThanOnceInParameterList,
                // `Variant(a, a)`:
                _ => IdentifierBoundMoreThanOnceInSamePattern,
            };
            self.r.report_error(ident.span, error(&ident.as_str()));
        }

        // Record as bound if it's valid:
        let ident_valid = ident.name != kw::Invalid;
        if ident_valid {
            bindings.last_mut().unwrap().1.insert(ident);
        }

        if already_bound_or {
            // `Variant1(a) | Variant2(a)`, ok
            // Reuse definition from the first `a`.
            self.innermost_rib_bindings(ValueNS)[&ident]
        } else {
            let res = Res::Local(pat_id);
            if ident_valid {
                // A completely fresh binding add to the set if it's valid.
                self.innermost_rib_bindings(ValueNS).insert(ident, res);
            }
            res
        }
    }

    fn innermost_rib_bindings(&mut self, ns: Namespace) -> &mut IdentMap<Res> {
        &mut self.ribs[ns].last_mut().unwrap().bindings
    }

    fn try_resolve_as_non_binding(
        &mut self,
        pat_src: PatternSource,
        pat: &Pat,
        bm: BindingMode,
        ident: Ident,
        has_sub: bool,
    ) -> Option<Res> {
        let binding = self.resolve_ident_in_lexical_scope(ident, ValueNS, None, pat.span)?.item()?;
        let res = binding.res();

        // An immutable (no `mut`) by-value (no `ref`) binding pattern without
        // a sub pattern (no `@ $pat`) is syntactically ambiguous as it could
        // also be interpreted as a path to e.g. a constant, variant, etc.
        let is_syntactic_ambiguity = !has_sub && bm == BindingMode::ByValue(Mutability::Immutable);

        match res {
            Res::Def(DefKind::Ctor(_, CtorKind::Const), _) |
            Res::Def(DefKind::Const, _) if is_syntactic_ambiguity => {
                // Disambiguate in favor of a unit struct/variant or constant pattern.
                self.r.record_use(ident, ValueNS, binding, false);
                Some(res)
            }
            Res::Def(DefKind::Ctor(..), _)
            | Res::Def(DefKind::Const, _)
            | Res::Def(DefKind::Static, _) => {
                // This is unambiguously a fresh binding, either syntactically
                // (e.g., `IDENT @ PAT` or `ref IDENT`) or because `IDENT` resolves
                // to something unusable as a pattern (e.g., constructor function),
                // but we still conservatively report an error, see
                // issues/33118#issuecomment-233962221 for one reason why.
                self.r.report_error(
                    ident.span,
                    ResolutionError::BindingShadowsSomethingUnacceptable(
                        pat_src.descr(),
                        ident.name,
                        binding,
                    ),
                );
                None
            }
            Res::Def(DefKind::Fn, _) | Res::Err => {
                // These entities are explicitly allowed to be shadowed by fresh bindings.
                None
            }
            res => {
                span_bug!(ident.span, "unexpected resolution for an \
                                        identifier in pattern: {:?}", res);
            }
        }
    }

    // High-level and context dependent path resolution routine.
    // Resolves the path and records the resolution into definition map.
    // If resolution fails tries several techniques to find likely
    // resolution candidates, suggest imports or other help, and report
    // errors in user friendly way.
    fn smart_resolve_path(&mut self,
                          id: NodeId,
                          qself: Option<&QSelf>,
                          path: &Path,
                          source: PathSource<'_>) {
        self.smart_resolve_path_fragment(
            id,
            qself,
            &Segment::from_path(path),
            path.span,
            source,
            CrateLint::SimplePath(id),
        );
    }

    fn smart_resolve_path_fragment(&mut self,
                                   id: NodeId,
                                   qself: Option<&QSelf>,
                                   path: &[Segment],
                                   span: Span,
                                   source: PathSource<'_>,
                                   crate_lint: CrateLint)
                                   -> PartialRes {
        let ns = source.namespace();
        let is_expected = &|res| source.is_expected(res);

        let report_errors = |this: &mut Self, res: Option<Res>| {
            let (err, candidates) = this.smart_resolve_report_errors(path, span, source, res);
            let def_id = this.parent_scope.module.normal_ancestor_id;
            let node_id = this.r.definitions.as_local_node_id(def_id).unwrap();
            let better = res.is_some();
            this.r.use_injections.push(UseError { err, candidates, node_id, better });
            PartialRes::new(Res::Err)
        };

        let partial_res = match self.resolve_qpath_anywhere(
            id,
            qself,
            path,
            ns,
            span,
            source.defer_to_typeck(),
            crate_lint,
        ) {
            Some(partial_res) if partial_res.unresolved_segments() == 0 => {
                if is_expected(partial_res.base_res()) || partial_res.base_res() == Res::Err {
                    partial_res
                } else {
                    report_errors(self, Some(partial_res.base_res()))
                }
            }
            Some(partial_res) if source.defer_to_typeck() => {
                // Not fully resolved associated item `T::A::B` or `<T as Tr>::A::B`
                // or `<T>::A::B`. If `B` should be resolved in value namespace then
                // it needs to be added to the trait map.
                if ns == ValueNS {
                    let item_name = path.last().unwrap().ident;
                    let traits = self.get_traits_containing_item(item_name, ns);
                    self.r.trait_map.insert(id, traits);
                }

                let mut std_path = vec![Segment::from_ident(Ident::with_dummy_span(sym::std))];
                std_path.extend(path);
                if self.r.primitive_type_table.primitive_types.contains_key(&path[0].ident.name) {
                    let cl = CrateLint::No;
                    let ns = Some(ns);
                    if let PathResult::Module(_) | PathResult::NonModule(_) =
                            self.resolve_path(&std_path, ns, false, span, cl) {
                        // check if we wrote `str::from_utf8` instead of `std::str::from_utf8`
                        let item_span = path.iter().last().map(|segment| segment.ident.span)
                            .unwrap_or(span);
                        debug!("accessed item from `std` submodule as a bare type {:?}", std_path);
                        let mut hm = self.r.session.confused_type_with_std_module.borrow_mut();
                        hm.insert(item_span, span);
                        // In some places (E0223) we only have access to the full path
                        hm.insert(span, span);
                    }
                }
                partial_res
            }
            _ => report_errors(self, None)
        };

        if let PathSource::TraitItem(..) = source {} else {
            // Avoid recording definition of `A::B` in `<T as A>::B::C`.
            self.r.record_partial_res(id, partial_res);
        }
        partial_res
    }

    fn self_type_is_available(&mut self, span: Span) -> bool {
        let binding = self.resolve_ident_in_lexical_scope(
            Ident::with_dummy_span(kw::SelfUpper),
            TypeNS,
            None,
            span,
        );
        if let Some(LexicalScopeBinding::Res(res)) = binding { res != Res::Err } else { false }
    }

    fn self_value_is_available(&mut self, self_span: Span, path_span: Span) -> bool {
        let ident = Ident::new(kw::SelfLower, self_span);
        let binding = self.resolve_ident_in_lexical_scope(ident, ValueNS, None, path_span);
        if let Some(LexicalScopeBinding::Res(res)) = binding { res != Res::Err } else { false }
    }

    // Resolve in alternative namespaces if resolution in the primary namespace fails.
    fn resolve_qpath_anywhere(
        &mut self,
        id: NodeId,
        qself: Option<&QSelf>,
        path: &[Segment],
        primary_ns: Namespace,
        span: Span,
        defer_to_typeck: bool,
        crate_lint: CrateLint,
    ) -> Option<PartialRes> {
        let mut fin_res = None;
        for (i, ns) in [primary_ns, TypeNS, ValueNS].iter().cloned().enumerate() {
            if i == 0 || ns != primary_ns {
                match self.resolve_qpath(id, qself, path, ns, span, crate_lint) {
                    // If defer_to_typeck, then resolution > no resolution,
                    // otherwise full resolution > partial resolution > no resolution.
                    Some(partial_res) if partial_res.unresolved_segments() == 0 ||
                                         defer_to_typeck =>
                        return Some(partial_res),
                    partial_res => if fin_res.is_none() { fin_res = partial_res },
                }
            }
        }

        // `MacroNS`
        assert!(primary_ns != MacroNS);
        if qself.is_none() {
            let path_seg = |seg: &Segment| PathSegment::from_ident(seg.ident);
            let path = Path { segments: path.iter().map(path_seg).collect(), span };
            if let Ok((_, res)) = self.r.resolve_macro_path(
                &path, None, &self.parent_scope, false, false
            ) {
                return Some(PartialRes::new(res));
            }
        }

        fin_res
    }

    /// Handles paths that may refer to associated items.
    fn resolve_qpath(
        &mut self,
        id: NodeId,
        qself: Option<&QSelf>,
        path: &[Segment],
        ns: Namespace,
        span: Span,
        crate_lint: CrateLint,
    ) -> Option<PartialRes> {
        debug!(
            "resolve_qpath(id={:?}, qself={:?}, path={:?}, ns={:?}, span={:?})",
            id,
            qself,
            path,
            ns,
            span,
        );

        if let Some(qself) = qself {
            if qself.position == 0 {
                // This is a case like `<T>::B`, where there is no
                // trait to resolve.  In that case, we leave the `B`
                // segment to be resolved by type-check.
                return Some(PartialRes::with_unresolved_segments(
                    Res::Def(DefKind::Mod, DefId::local(CRATE_DEF_INDEX)), path.len()
                ));
            }

            // Make sure `A::B` in `<T as A::B>::C` is a trait item.
            //
            // Currently, `path` names the full item (`A::B::C`, in
            // our example).  so we extract the prefix of that that is
            // the trait (the slice upto and including
            // `qself.position`). And then we recursively resolve that,
            // but with `qself` set to `None`.
            //
            // However, setting `qself` to none (but not changing the
            // span) loses the information about where this path
            // *actually* appears, so for the purposes of the crate
            // lint we pass along information that this is the trait
            // name from a fully qualified path, and this also
            // contains the full span (the `CrateLint::QPathTrait`).
            let ns = if qself.position + 1 == path.len() { ns } else { TypeNS };
            let partial_res = self.smart_resolve_path_fragment(
                id,
                None,
                &path[..=qself.position],
                span,
                PathSource::TraitItem(ns),
                CrateLint::QPathTrait {
                    qpath_id: id,
                    qpath_span: qself.path_span,
                },
            );

            // The remaining segments (the `C` in our example) will
            // have to be resolved by type-check, since that requires doing
            // trait resolution.
            return Some(PartialRes::with_unresolved_segments(
                partial_res.base_res(),
                partial_res.unresolved_segments() + path.len() - qself.position - 1,
            ));
        }

        let result = match self.resolve_path(&path, Some(ns), true, span, crate_lint) {
            PathResult::NonModule(path_res) => path_res,
            PathResult::Module(ModuleOrUniformRoot::Module(module)) if !module.is_normal() => {
                PartialRes::new(module.res().unwrap())
            }
            // In `a(::assoc_item)*` `a` cannot be a module. If `a` does resolve to a module we
            // don't report an error right away, but try to fallback to a primitive type.
            // So, we are still able to successfully resolve something like
            //
            // use std::u8; // bring module u8 in scope
            // fn f() -> u8 { // OK, resolves to primitive u8, not to std::u8
            //     u8::max_value() // OK, resolves to associated function <u8>::max_value,
            //                     // not to non-existent std::u8::max_value
            // }
            //
            // Such behavior is required for backward compatibility.
            // The same fallback is used when `a` resolves to nothing.
            PathResult::Module(ModuleOrUniformRoot::Module(_)) |
            PathResult::Failed { .. }
                    if (ns == TypeNS || path.len() > 1) &&
                       self.r.primitive_type_table.primitive_types
                           .contains_key(&path[0].ident.name) => {
                let prim = self.r.primitive_type_table.primitive_types[&path[0].ident.name];
                PartialRes::with_unresolved_segments(Res::PrimTy(prim), path.len() - 1)
            }
            PathResult::Module(ModuleOrUniformRoot::Module(module)) =>
                PartialRes::new(module.res().unwrap()),
            PathResult::Failed { is_error_from_last_segment: false, span, label, suggestion } => {
                self.r.report_error(span, ResolutionError::FailedToResolve { label, suggestion });
                PartialRes::new(Res::Err)
            }
            PathResult::Module(..) | PathResult::Failed { .. } => return None,
            PathResult::Indeterminate => bug!("indetermined path result in resolve_qpath"),
        };

        if path.len() > 1 && result.base_res() != Res::Err &&
           path[0].ident.name != kw::PathRoot &&
           path[0].ident.name != kw::DollarCrate {
            let unqualified_result = {
                match self.resolve_path(
                    &[*path.last().unwrap()],
                    Some(ns),
                    false,
                    span,
                    CrateLint::No,
                ) {
                    PathResult::NonModule(path_res) => path_res.base_res(),
                    PathResult::Module(ModuleOrUniformRoot::Module(module)) =>
                        module.res().unwrap(),
                    _ => return Some(result),
                }
            };
            if result.base_res() == unqualified_result {
                let lint = lint::builtin::UNUSED_QUALIFICATIONS;
                self.r.lint_buffer.buffer_lint(lint, id, span, "unnecessary qualification")
            }
        }

        Some(result)
    }

    fn with_resolved_label(&mut self, label: Option<Label>, id: NodeId, f: impl FnOnce(&mut Self)) {
        if let Some(label) = label {
            if label.ident.as_str().as_bytes()[1] != b'_' {
                self.diagnostic_metadata.unused_labels.insert(id, label.ident.span);
            }
            self.with_label_rib(NormalRibKind, |this| {
                let ident = label.ident.modern_and_legacy();
                this.label_ribs.last_mut().unwrap().bindings.insert(ident, id);
                f(this);
            });
        } else {
            f(self);
        }
    }

    fn resolve_labeled_block(&mut self, label: Option<Label>, id: NodeId, block: &Block) {
        self.with_resolved_label(label, id, |this| this.visit_block(block));
    }

    fn resolve_block(&mut self, block: &Block) {
        debug!("(resolving block) entering block");
        // Move down in the graph, if there's an anonymous module rooted here.
        let orig_module = self.parent_scope.module;
        let anonymous_module = self.r.block_map.get(&block.id).cloned(); // clones a reference

        let mut num_macro_definition_ribs = 0;
        if let Some(anonymous_module) = anonymous_module {
            debug!("(resolving block) found anonymous module, moving down");
            self.ribs[ValueNS].push(Rib::new(ModuleRibKind(anonymous_module)));
            self.ribs[TypeNS].push(Rib::new(ModuleRibKind(anonymous_module)));
            self.parent_scope.module = anonymous_module;
        } else {
            self.ribs[ValueNS].push(Rib::new(NormalRibKind));
        }

        // Descend into the block.
        for stmt in &block.stmts {
            if let StmtKind::Item(ref item) = stmt.kind {
                if let ItemKind::MacroDef(..) = item.kind {
                    num_macro_definition_ribs += 1;
                    let res = self.r.definitions.local_def_id(item.id);
                    self.ribs[ValueNS].push(Rib::new(MacroDefinition(res)));
                    self.label_ribs.push(Rib::new(MacroDefinition(res)));
                }
            }

            self.visit_stmt(stmt);
        }

        // Move back up.
        self.parent_scope.module = orig_module;
        for _ in 0 .. num_macro_definition_ribs {
            self.ribs[ValueNS].pop();
            self.label_ribs.pop();
        }
        self.ribs[ValueNS].pop();
        if anonymous_module.is_some() {
            self.ribs[TypeNS].pop();
        }
        debug!("(resolving block) leaving block");
    }

    fn resolve_expr(&mut self, expr: &Expr, parent: Option<&Expr>) {
        // First, record candidate traits for this expression if it could
        // result in the invocation of a method call.

        self.record_candidate_traits_for_expr_if_necessary(expr);

        // Next, resolve the node.
        match expr.kind {
            ExprKind::Path(ref qself, ref path) => {
                self.smart_resolve_path(expr.id, qself.as_ref(), path, PathSource::Expr(parent));
                visit::walk_expr(self, expr);
            }

            ExprKind::Struct(ref path, ..) => {
                self.smart_resolve_path(expr.id, None, path, PathSource::Struct);
                visit::walk_expr(self, expr);
            }

            ExprKind::Break(Some(label), _) | ExprKind::Continue(Some(label)) => {
                let node_id = self.search_label(label.ident, |rib, ident| {
                    rib.bindings.get(&ident.modern_and_legacy()).cloned()
                });
                match node_id {
                    None => {
                        // Search again for close matches...
                        // Picks the first label that is "close enough", which is not necessarily
                        // the closest match
                        let close_match = self.search_label(label.ident, |rib, ident| {
                            let names = rib.bindings.iter().filter_map(|(id, _)| {
                                if id.span.ctxt() == label.ident.span.ctxt() {
                                    Some(&id.name)
                                } else {
                                    None
                                }
                            });
                            find_best_match_for_name(names, &ident.as_str(), None)
                        });
                        self.r.record_partial_res(expr.id, PartialRes::new(Res::Err));
                        self.r.report_error(
                            label.ident.span,
                            ResolutionError::UndeclaredLabel(&label.ident.as_str(), close_match),
                        );
                    }
                    Some(node_id) => {
                        // Since this res is a label, it is never read.
                        self.r.label_res_map.insert(expr.id, node_id);
                        self.diagnostic_metadata.unused_labels.remove(&node_id);
                    }
                }

                // visit `break` argument if any
                visit::walk_expr(self, expr);
            }

            ExprKind::Let(ref pat, ref scrutinee) => {
                self.visit_expr(scrutinee);
                self.resolve_pattern_top(pat, PatternSource::Let);
            }

            ExprKind::If(ref cond, ref then, ref opt_else) => {
                self.with_rib(ValueNS, NormalRibKind, |this| {
                    this.visit_expr(cond);
                    this.visit_block(then);
                });
                opt_else.as_ref().map(|expr| self.visit_expr(expr));
            }

            ExprKind::Loop(ref block, label) => self.resolve_labeled_block(label, expr.id, &block),

            ExprKind::While(ref cond, ref block, label) => {
                self.with_resolved_label(label, expr.id, |this| {
                    this.with_rib(ValueNS, NormalRibKind, |this| {
                        this.visit_expr(cond);
                        this.visit_block(block);
                    })
                });
            }

            ExprKind::ForLoop(ref pat, ref iter_expr, ref block, label) => {
                self.visit_expr(iter_expr);
                self.with_rib(ValueNS, NormalRibKind, |this| {
                    this.resolve_pattern_top(pat, PatternSource::For);
                    this.resolve_labeled_block(label, expr.id, block);
                });
            }

            ExprKind::Block(ref block, label) => self.resolve_labeled_block(label, block.id, block),

            // Equivalent to `visit::walk_expr` + passing some context to children.
            ExprKind::Field(ref subexpression, _) => {
                self.resolve_expr(subexpression, Some(expr));
            }
            ExprKind::MethodCall(ref segment, ref arguments) => {
                let mut arguments = arguments.iter();
                self.resolve_expr(arguments.next().unwrap(), Some(expr));
                for argument in arguments {
                    self.resolve_expr(argument, None);
                }
                self.visit_path_segment(expr.span, segment);
            }

            ExprKind::Call(ref callee, ref arguments) => {
                self.resolve_expr(callee, Some(expr));
                for argument in arguments {
                    self.resolve_expr(argument, None);
                }
            }
            ExprKind::Type(ref type_expr, _) => {
                self.diagnostic_metadata.current_type_ascription.push(type_expr.span);
                visit::walk_expr(self, expr);
                self.diagnostic_metadata.current_type_ascription.pop();
            }
            // `async |x| ...` gets desugared to `|x| future_from_generator(|| ...)`, so we need to
            // resolve the arguments within the proper scopes so that usages of them inside the
            // closure are detected as upvars rather than normal closure arg usages.
            ExprKind::Closure(_, IsAsync::Async { .. }, _, ref fn_decl, ref body, _span) => {
                self.with_rib(ValueNS, NormalRibKind, |this| {
                    // Resolve arguments:
                    this.resolve_params(&fn_decl.inputs);
                    // No need to resolve return type --
                    // the outer closure return type is `FunctionRetTy::Default`.

                    // Now resolve the inner closure
                    {
                        // No need to resolve arguments: the inner closure has none.
                        // Resolve the return type:
                        visit::walk_fn_ret_ty(this, &fn_decl.output);
                        // Resolve the body
                        this.visit_expr(body);
                    }
                });
            }
            _ => {
                visit::walk_expr(self, expr);
            }
        }
    }

    fn record_candidate_traits_for_expr_if_necessary(&mut self, expr: &Expr) {
        match expr.kind {
            ExprKind::Field(_, ident) => {
                // FIXME(#6890): Even though you can't treat a method like a
                // field, we need to add any trait methods we find that match
                // the field name so that we can do some nice error reporting
                // later on in typeck.
                let traits = self.get_traits_containing_item(ident, ValueNS);
                self.r.trait_map.insert(expr.id, traits);
            }
            ExprKind::MethodCall(ref segment, ..) => {
                debug!("(recording candidate traits for expr) recording traits for {}",
                       expr.id);
                let traits = self.get_traits_containing_item(segment.ident, ValueNS);
                self.r.trait_map.insert(expr.id, traits);
            }
            _ => {
                // Nothing to do.
            }
        }
    }

    fn get_traits_containing_item(&mut self, mut ident: Ident, ns: Namespace)
                                  -> Vec<TraitCandidate> {
        debug!("(getting traits containing item) looking for '{}'", ident.name);

        let mut found_traits = Vec::new();
        // Look for the current trait.
        if let Some((module, _)) = self.current_trait_ref {
            if self.r.resolve_ident_in_module(
                ModuleOrUniformRoot::Module(module),
                ident,
                ns,
                &self.parent_scope,
                false,
                module.span,
            ).is_ok() {
                let def_id = module.def_id().unwrap();
                found_traits.push(TraitCandidate { def_id: def_id, import_ids: smallvec![] });
            }
        }

        ident.span = ident.span.modern();
        let mut search_module = self.parent_scope.module;
        loop {
            self.get_traits_in_module_containing_item(ident, ns, search_module, &mut found_traits);
            search_module = unwrap_or!(
                self.r.hygienic_lexical_parent(search_module, &mut ident.span), break
            );
        }

        if let Some(prelude) = self.r.prelude {
            if !search_module.no_implicit_prelude {
                self.get_traits_in_module_containing_item(ident, ns, prelude, &mut found_traits);
            }
        }

        found_traits
    }

    fn get_traits_in_module_containing_item(&mut self,
                                            ident: Ident,
                                            ns: Namespace,
                                            module: Module<'a>,
                                            found_traits: &mut Vec<TraitCandidate>) {
        assert!(ns == TypeNS || ns == ValueNS);
        let mut traits = module.traits.borrow_mut();
        if traits.is_none() {
            let mut collected_traits = Vec::new();
            module.for_each_child(self.r, |_, name, ns, binding| {
                if ns != TypeNS { return }
                match binding.res() {
                    Res::Def(DefKind::Trait, _) |
                    Res::Def(DefKind::TraitAlias, _) => collected_traits.push((name, binding)),
                    _ => (),
                }
            });
            *traits = Some(collected_traits.into_boxed_slice());
        }

        for &(trait_name, binding) in traits.as_ref().unwrap().iter() {
            // Traits have pseudo-modules that can be used to search for the given ident.
            if let Some(module) = binding.module() {
                let mut ident = ident;
                if ident.span.glob_adjust(
                    module.expansion,
                    binding.span,
                ).is_none() {
                    continue
                }
                if self.r.resolve_ident_in_module_unadjusted(
                    ModuleOrUniformRoot::Module(module),
                    ident,
                    ns,
                    &self.parent_scope,
                    false,
                    module.span,
                ).is_ok() {
                    let import_ids = self.find_transitive_imports(&binding.kind, trait_name);
                    let trait_def_id = module.def_id().unwrap();
                    found_traits.push(TraitCandidate { def_id: trait_def_id, import_ids });
                }
            } else if let Res::Def(DefKind::TraitAlias, _) = binding.res() {
                // For now, just treat all trait aliases as possible candidates, since we don't
                // know if the ident is somewhere in the transitive bounds.
                let import_ids = self.find_transitive_imports(&binding.kind, trait_name);
                let trait_def_id = binding.res().def_id();
                found_traits.push(TraitCandidate { def_id: trait_def_id, import_ids });
            } else {
                bug!("candidate is not trait or trait alias?")
            }
        }
    }

    fn find_transitive_imports(&mut self, mut kind: &NameBindingKind<'_>,
                               trait_name: Ident) -> SmallVec<[NodeId; 1]> {
        let mut import_ids = smallvec![];
        while let NameBindingKind::Import { directive, binding, .. } = kind {
            self.r.maybe_unused_trait_imports.insert(directive.id);
            self.r.add_to_glob_map(&directive, trait_name);
            import_ids.push(directive.id);
            kind = &binding.kind;
        };
        import_ids
    }
}

impl<'a> Resolver<'a> {
    pub(crate) fn late_resolve_crate(&mut self, krate: &Crate) {
        let mut late_resolution_visitor = LateResolutionVisitor::new(self);
        visit::walk_crate(&mut late_resolution_visitor, krate);
        for (id, span) in late_resolution_visitor.diagnostic_metadata.unused_labels.iter() {
            self.lint_buffer.buffer_lint(lint::builtin::UNUSED_LABELS, *id, *span, "unused label");
        }
    }
}
