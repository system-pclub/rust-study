//! The Rust abstract syntax tree module.
//!
//! This module contains common structures forming the language AST.
//! Two main entities in the module are [`Item`] (which represents an AST element with
//! additional metadata), and [`ItemKind`] (which represents a concrete type and contains
//! information specific to the type of the item).
//!
//! Other module items that worth mentioning:
//! - [`Ty`] and [`TyKind`]: A parsed Rust type.
//! - [`Expr`] and [`ExprKind`]: A parsed Rust expression.
//! - [`Pat`] and [`PatKind`]: A parsed Rust pattern. Patterns are often dual to expressions.
//! - [`Stmt`] and [`StmtKind`]: An executable action that does not return a value.
//! - [`FnDecl`], [`FnHeader`] and [`Param`]: Metadata associated with a function declaration.
//! - [`Generics`], [`GenericParam`], [`WhereClause`]: Metadata associated with generic parameters.
//! - [`EnumDef`] and [`Variant`]: Enum declaration.
//! - [`Lit`] and [`LitKind`]: Literal expressions.
//! - [`MacroDef`], [`MacStmtStyle`], [`Mac`], [`MacDelimeter`]: Macro definition and invocation.
//! - [`Attribute`]: Metadata associated with item.
//! - [`UnOp`], [`UnOpKind`], [`BinOp`], [`BinOpKind`]: Unary and binary operators.

pub use GenericArgs::*;
pub use UnsafeSource::*;
pub use crate::util::parser::ExprPrecedence;

pub use syntax_pos::symbol::{Ident, Symbol as Name};

use crate::ptr::P;
use crate::source_map::{dummy_spanned, respan, Spanned};
use crate::token::{self, DelimToken};
use crate::tokenstream::TokenStream;

use syntax_pos::symbol::{kw, sym, Symbol};
use syntax_pos::{Span, DUMMY_SP, ExpnId};

use rustc_data_structures::fx::FxHashSet;
use rustc_data_structures::sync::Lrc;
use rustc_data_structures::thin_vec::ThinVec;
use rustc_index::vec::Idx;
use rustc_serialize::{self, Decoder, Encoder};
use rustc_macros::HashStable_Generic;

use std::fmt;

#[cfg(test)]
mod tests;

/// A "Label" is an identifier of some point in sources,
/// e.g. in the following code:
///
/// ```rust
/// 'outer: loop {
///     break 'outer;
/// }
/// ```
///
/// `'outer` is a label.
#[derive(Clone, RustcEncodable, RustcDecodable, Copy)]
pub struct Label {
    pub ident: Ident,
}

impl fmt::Debug for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "label({:?})", self.ident)
    }
}

/// A "Lifetime" is an annotation of the scope in which variable
/// can be used, e.g. `'a` in `&'a i32`.
#[derive(Clone, RustcEncodable, RustcDecodable, Copy)]
pub struct Lifetime {
    pub id: NodeId,
    pub ident: Ident,
}

impl fmt::Debug for Lifetime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "lifetime({}: {})",
            self.id,
            self
        )
    }
}

impl fmt::Display for Lifetime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.ident.name)
    }
}

/// A "Path" is essentially Rust's notion of a name.
///
/// It's represented as a sequence of identifiers,
/// along with a bunch of supporting information.
///
/// E.g., `std::cmp::PartialEq`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Path {
    pub span: Span,
    /// The segments in the path: the things separated by `::`.
    /// Global paths begin with `kw::PathRoot`.
    pub segments: Vec<PathSegment>,
}

impl PartialEq<Symbol> for Path {
    fn eq(&self, symbol: &Symbol) -> bool {
        self.segments.len() == 1 && {
            self.segments[0].ident.name == *symbol
        }
    }
}

impl Path {
    // Convert a span and an identifier to the corresponding
    // one-segment path.
    pub fn from_ident(ident: Ident) -> Path {
        Path {
            segments: vec![PathSegment::from_ident(ident)],
            span: ident.span,
        }
    }

    pub fn is_global(&self) -> bool {
        !self.segments.is_empty() && self.segments[0].ident.name == kw::PathRoot
    }
}

/// A segment of a path: an identifier, an optional lifetime, and a set of types.
///
/// E.g., `std`, `String` or `Box<T>`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct PathSegment {
    /// The identifier portion of this path segment.
    pub ident: Ident,

    pub id: NodeId,

    /// Type/lifetime parameters attached to this path. They come in
    /// two flavors: `Path<A,B,C>` and `Path(A,B) -> C`.
    /// `None` means that no parameter list is supplied (`Path`),
    /// `Some` means that parameter list is supplied (`Path<X, Y>`)
    /// but it can be empty (`Path<>`).
    /// `P` is used as a size optimization for the common case with no parameters.
    pub args: Option<P<GenericArgs>>,
}

impl PathSegment {
    pub fn from_ident(ident: Ident) -> Self {
        PathSegment { ident, id: DUMMY_NODE_ID, args: None }
    }
    pub fn path_root(span: Span) -> Self {
        PathSegment::from_ident(Ident::new(kw::PathRoot, span))
    }
}

/// The arguments of a path segment.
///
/// E.g., `<A, B>` as in `Foo<A, B>` or `(A, B)` as in `Foo(A, B)`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum GenericArgs {
    /// The `<'a, A, B, C>` in `foo::bar::baz::<'a, A, B, C>`.
    AngleBracketed(AngleBracketedArgs),
    /// The `(A, B)` and `C` in `Foo(A, B) -> C`.
    Parenthesized(ParenthesizedArgs),
}

impl GenericArgs {
    pub fn is_parenthesized(&self) -> bool {
        match *self {
            Parenthesized(..) => true,
            _ => false,
        }
    }

    pub fn is_angle_bracketed(&self) -> bool {
        match *self {
            AngleBracketed(..) => true,
            _ => false,
        }
    }

    pub fn span(&self) -> Span {
        match *self {
            AngleBracketed(ref data) => data.span,
            Parenthesized(ref data) => data.span,
        }
    }
}

/// Concrete argument in the sequence of generic args.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum GenericArg {
    /// `'a` in `Foo<'a>`
    Lifetime(Lifetime),
    /// `Bar` in `Foo<Bar>`
    Type(P<Ty>),
    /// `1` in `Foo<1>`
    Const(AnonConst),
}

impl GenericArg {
    pub fn span(&self) -> Span {
        match self {
            GenericArg::Lifetime(lt) => lt.ident.span,
            GenericArg::Type(ty) => ty.span,
            GenericArg::Const(ct) => ct.value.span,
        }
    }
}

/// A path like `Foo<'a, T>`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug, Default)]
pub struct AngleBracketedArgs {
    /// The overall span.
    pub span: Span,
    /// The arguments for this path segment.
    pub args: Vec<GenericArg>,
    /// Constraints on associated types, if any.
    /// E.g., `Foo<A = Bar, B: Baz>`.
    pub constraints: Vec<AssocTyConstraint>,
}

impl Into<Option<P<GenericArgs>>> for AngleBracketedArgs {
    fn into(self) -> Option<P<GenericArgs>> {
        Some(P(GenericArgs::AngleBracketed(self)))
    }
}

impl Into<Option<P<GenericArgs>>> for ParenthesizedArgs {
    fn into(self) -> Option<P<GenericArgs>> {
        Some(P(GenericArgs::Parenthesized(self)))
    }
}

/// A path like `Foo(A, B) -> C`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct ParenthesizedArgs {
    /// Overall span
    pub span: Span,

    /// `(A, B)`
    pub inputs: Vec<P<Ty>>,

    /// `C`
    pub output: Option<P<Ty>>,
}

impl ParenthesizedArgs {
    pub fn as_angle_bracketed_args(&self) -> AngleBracketedArgs {
        AngleBracketedArgs {
            span: self.span,
            args: self.inputs.iter().cloned().map(|input| GenericArg::Type(input)).collect(),
            constraints: vec![],
        }
    }
}

// hack to ensure that we don't try to access the private parts of `NodeId` in this module
mod node_id_inner {
    use rustc_index::vec::Idx;
    rustc_index::newtype_index! {
        pub struct NodeId {
            ENCODABLE = custom
            DEBUG_FORMAT = "NodeId({})"
        }
    }
}

pub use node_id_inner::NodeId;

impl NodeId {
    pub fn placeholder_from_expn_id(expn_id: ExpnId) -> Self {
        NodeId::from_u32(expn_id.as_u32())
    }

    pub fn placeholder_to_expn_id(self) -> ExpnId {
        ExpnId::from_u32(self.as_u32())
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.as_u32(), f)
    }
}

impl rustc_serialize::UseSpecializedEncodable for NodeId {
    fn default_encode<S: Encoder>(&self, s: &mut S) -> Result<(), S::Error> {
        s.emit_u32(self.as_u32())
    }
}

impl rustc_serialize::UseSpecializedDecodable for NodeId {
    fn default_decode<D: Decoder>(d: &mut D) -> Result<NodeId, D::Error> {
        d.read_u32().map(NodeId::from_u32)
    }
}

/// `NodeId` used to represent the root of the crate.
pub const CRATE_NODE_ID: NodeId = NodeId::from_u32_const(0);

/// When parsing and doing expansions, we initially give all AST nodes this AST
/// node value. Then later, in the renumber pass, we renumber them to have
/// small, positive ids.
pub const DUMMY_NODE_ID: NodeId = NodeId::MAX;

/// A modifier on a bound, currently this is only used for `?Sized`, where the
/// modifier is `Maybe`. Negative bounds should also be handled here.
#[derive(Copy, Clone, PartialEq, Eq, RustcEncodable, RustcDecodable, Debug)]
pub enum TraitBoundModifier {
    None,
    Maybe,
}

/// The AST represents all type param bounds as types.
/// `typeck::collect::compute_bounds` matches these against
/// the "special" built-in traits (see `middle::lang_items`) and
/// detects `Copy`, `Send` and `Sync`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum GenericBound {
    Trait(PolyTraitRef, TraitBoundModifier),
    Outlives(Lifetime),
}

impl GenericBound {
    pub fn span(&self) -> Span {
        match self {
            &GenericBound::Trait(ref t, ..) => t.span,
            &GenericBound::Outlives(ref l) => l.ident.span,
        }
    }
}

pub type GenericBounds = Vec<GenericBound>;

/// Specifies the enforced ordering for generic parameters. In the future,
/// if we wanted to relax this order, we could override `PartialEq` and
/// `PartialOrd`, to allow the kinds to be unordered.
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum ParamKindOrd {
    Lifetime,
    Type,
    Const,
}

impl fmt::Display for ParamKindOrd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamKindOrd::Lifetime => "lifetime".fmt(f),
            ParamKindOrd::Type => "type".fmt(f),
            ParamKindOrd::Const => "const".fmt(f),
        }
    }
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum GenericParamKind {
    /// A lifetime definition (e.g., `'a: 'b + 'c + 'd`).
    Lifetime,
    Type { default: Option<P<Ty>> },
    Const { ty: P<Ty> },
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct GenericParam {
    pub id: NodeId,
    pub ident: Ident,
    pub attrs: ThinVec<Attribute>,
    pub bounds: GenericBounds,
    pub is_placeholder: bool,
    pub kind: GenericParamKind,
}

/// Represents lifetime, type and const parameters attached to a declaration of
/// a function, enum, trait, etc.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Generics {
    pub params: Vec<GenericParam>,
    pub where_clause: WhereClause,
    pub span: Span,
}

impl Default for Generics {
    /// Creates an instance of `Generics`.
    fn default() -> Generics {
        Generics {
            params: Vec::new(),
            where_clause: WhereClause {
                predicates: Vec::new(),
                span: DUMMY_SP,
            },
            span: DUMMY_SP,
        }
    }
}

/// A where-clause in a definition.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct WhereClause {
    pub predicates: Vec<WherePredicate>,
    pub span: Span,
}

/// A single predicate in a where-clause.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum WherePredicate {
    /// A type binding (e.g., `for<'c> Foo: Send + Clone + 'c`).
    BoundPredicate(WhereBoundPredicate),
    /// A lifetime predicate (e.g., `'a: 'b + 'c`).
    RegionPredicate(WhereRegionPredicate),
    /// An equality predicate (unsupported).
    EqPredicate(WhereEqPredicate),
}

impl WherePredicate {
    pub fn span(&self) -> Span {
        match self {
            &WherePredicate::BoundPredicate(ref p) => p.span,
            &WherePredicate::RegionPredicate(ref p) => p.span,
            &WherePredicate::EqPredicate(ref p) => p.span,
        }
    }
}

/// A type bound.
///
/// E.g., `for<'c> Foo: Send + Clone + 'c`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct WhereBoundPredicate {
    pub span: Span,
    /// Any generics from a `for` binding.
    pub bound_generic_params: Vec<GenericParam>,
    /// The type being bounded.
    pub bounded_ty: P<Ty>,
    /// Trait and lifetime bounds (`Clone + Send + 'static`).
    pub bounds: GenericBounds,
}

/// A lifetime predicate.
///
/// E.g., `'a: 'b + 'c`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct WhereRegionPredicate {
    pub span: Span,
    pub lifetime: Lifetime,
    pub bounds: GenericBounds,
}

/// An equality predicate (unsupported).
///
/// E.g., `T = int`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct WhereEqPredicate {
    pub id: NodeId,
    pub span: Span,
    pub lhs_ty: P<Ty>,
    pub rhs_ty: P<Ty>,
}

/// The set of `MetaItem`s that define the compilation environment of the crate,
/// used to drive conditional compilation.
pub type CrateConfig = FxHashSet<(Name, Option<Symbol>)>;

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Crate {
    pub module: Mod,
    pub attrs: Vec<Attribute>,
    pub span: Span,
}

/// Possible values inside of compile-time attribute lists.
///
/// E.g., the '..' in `#[name(..)]`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum NestedMetaItem {
    /// A full MetaItem, for recursive meta items.
    MetaItem(MetaItem),
    /// A literal.
    ///
    /// E.g., `"foo"`, `64`, `true`.
    Literal(Lit),
}

/// A spanned compile-time attribute item.
///
/// E.g., `#[test]`, `#[derive(..)]`, `#[rustfmt::skip]` or `#[feature = "foo"]`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct MetaItem {
    pub path: Path,
    pub kind: MetaItemKind,
    pub span: Span,
}

/// A compile-time attribute item.
///
/// E.g., `#[test]`, `#[derive(..)]` or `#[feature = "foo"]`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum MetaItemKind {
    /// Word meta item.
    ///
    /// E.g., `test` as in `#[test]`.
    Word,
    /// List meta item.
    ///
    /// E.g., `derive(..)` as in `#[derive(..)]`.
    List(Vec<NestedMetaItem>),
    /// Name value meta item.
    ///
    /// E.g., `feature = "foo"` as in `#[feature = "foo"]`.
    NameValue(Lit),
}

/// A block (`{ .. }`).
///
/// E.g., `{ .. }` as in `fn foo() { .. }`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Block {
    /// The statements in the block.
    pub stmts: Vec<Stmt>,
    pub id: NodeId,
    /// Distinguishes between `unsafe { ... }` and `{ ... }`.
    pub rules: BlockCheckMode,
    pub span: Span,
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Pat {
    pub id: NodeId,
    pub kind: PatKind,
    pub span: Span,
}

impl Pat {
    /// Attempt reparsing the pattern as a type.
    /// This is intended for use by diagnostics.
    pub fn to_ty(&self) -> Option<P<Ty>> {
        let kind = match &self.kind {
            // In a type expression `_` is an inference variable.
            PatKind::Wild => TyKind::Infer,
            // An IDENT pattern with no binding mode would be valid as path to a type. E.g. `u32`.
            PatKind::Ident(BindingMode::ByValue(Mutability::Immutable), ident, None) => {
                TyKind::Path(None, Path::from_ident(*ident))
            }
            PatKind::Path(qself, path) => TyKind::Path(qself.clone(), path.clone()),
            PatKind::Mac(mac) => TyKind::Mac(mac.clone()),
            // `&mut? P` can be reinterpreted as `&mut? T` where `T` is `P` reparsed as a type.
            PatKind::Ref(pat, mutbl) => pat
                .to_ty()
                .map(|ty| TyKind::Rptr(None, MutTy { ty, mutbl: *mutbl }))?,
            // A slice/array pattern `[P]` can be reparsed as `[T]`, an unsized array,
            // when `P` can be reparsed as a type `T`.
            PatKind::Slice(pats) if pats.len() == 1 => pats[0].to_ty().map(TyKind::Slice)?,
            // A tuple pattern `(P0, .., Pn)` can be reparsed as `(T0, .., Tn)`
            // assuming `T0` to `Tn` are all syntactically valid as types.
            PatKind::Tuple(pats) => {
                let mut tys = Vec::with_capacity(pats.len());
                // FIXME(#48994) - could just be collected into an Option<Vec>
                for pat in pats {
                    tys.push(pat.to_ty()?);
                }
                TyKind::Tup(tys)
            }
            _ => return None,
        };

        Some(P(Ty {
            kind,
            id: self.id,
            span: self.span,
        }))
    }

    /// Walk top-down and call `it` in each place where a pattern occurs
    /// starting with the root pattern `walk` is called on. If `it` returns
    /// false then we will descend no further but siblings will be processed.
    pub fn walk(&self, it: &mut impl FnMut(&Pat) -> bool) {
        if !it(self) {
            return;
        }

        match &self.kind {
            // Walk into the pattern associated with `Ident` (if any).
            PatKind::Ident(_, _, Some(p)) => p.walk(it),

            // Walk into each field of struct.
            PatKind::Struct(_, fields, _) => fields.iter().for_each(|field| field.pat.walk(it)),

            // Sequence of patterns.
            PatKind::TupleStruct(_, s)
            | PatKind::Tuple(s)
            | PatKind::Slice(s)
            | PatKind::Or(s) => s.iter().for_each(|p| p.walk(it)),

            // Trivial wrappers over inner patterns.
            PatKind::Box(s)
            | PatKind::Ref(s, _)
            | PatKind::Paren(s) => s.walk(it),

            // These patterns do not contain subpatterns, skip.
            PatKind::Wild
            | PatKind::Rest
            | PatKind::Lit(_)
            | PatKind::Range(..)
            | PatKind::Ident(..)
            | PatKind::Path(..)
            | PatKind::Mac(_) => {},
        }
    }

    /// Is this a `..` pattern?
    pub fn is_rest(&self) -> bool {
        match self.kind {
            PatKind::Rest => true,
            _ => false,
        }
    }
}

/// A single field in a struct pattern
///
/// Patterns like the fields of Foo `{ x, ref y, ref mut z }`
/// are treated the same as` x: x, y: ref y, z: ref mut z`,
/// except is_shorthand is true
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct FieldPat {
    /// The identifier for the field
    pub ident: Ident,
    /// The pattern the field is destructured to
    pub pat: P<Pat>,
    pub is_shorthand: bool,
    pub attrs: ThinVec<Attribute>,
    pub id: NodeId,
    pub span: Span,
    pub is_placeholder: bool,
}

#[derive(Clone, PartialEq, RustcEncodable, RustcDecodable, Debug, Copy)]
pub enum BindingMode {
    ByRef(Mutability),
    ByValue(Mutability),
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum RangeEnd {
    Included(RangeSyntax),
    Excluded,
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum RangeSyntax {
    /// `...`
    DotDotDot,
    /// `..=`
    DotDotEq,
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum PatKind {
    /// Represents a wildcard pattern (`_`).
    Wild,

    /// A `PatKind::Ident` may either be a new bound variable (`ref mut binding @ OPT_SUBPATTERN`),
    /// or a unit struct/variant pattern, or a const pattern (in the last two cases the third
    /// field must be `None`). Disambiguation cannot be done with parser alone, so it happens
    /// during name resolution.
    Ident(BindingMode, Ident, Option<P<Pat>>),

    /// A struct or struct variant pattern (e.g., `Variant {x, y, ..}`).
    /// The `bool` is `true` in the presence of a `..`.
    Struct(Path, Vec<FieldPat>, /* recovered */ bool),

    /// A tuple struct/variant pattern (`Variant(x, y, .., z)`).
    TupleStruct(Path, Vec<P<Pat>>),

    /// An or-pattern `A | B | C`.
    /// Invariant: `pats.len() >= 2`.
    Or(Vec<P<Pat>>),

    /// A possibly qualified path pattern.
    /// Unqualified path patterns `A::B::C` can legally refer to variants, structs, constants
    /// or associated constants. Qualified path patterns `<A>::B::C`/`<A as Trait>::B::C` can
    /// only legally refer to associated constants.
    Path(Option<QSelf>, Path),

    /// A tuple pattern (`(a, b)`).
    Tuple(Vec<P<Pat>>),

    /// A `box` pattern.
    Box(P<Pat>),

    /// A reference pattern (e.g., `&mut (a, b)`).
    Ref(P<Pat>, Mutability),

    /// A literal.
    Lit(P<Expr>),

    /// A range pattern (e.g., `1...2`, `1..=2` or `1..2`).
    Range(P<Expr>, P<Expr>, Spanned<RangeEnd>),

    /// A slice pattern `[a, b, c]`.
    Slice(Vec<P<Pat>>),

    /// A rest pattern `..`.
    ///
    /// Syntactically it is valid anywhere.
    ///
    /// Semantically however, it only has meaning immediately inside:
    /// - a slice pattern: `[a, .., b]`,
    /// - a binding pattern immediately inside a slice pattern: `[a, r @ ..]`,
    /// - a tuple pattern: `(a, .., b)`,
    /// - a tuple struct/variant pattern: `$path(a, .., b)`.
    ///
    /// In all of these cases, an additional restriction applies,
    /// only one rest pattern may occur in the pattern sequences.
    Rest,

    /// Parentheses in patterns used for grouping (i.e., `(PAT)`).
    Paren(P<Pat>),

    /// A macro pattern; pre-expansion.
    Mac(Mac),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash,
         RustcEncodable, RustcDecodable, Debug, Copy, HashStable_Generic)]
pub enum Mutability {
    Mutable,
    Immutable,
}

impl Mutability {
    /// Returns `MutMutable` only if both `self` and `other` are mutable.
    pub fn and(self, other: Self) -> Self {
        match self {
            Mutability::Mutable => other,
            Mutability::Immutable => Mutability::Immutable,
        }
    }

    pub fn invert(self) -> Self {
        match self {
            Mutability::Mutable => Mutability::Immutable,
            Mutability::Immutable => Mutability::Mutable,
        }
    }

    pub fn prefix_str(&self) -> &'static str {
        match self {
            Mutability::Mutable => "mut ",
            Mutability::Immutable => "",
        }
    }
}

#[derive(Clone, PartialEq, RustcEncodable, RustcDecodable, Debug, Copy)]
pub enum BinOpKind {
    /// The `+` operator (addition)
    Add,
    /// The `-` operator (subtraction)
    Sub,
    /// The `*` operator (multiplication)
    Mul,
    /// The `/` operator (division)
    Div,
    /// The `%` operator (modulus)
    Rem,
    /// The `&&` operator (logical and)
    And,
    /// The `||` operator (logical or)
    Or,
    /// The `^` operator (bitwise xor)
    BitXor,
    /// The `&` operator (bitwise and)
    BitAnd,
    /// The `|` operator (bitwise or)
    BitOr,
    /// The `<<` operator (shift left)
    Shl,
    /// The `>>` operator (shift right)
    Shr,
    /// The `==` operator (equality)
    Eq,
    /// The `<` operator (less than)
    Lt,
    /// The `<=` operator (less than or equal to)
    Le,
    /// The `!=` operator (not equal to)
    Ne,
    /// The `>=` operator (greater than or equal to)
    Ge,
    /// The `>` operator (greater than)
    Gt,
}

impl BinOpKind {
    pub fn to_string(&self) -> &'static str {
        use BinOpKind::*;
        match *self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Rem => "%",
            And => "&&",
            Or => "||",
            BitXor => "^",
            BitAnd => "&",
            BitOr => "|",
            Shl => "<<",
            Shr => ">>",
            Eq => "==",
            Lt => "<",
            Le => "<=",
            Ne => "!=",
            Ge => ">=",
            Gt => ">",
        }
    }
    pub fn lazy(&self) -> bool {
        match *self {
            BinOpKind::And | BinOpKind::Or => true,
            _ => false,
        }
    }

    pub fn is_shift(&self) -> bool {
        match *self {
            BinOpKind::Shl | BinOpKind::Shr => true,
            _ => false,
        }
    }

    pub fn is_comparison(&self) -> bool {
        use BinOpKind::*;
        // Note for developers: please keep this as is;
        // we want compilation to fail if another variant is added.
        match *self {
            Eq | Lt | Le | Ne | Gt | Ge => true,
            And | Or | Add | Sub | Mul | Div | Rem | BitXor | BitAnd | BitOr | Shl | Shr => false,
        }
    }

    /// Returns `true` if the binary operator takes its arguments by value
    pub fn is_by_value(&self) -> bool {
        !self.is_comparison()
    }
}

pub type BinOp = Spanned<BinOpKind>;

/// Unary operator.
///
/// Note that `&data` is not an operator, it's an `AddrOf` expression.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug, Copy)]
pub enum UnOp {
    /// The `*` operator for dereferencing
    Deref,
    /// The `!` operator for logical inversion
    Not,
    /// The `-` operator for negation
    Neg,
}

impl UnOp {
    /// Returns `true` if the unary operator takes its argument by value
    pub fn is_by_value(u: UnOp) -> bool {
        match u {
            UnOp::Neg | UnOp::Not => true,
            _ => false,
        }
    }

    pub fn to_string(op: UnOp) -> &'static str {
        match op {
            UnOp::Deref => "*",
            UnOp::Not => "!",
            UnOp::Neg => "-",
        }
    }
}

/// A statement
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Stmt {
    pub id: NodeId,
    pub kind: StmtKind,
    pub span: Span,
}

impl Stmt {
    pub fn add_trailing_semicolon(mut self) -> Self {
        self.kind = match self.kind {
            StmtKind::Expr(expr) => StmtKind::Semi(expr),
            StmtKind::Mac(mac) => {
                StmtKind::Mac(mac.map(|(mac, _style, attrs)| (mac, MacStmtStyle::Semicolon, attrs)))
            }
            kind => kind,
        };
        self
    }

    pub fn is_item(&self) -> bool {
        match self.kind {
            StmtKind::Item(_) => true,
            _ => false,
        }
    }

    pub fn is_expr(&self) -> bool {
        match self.kind {
            StmtKind::Expr(_) => true,
            _ => false,
        }
    }
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum StmtKind {
    /// A local (let) binding.
    Local(P<Local>),
    /// An item definition.
    Item(P<Item>),
    /// Expr without trailing semi-colon.
    Expr(P<Expr>),
    /// Expr with a trailing semi-colon.
    Semi(P<Expr>),
    /// Macro.
    Mac(P<(Mac, MacStmtStyle, ThinVec<Attribute>)>),
}

#[derive(Clone, Copy, PartialEq, RustcEncodable, RustcDecodable, Debug)]
pub enum MacStmtStyle {
    /// The macro statement had a trailing semicolon (e.g., `foo! { ... };`
    /// `foo!(...);`, `foo![...];`).
    Semicolon,
    /// The macro statement had braces (e.g., `foo! { ... }`).
    Braces,
    /// The macro statement had parentheses or brackets and no semicolon (e.g.,
    /// `foo!(...)`). All of these will end up being converted into macro
    /// expressions.
    NoBraces,
}

/// Local represents a `let` statement, e.g., `let <pat>:<ty> = <expr>;`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Local {
    pub id: NodeId,
    pub pat: P<Pat>,
    pub ty: Option<P<Ty>>,
    /// Initializer expression to set the value, if any.
    pub init: Option<P<Expr>>,
    pub span: Span,
    pub attrs: ThinVec<Attribute>,
}

/// An arm of a 'match'.
///
/// E.g., `0..=10 => { println!("match!") }` as in
///
/// ```
/// match 123 {
///     0..=10 => { println!("match!") },
///     _ => { println!("no match!") },
/// }
/// ```
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Arm {
    pub attrs: Vec<Attribute>,
    /// Match arm pattern, e.g. `10` in `match foo { 10 => {}, _ => {} }`
    pub pat: P<Pat>,
    /// Match arm guard, e.g. `n > 10` in `match foo { n if n > 10 => {}, _ => {} }`
    pub guard: Option<P<Expr>>,
    /// Match arm body.
    pub body: P<Expr>,
    pub span: Span,
    pub id: NodeId,
    pub is_placeholder: bool,
}

/// Access of a named (e.g., `obj.foo`) or unnamed (e.g., `obj.0`) struct field.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Field {
    pub attrs: ThinVec<Attribute>,
    pub id: NodeId,
    pub span: Span,
    pub ident: Ident,
    pub expr: P<Expr>,
    pub is_shorthand: bool,
    pub is_placeholder: bool,
}

#[derive(Clone, PartialEq, RustcEncodable, RustcDecodable, Debug, Copy)]
pub enum BlockCheckMode {
    Default,
    Unsafe(UnsafeSource),
}

#[derive(Clone, PartialEq, RustcEncodable, RustcDecodable, Debug, Copy)]
pub enum UnsafeSource {
    CompilerGenerated,
    UserProvided,
}

/// A constant (expression) that's not an item or associated item,
/// but needs its own `DefId` for type-checking, const-eval, etc.
/// These are usually found nested inside types (e.g., array lengths)
/// or expressions (e.g., repeat counts), and also used to define
/// explicit discriminant values for enum variants.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct AnonConst {
    pub id: NodeId,
    pub value: P<Expr>,
}

/// An expression.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
    pub attrs: ThinVec<Attribute>,
}

// `Expr` is used a lot. Make sure it doesn't unintentionally get bigger.
#[cfg(target_arch = "x86_64")]
rustc_data_structures::static_assert_size!(Expr, 96);

impl Expr {
    /// Returns `true` if this expression would be valid somewhere that expects a value;
    /// for example, an `if` condition.
    pub fn returns(&self) -> bool {
        if let ExprKind::Block(ref block, _) = self.kind {
            match block.stmts.last().map(|last_stmt| &last_stmt.kind) {
                // Implicit return
                Some(&StmtKind::Expr(_)) => true,
                Some(&StmtKind::Semi(ref expr)) => {
                    if let ExprKind::Ret(_) = expr.kind {
                        // Last statement is explicit return.
                        true
                    } else {
                        false
                    }
                }
                // This is a block that doesn't end in either an implicit or explicit return.
                _ => false,
            }
        } else {
            // This is not a block, it is a value.
            true
        }
    }

    pub fn to_bound(&self) -> Option<GenericBound> {
        match &self.kind {
            ExprKind::Path(None, path) => Some(GenericBound::Trait(
                PolyTraitRef::new(Vec::new(), path.clone(), self.span),
                TraitBoundModifier::None,
            )),
            _ => None,
        }
    }

    /// Attempts to reparse as `Ty` (for diagnostic purposes).
    pub fn to_ty(&self) -> Option<P<Ty>> {
        let kind = match &self.kind {
            // Trivial conversions.
            ExprKind::Path(qself, path) => TyKind::Path(qself.clone(), path.clone()),
            ExprKind::Mac(mac) => TyKind::Mac(mac.clone()),

            ExprKind::Paren(expr) => expr.to_ty().map(TyKind::Paren)?,

            ExprKind::AddrOf(mutbl, expr) => expr
                .to_ty()
                .map(|ty| TyKind::Rptr(None, MutTy { ty, mutbl: *mutbl }))?,

            ExprKind::Repeat(expr, expr_len) => {
                expr.to_ty().map(|ty| TyKind::Array(ty, expr_len.clone()))?
            }

            ExprKind::Array(exprs) if exprs.len() == 1 => exprs[0].to_ty().map(TyKind::Slice)?,

            ExprKind::Tup(exprs) => {
                let tys = exprs
                    .iter()
                    .map(|expr| expr.to_ty())
                    .collect::<Option<Vec<_>>>()?;
                TyKind::Tup(tys)
            }

            // If binary operator is `Add` and both `lhs` and `rhs` are trait bounds,
            // then type of result is trait object.
            // Othewise we don't assume the result type.
            ExprKind::Binary(binop, lhs, rhs) if binop.node == BinOpKind::Add => {
                if let (Some(lhs), Some(rhs)) = (lhs.to_bound(), rhs.to_bound()) {
                    TyKind::TraitObject(vec![lhs, rhs], TraitObjectSyntax::None)
                } else {
                    return None;
                }
            }

            // This expression doesn't look like a type syntactically.
            _ => return None,
        };

        Some(P(Ty {
            kind,
            id: self.id,
            span: self.span,
        }))
    }

    pub fn precedence(&self) -> ExprPrecedence {
        match self.kind {
            ExprKind::Box(_) => ExprPrecedence::Box,
            ExprKind::Array(_) => ExprPrecedence::Array,
            ExprKind::Call(..) => ExprPrecedence::Call,
            ExprKind::MethodCall(..) => ExprPrecedence::MethodCall,
            ExprKind::Tup(_) => ExprPrecedence::Tup,
            ExprKind::Binary(op, ..) => ExprPrecedence::Binary(op.node),
            ExprKind::Unary(..) => ExprPrecedence::Unary,
            ExprKind::Lit(_) => ExprPrecedence::Lit,
            ExprKind::Type(..) | ExprKind::Cast(..) => ExprPrecedence::Cast,
            ExprKind::Let(..) => ExprPrecedence::Let,
            ExprKind::If(..) => ExprPrecedence::If,
            ExprKind::While(..) => ExprPrecedence::While,
            ExprKind::ForLoop(..) => ExprPrecedence::ForLoop,
            ExprKind::Loop(..) => ExprPrecedence::Loop,
            ExprKind::Match(..) => ExprPrecedence::Match,
            ExprKind::Closure(..) => ExprPrecedence::Closure,
            ExprKind::Block(..) => ExprPrecedence::Block,
            ExprKind::TryBlock(..) => ExprPrecedence::TryBlock,
            ExprKind::Async(..) => ExprPrecedence::Async,
            ExprKind::Await(..) => ExprPrecedence::Await,
            ExprKind::Assign(..) => ExprPrecedence::Assign,
            ExprKind::AssignOp(..) => ExprPrecedence::AssignOp,
            ExprKind::Field(..) => ExprPrecedence::Field,
            ExprKind::Index(..) => ExprPrecedence::Index,
            ExprKind::Range(..) => ExprPrecedence::Range,
            ExprKind::Path(..) => ExprPrecedence::Path,
            ExprKind::AddrOf(..) => ExprPrecedence::AddrOf,
            ExprKind::Break(..) => ExprPrecedence::Break,
            ExprKind::Continue(..) => ExprPrecedence::Continue,
            ExprKind::Ret(..) => ExprPrecedence::Ret,
            ExprKind::InlineAsm(..) => ExprPrecedence::InlineAsm,
            ExprKind::Mac(..) => ExprPrecedence::Mac,
            ExprKind::Struct(..) => ExprPrecedence::Struct,
            ExprKind::Repeat(..) => ExprPrecedence::Repeat,
            ExprKind::Paren(..) => ExprPrecedence::Paren,
            ExprKind::Try(..) => ExprPrecedence::Try,
            ExprKind::Yield(..) => ExprPrecedence::Yield,
            ExprKind::Err => ExprPrecedence::Err,
        }
    }
}

/// Limit types of a range (inclusive or exclusive)
#[derive(Copy, Clone, PartialEq, RustcEncodable, RustcDecodable, Debug)]
pub enum RangeLimits {
    /// Inclusive at the beginning, exclusive at the end
    HalfOpen,
    /// Inclusive at the beginning and end
    Closed,
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum ExprKind {
    /// A `box x` expression.
    Box(P<Expr>),
    /// An array (`[a, b, c, d]`)
    Array(Vec<P<Expr>>),
    /// A function call
    ///
    /// The first field resolves to the function itself,
    /// and the second field is the list of arguments.
    /// This also represents calling the constructor of
    /// tuple-like ADTs such as tuple structs and enum variants.
    Call(P<Expr>, Vec<P<Expr>>),
    /// A method call (`x.foo::<'static, Bar, Baz>(a, b, c, d)`)
    ///
    /// The `PathSegment` represents the method name and its generic arguments
    /// (within the angle brackets).
    /// The first element of the vector of an `Expr` is the expression that evaluates
    /// to the object on which the method is being called on (the receiver),
    /// and the remaining elements are the rest of the arguments.
    /// Thus, `x.foo::<Bar, Baz>(a, b, c, d)` is represented as
    /// `ExprKind::MethodCall(PathSegment { foo, [Bar, Baz] }, [x, a, b, c, d])`.
    MethodCall(PathSegment, Vec<P<Expr>>),
    /// A tuple (e.g., `(a, b, c, d)`).
    Tup(Vec<P<Expr>>),
    /// A binary operation (e.g., `a + b`, `a * b`).
    Binary(BinOp, P<Expr>, P<Expr>),
    /// A unary operation (e.g., `!x`, `*x`).
    Unary(UnOp, P<Expr>),
    /// A literal (e.g., `1`, `"foo"`).
    Lit(Lit),
    /// A cast (e.g., `foo as f64`).
    Cast(P<Expr>, P<Ty>),
    /// A type ascription (e.g., `42: usize`).
    Type(P<Expr>, P<Ty>),
    /// A `let pat = expr` expression that is only semantically allowed in the condition
    /// of `if` / `while` expressions. (e.g., `if let 0 = x { .. }`).
    Let(P<Pat>, P<Expr>),
    /// An `if` block, with an optional `else` block.
    ///
    /// `if expr { block } else { expr }`
    If(P<Expr>, P<Block>, Option<P<Expr>>),
    /// A while loop, with an optional label.
    ///
    /// `'label: while expr { block }`
    While(P<Expr>, P<Block>, Option<Label>),
    /// A `for` loop, with an optional label.
    ///
    /// `'label: for pat in expr { block }`
    ///
    /// This is desugared to a combination of `loop` and `match` expressions.
    ForLoop(P<Pat>, P<Expr>, P<Block>, Option<Label>),
    /// Conditionless loop (can be exited with `break`, `continue`, or `return`).
    ///
    /// `'label: loop { block }`
    Loop(P<Block>, Option<Label>),
    /// A `match` block.
    Match(P<Expr>, Vec<Arm>),
    /// A closure (e.g., `move |a, b, c| a + b + c`).
    ///
    /// The final span is the span of the argument block `|...|`.
    Closure(CaptureBy, IsAsync, Movability, P<FnDecl>, P<Expr>, Span),
    /// A block (`'label: { ... }`).
    Block(P<Block>, Option<Label>),
    /// An async block (`async move { ... }`).
    ///
    /// The `NodeId` is the `NodeId` for the closure that results from
    /// desugaring an async block, just like the NodeId field in the
    /// `IsAsync` enum. This is necessary in order to create a def for the
    /// closure which can be used as a parent of any child defs. Defs
    /// created during lowering cannot be made the parent of any other
    /// preexisting defs.
    Async(CaptureBy, NodeId, P<Block>),
    /// An await expression (`my_future.await`).
    Await(P<Expr>),

    /// A try block (`try { ... }`).
    TryBlock(P<Block>),

    /// An assignment (`a = foo()`).
    Assign(P<Expr>, P<Expr>),
    /// An assignment with an operator.
    ///
    /// E.g., `a += 1`.
    AssignOp(BinOp, P<Expr>, P<Expr>),
    /// Access of a named (e.g., `obj.foo`) or unnamed (e.g., `obj.0`) struct field.
    Field(P<Expr>, Ident),
    /// An indexing operation (e.g., `foo[2]`).
    Index(P<Expr>, P<Expr>),
    /// A range (e.g., `1..2`, `1..`, `..2`, `1..=2`, `..=2`).
    Range(Option<P<Expr>>, Option<P<Expr>>, RangeLimits),

    /// Variable reference, possibly containing `::` and/or type
    /// parameters (e.g., `foo::bar::<baz>`).
    ///
    /// Optionally "qualified" (e.g., `<Vec<T> as SomeTrait>::SomeType`).
    Path(Option<QSelf>, Path),

    /// A referencing operation (`&a` or `&mut a`).
    AddrOf(Mutability, P<Expr>),
    /// A `break`, with an optional label to break, and an optional expression.
    Break(Option<Label>, Option<P<Expr>>),
    /// A `continue`, with an optional label.
    Continue(Option<Label>),
    /// A `return`, with an optional value to be returned.
    Ret(Option<P<Expr>>),

    /// Output of the `asm!()` macro.
    InlineAsm(P<InlineAsm>),

    /// A macro invocation; pre-expansion.
    Mac(Mac),

    /// A struct literal expression.
    ///
    /// E.g., `Foo {x: 1, y: 2}`, or `Foo {x: 1, .. base}`,
    /// where `base` is the `Option<Expr>`.
    Struct(Path, Vec<Field>, Option<P<Expr>>),

    /// An array literal constructed from one repeated element.
    ///
    /// E.g., `[1; 5]`. The expression is the element to be
    /// repeated; the constant is the number of times to repeat it.
    Repeat(P<Expr>, AnonConst),

    /// No-op: used solely so we can pretty-print faithfully.
    Paren(P<Expr>),

    /// A try expression (`expr?`).
    Try(P<Expr>),

    /// A `yield`, with an optional value to be yielded.
    Yield(Option<P<Expr>>),

    /// Placeholder for an expression that wasn't syntactically well formed in some way.
    Err,
}

/// The explicit `Self` type in a "qualified path". The actual
/// path, including the trait and the associated item, is stored
/// separately. `position` represents the index of the associated
/// item qualified with this `Self` type.
///
/// ```ignore (only-for-syntax-highlight)
/// <Vec<T> as a::b::Trait>::AssociatedItem
///  ^~~~~     ~~~~~~~~~~~~~~^
///  ty        position = 3
///
/// <Vec<T>>::AssociatedItem
///  ^~~~~    ^
///  ty       position = 0
/// ```
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct QSelf {
    pub ty: P<Ty>,

    /// The span of `a::b::Trait` in a path like `<Vec<T> as
    /// a::b::Trait>::AssociatedItem`; in the case where `position ==
    /// 0`, this is an empty span.
    pub path_span: Span,
    pub position: usize,
}

/// A capture clause used in closures and `async` blocks.
#[derive(Clone, Copy, PartialEq, RustcEncodable, RustcDecodable, Debug, HashStable_Generic)]
pub enum CaptureBy {
    /// `move |x| y + x`.
    Value,
    /// `move` keyword was not specified.
    Ref,
}

/// The movability of a generator / closure literal:
/// whether a generator contains self-references, causing it to be `!Unpin`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash,
         RustcEncodable, RustcDecodable, Debug, Copy, HashStable_Generic)]
pub enum Movability {
    /// May contain self-references, `!Unpin`.
    Static,
    /// Must not contain self-references, `Unpin`.
    Movable,
}

/// Represents a macro invocation. The `Path` indicates which macro
/// is being invoked, and the vector of token-trees contains the source
/// of the macro invocation.
///
/// N.B., the additional ident for a `macro_rules`-style macro is actually
/// stored in the enclosing item.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Mac {
    pub path: Path,
    pub delim: MacDelimiter,
    pub tts: TokenStream,
    pub span: Span,
    pub prior_type_ascription: Option<(Span, bool)>,
}

#[derive(Copy, Clone, PartialEq, Eq, RustcEncodable, RustcDecodable, Debug)]
pub enum MacDelimiter {
    Parenthesis,
    Bracket,
    Brace,
}

impl Mac {
    pub fn stream(&self) -> TokenStream {
        self.tts.clone()
    }
}

impl MacDelimiter {
    crate fn to_token(self) -> DelimToken {
        match self {
            MacDelimiter::Parenthesis => DelimToken::Paren,
            MacDelimiter::Bracket => DelimToken::Bracket,
            MacDelimiter::Brace => DelimToken::Brace,
        }
    }
}

/// Represents a macro definition.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct MacroDef {
    pub tokens: TokenStream,
    /// `true` if macro was defined with `macro_rules`.
    pub legacy: bool,
}

impl MacroDef {
    pub fn stream(&self) -> TokenStream {
        self.tokens.clone().into()
    }
}

// Clippy uses Hash and PartialEq
#[derive(Clone, RustcEncodable, RustcDecodable, Debug, Copy, Hash, PartialEq, HashStable_Generic)]
pub enum StrStyle {
    /// A regular string, like `"foo"`.
    Cooked,
    /// A raw string, like `r##"foo"##`.
    ///
    /// The value is the number of `#` symbols used.
    Raw(u16),
}

/// An AST literal.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Lit {
    /// The original literal token as written in source code.
    pub token: token::Lit,
    /// The "semantic" representation of the literal lowered from the original tokens.
    /// Strings are unescaped, hexadecimal forms are eliminated, etc.
    /// FIXME: Remove this and only create the semantic representation during lowering to HIR.
    pub kind: LitKind,
    pub span: Span,
}

/// Same as `Lit`, but restricted to string literals.
#[derive(Clone, Copy, RustcEncodable, RustcDecodable, Debug)]
pub struct StrLit {
    /// The original literal token as written in source code.
    pub style: StrStyle,
    pub symbol: Symbol,
    pub suffix: Option<Symbol>,
    pub span: Span,
    /// The unescaped "semantic" representation of the literal lowered from the original token.
    /// FIXME: Remove this and only create the semantic representation during lowering to HIR.
    pub symbol_unescaped: Symbol,
}

impl StrLit {
    crate fn as_lit(&self) -> Lit {
        let token_kind = match self.style {
            StrStyle::Cooked => token::Str,
            StrStyle::Raw(n) => token::StrRaw(n),
        };
        Lit {
            token: token::Lit::new(token_kind, self.symbol, self.suffix),
            span: self.span,
            kind: LitKind::Str(self.symbol_unescaped, self.style),
        }
    }
}

// Clippy uses Hash and PartialEq
/// Type of the integer literal based on provided suffix.
#[derive(Clone, Copy, RustcEncodable, RustcDecodable, Debug, Hash, PartialEq, HashStable_Generic)]
pub enum LitIntType {
    /// e.g. `42_i32`.
    Signed(IntTy),
    /// e.g. `42_u32`.
    Unsigned(UintTy),
    /// e.g. `42`.
    Unsuffixed,
}

/// Type of the float literal based on provided suffix.
#[derive(Clone, Copy, RustcEncodable, RustcDecodable, Debug, Hash, PartialEq, HashStable_Generic)]
pub enum LitFloatType {
    /// A float literal with a suffix (`1f32` or `1E10f32`).
    Suffixed(FloatTy),
    /// A float literal without a suffix (`1.0 or 1.0E10`).
    Unsuffixed,
}

/// Literal kind.
///
/// E.g., `"foo"`, `42`, `12.34`, or `bool`.
// Clippy uses Hash and PartialEq
#[derive(Clone, RustcEncodable, RustcDecodable, Debug, Hash, PartialEq, HashStable_Generic)]
pub enum LitKind {
    /// A string literal (`"foo"`).
    Str(Symbol, StrStyle),
    /// A byte string (`b"foo"`).
    ByteStr(Lrc<Vec<u8>>),
    /// A byte char (`b'f'`).
    Byte(u8),
    /// A character literal (`'a'`).
    Char(char),
    /// An integer literal (`1`).
    Int(u128, LitIntType),
    /// A float literal (`1f64` or `1E10f64`).
    Float(Symbol, LitFloatType),
    /// A boolean literal.
    Bool(bool),
    /// Placeholder for a literal that wasn't well-formed in some way.
    Err(Symbol),
}

impl LitKind {
    /// Returns `true` if this literal is a string.
    pub fn is_str(&self) -> bool {
        match *self {
            LitKind::Str(..) => true,
            _ => false,
        }
    }

    /// Returns `true` if this literal is byte literal string.
    pub fn is_bytestr(&self) -> bool {
        match self {
            LitKind::ByteStr(_) => true,
            _ => false,
        }
    }

    /// Returns `true` if this is a numeric literal.
    pub fn is_numeric(&self) -> bool {
        match *self {
            LitKind::Int(..) | LitKind::Float(..) => true,
            _ => false,
        }
    }

    /// Returns `true` if this literal has no suffix.
    /// Note: this will return true for literals with prefixes such as raw strings and byte strings.
    pub fn is_unsuffixed(&self) -> bool {
        !self.is_suffixed()
    }

    /// Returns `true` if this literal has a suffix.
    pub fn is_suffixed(&self) -> bool {
        match *self {
            // suffixed variants
            LitKind::Int(_, LitIntType::Signed(..))
            | LitKind::Int(_, LitIntType::Unsigned(..))
            | LitKind::Float(_, LitFloatType::Suffixed(..)) => true,
            // unsuffixed variants
            LitKind::Str(..)
            | LitKind::ByteStr(..)
            | LitKind::Byte(..)
            | LitKind::Char(..)
            | LitKind::Int(_, LitIntType::Unsuffixed)
            | LitKind::Float(_, LitFloatType::Unsuffixed)
            | LitKind::Bool(..)
            | LitKind::Err(..) => false,
        }
    }
}

// N.B., If you change this, you'll probably want to change the corresponding
// type structure in `middle/ty.rs` as well.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct MutTy {
    pub ty: P<Ty>,
    pub mutbl: Mutability,
}

/// Represents a function's signature in a trait declaration,
/// trait implementation, or free function.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct FnSig {
    pub header: FnHeader,
    pub decl: P<FnDecl>,
}

/// Represents an item declaration within a trait declaration,
/// possibly including a default implementation. A trait item is
/// either required (meaning it doesn't have an implementation, just a
/// signature) or provided (meaning it has a default implementation).
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct TraitItem {
    pub attrs: Vec<Attribute>,
    pub id: NodeId,
    pub span: Span,
    pub vis: Visibility,
    pub ident: Ident,

    pub generics: Generics,
    pub kind: TraitItemKind,
    /// See `Item::tokens` for what this is.
    pub tokens: Option<TokenStream>,
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum TraitItemKind {
    Const(P<Ty>, Option<P<Expr>>),
    Method(FnSig, Option<P<Block>>),
    Type(GenericBounds, Option<P<Ty>>),
    Macro(Mac),
}

/// Represents anything within an `impl` block.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct ImplItem {
    pub attrs: Vec<Attribute>,
    pub id: NodeId,
    pub span: Span,
    pub vis: Visibility,
    pub ident: Ident,

    pub defaultness: Defaultness,
    pub generics: Generics,
    pub kind: ImplItemKind,
    /// See `Item::tokens` for what this is.
    pub tokens: Option<TokenStream>,
}

/// Represents various kinds of content within an `impl`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum ImplItemKind {
    Const(P<Ty>, P<Expr>),
    Method(FnSig, P<Block>),
    TyAlias(P<Ty>),
    Macro(Mac),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, HashStable_Generic,
         RustcEncodable, RustcDecodable, Debug)]
pub enum FloatTy {
    F32,
    F64,
}

impl FloatTy {
    pub fn name_str(self) -> &'static str {
        match self {
            FloatTy::F32 => "f32",
            FloatTy::F64 => "f64",
        }
    }

    pub fn name(self) -> Symbol {
        match self {
            FloatTy::F32 => sym::f32,
            FloatTy::F64 => sym::f64,
        }
    }

    pub fn bit_width(self) -> usize {
        match self {
            FloatTy::F32 => 32,
            FloatTy::F64 => 64,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, HashStable_Generic,
         RustcEncodable, RustcDecodable, Debug)]
pub enum IntTy {
    Isize,
    I8,
    I16,
    I32,
    I64,
    I128,
}

impl IntTy {
    pub fn name_str(&self) -> &'static str {
        match *self {
            IntTy::Isize => "isize",
            IntTy::I8 => "i8",
            IntTy::I16 => "i16",
            IntTy::I32 => "i32",
            IntTy::I64 => "i64",
            IntTy::I128 => "i128",
        }
    }

    pub fn name(&self) -> Symbol {
        match *self {
            IntTy::Isize => sym::isize,
            IntTy::I8 => sym::i8,
            IntTy::I16 => sym::i16,
            IntTy::I32 => sym::i32,
            IntTy::I64 => sym::i64,
            IntTy::I128 => sym::i128,
        }
    }

    pub fn val_to_string(&self, val: i128) -> String {
        // Cast to a `u128` so we can correctly print `INT128_MIN`. All integral types
        // are parsed as `u128`, so we wouldn't want to print an extra negative
        // sign.
        format!("{}{}", val as u128, self.name_str())
    }

    pub fn bit_width(&self) -> Option<usize> {
        Some(match *self {
            IntTy::Isize => return None,
            IntTy::I8 => 8,
            IntTy::I16 => 16,
            IntTy::I32 => 32,
            IntTy::I64 => 64,
            IntTy::I128 => 128,
        })
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, HashStable_Generic,
         RustcEncodable, RustcDecodable, Copy, Debug)]
pub enum UintTy {
    Usize,
    U8,
    U16,
    U32,
    U64,
    U128,
}

impl UintTy {
    pub fn name_str(&self) -> &'static str {
        match *self {
            UintTy::Usize => "usize",
            UintTy::U8 => "u8",
            UintTy::U16 => "u16",
            UintTy::U32 => "u32",
            UintTy::U64 => "u64",
            UintTy::U128 => "u128",
        }
    }

    pub fn name(&self) -> Symbol {
        match *self {
            UintTy::Usize => sym::usize,
            UintTy::U8 => sym::u8,
            UintTy::U16 => sym::u16,
            UintTy::U32 => sym::u32,
            UintTy::U64 => sym::u64,
            UintTy::U128 => sym::u128,
        }
    }

    pub fn val_to_string(&self, val: u128) -> String {
        format!("{}{}", val, self.name_str())
    }

    pub fn bit_width(&self) -> Option<usize> {
        Some(match *self {
            UintTy::Usize => return None,
            UintTy::U8 => 8,
            UintTy::U16 => 16,
            UintTy::U32 => 32,
            UintTy::U64 => 64,
            UintTy::U128 => 128,
        })
    }
}

/// A constraint on an associated type (e.g., `A = Bar` in `Foo<A = Bar>` or
/// `A: TraitA + TraitB` in `Foo<A: TraitA + TraitB>`).
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct AssocTyConstraint {
    pub id: NodeId,
    pub ident: Ident,
    pub kind: AssocTyConstraintKind,
    pub span: Span,
}

/// The kinds of an `AssocTyConstraint`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum AssocTyConstraintKind {
    /// E.g., `A = Bar` in `Foo<A = Bar>`.
    Equality {
        ty: P<Ty>,
    },
    /// E.g. `A: TraitA + TraitB` in `Foo<A: TraitA + TraitB>`.
    Bound {
        bounds: GenericBounds,
    },
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Ty {
    pub id: NodeId,
    pub kind: TyKind,
    pub span: Span,
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct BareFnTy {
    pub unsafety: Unsafety,
    pub ext: Extern,
    pub generic_params: Vec<GenericParam>,
    pub decl: P<FnDecl>,
}

/// The various kinds of type recognized by the compiler.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum TyKind {
    /// A variable-length slice (`[T]`).
    Slice(P<Ty>),
    /// A fixed length array (`[T; n]`).
    Array(P<Ty>, AnonConst),
    /// A raw pointer (`*const T` or `*mut T`).
    Ptr(MutTy),
    /// A reference (`&'a T` or `&'a mut T`).
    Rptr(Option<Lifetime>, MutTy),
    /// A bare function (e.g., `fn(usize) -> bool`).
    BareFn(P<BareFnTy>),
    /// The never type (`!`).
    Never,
    /// A tuple (`(A, B, C, D,...)`).
    Tup(Vec<P<Ty>>),
    /// A path (`module::module::...::Type`), optionally
    /// "qualified", e.g., `<Vec<T> as SomeTrait>::SomeType`.
    ///
    /// Type parameters are stored in the `Path` itself.
    Path(Option<QSelf>, Path),
    /// A trait object type `Bound1 + Bound2 + Bound3`
    /// where `Bound` is a trait or a lifetime.
    TraitObject(GenericBounds, TraitObjectSyntax),
    /// An `impl Bound1 + Bound2 + Bound3` type
    /// where `Bound` is a trait or a lifetime.
    ///
    /// The `NodeId` exists to prevent lowering from having to
    /// generate `NodeId`s on the fly, which would complicate
    /// the generation of opaque `type Foo = impl Trait` items significantly.
    ImplTrait(NodeId, GenericBounds),
    /// No-op; kept solely so that we can pretty-print faithfully.
    Paren(P<Ty>),
    /// Unused for now.
    Typeof(AnonConst),
    /// This means the type should be inferred instead of it having been
    /// specified. This can appear anywhere in a type.
    Infer,
    /// Inferred type of a `self` or `&self` argument in a method.
    ImplicitSelf,
    /// A macro in the type position.
    Mac(Mac),
    /// Placeholder for a kind that has failed to be defined.
    Err,
    /// Placeholder for a `va_list`.
    CVarArgs,
}

impl TyKind {
    pub fn is_implicit_self(&self) -> bool {
        if let TyKind::ImplicitSelf = *self {
            true
        } else {
            false
        }
    }

    pub fn is_unit(&self) -> bool {
        if let TyKind::Tup(ref tys) = *self {
            tys.is_empty()
        } else {
            false
        }
    }

    /// HACK(type_alias_impl_trait, Centril): A temporary crutch used
    /// in lowering to avoid making larger changes there and beyond.
    pub fn opaque_top_hack(&self) -> Option<&GenericBounds> {
        match self {
            Self::ImplTrait(_, bounds) => Some(bounds),
            _ => None,
        }
    }
}

/// Syntax used to declare a trait object.
#[derive(Clone, Copy, PartialEq, RustcEncodable, RustcDecodable, Debug)]
pub enum TraitObjectSyntax {
    Dyn,
    None,
}

/// Inline assembly dialect.
///
/// E.g., `"intel"` as in `asm!("mov eax, 2" : "={eax}"(result) : : : "intel")`.
#[derive(Clone, PartialEq, RustcEncodable, RustcDecodable, Debug, Copy, HashStable_Generic)]
pub enum AsmDialect {
    Att,
    Intel,
}

/// Inline assembly.
///
/// E.g., `"={eax}"(result)` as in `asm!("mov eax, 2" : "={eax}"(result) : : : "intel")`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct InlineAsmOutput {
    pub constraint: Symbol,
    pub expr: P<Expr>,
    pub is_rw: bool,
    pub is_indirect: bool,
}

/// Inline assembly.
///
/// E.g., `asm!("NOP");`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct InlineAsm {
    pub asm: Symbol,
    pub asm_str_style: StrStyle,
    pub outputs: Vec<InlineAsmOutput>,
    pub inputs: Vec<(Symbol, P<Expr>)>,
    pub clobbers: Vec<Symbol>,
    pub volatile: bool,
    pub alignstack: bool,
    pub dialect: AsmDialect,
}

/// A parameter in a function header.
///
/// E.g., `bar: usize` as in `fn foo(bar: usize)`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Param {
    pub attrs: ThinVec<Attribute>,
    pub ty: P<Ty>,
    pub pat: P<Pat>,
    pub id: NodeId,
    pub span: Span,
    pub is_placeholder: bool,
}

/// Alternative representation for `Arg`s describing `self` parameter of methods.
///
/// E.g., `&mut self` as in `fn foo(&mut self)`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum SelfKind {
    /// `self`, `mut self`
    Value(Mutability),
    /// `&'lt self`, `&'lt mut self`
    Region(Option<Lifetime>, Mutability),
    /// `self: TYPE`, `mut self: TYPE`
    Explicit(P<Ty>, Mutability),
}

pub type ExplicitSelf = Spanned<SelfKind>;

impl Param {
    /// Attempts to cast parameter to `ExplicitSelf`.
    pub fn to_self(&self) -> Option<ExplicitSelf> {
        if let PatKind::Ident(BindingMode::ByValue(mutbl), ident, _) = self.pat.kind {
            if ident.name == kw::SelfLower {
                return match self.ty.kind {
                    TyKind::ImplicitSelf => Some(respan(self.pat.span, SelfKind::Value(mutbl))),
                    TyKind::Rptr(lt, MutTy { ref ty, mutbl }) if ty.kind.is_implicit_self() => {
                        Some(respan(self.pat.span, SelfKind::Region(lt, mutbl)))
                    }
                    _ => Some(respan(
                        self.pat.span.to(self.ty.span),
                        SelfKind::Explicit(self.ty.clone(), mutbl),
                    )),
                };
            }
        }
        None
    }

    /// Returns `true` if parameter is `self`.
    pub fn is_self(&self) -> bool {
        if let PatKind::Ident(_, ident, _) = self.pat.kind {
            ident.name == kw::SelfLower
        } else {
            false
        }
    }

    /// Builds a `Param` object from `ExplicitSelf`.
    pub fn from_self(attrs: ThinVec<Attribute>, eself: ExplicitSelf, eself_ident: Ident) -> Param {
        let span = eself.span.to(eself_ident.span);
        let infer_ty = P(Ty {
            id: DUMMY_NODE_ID,
            kind: TyKind::ImplicitSelf,
            span,
        });
        let param = |mutbl, ty| Param {
            attrs,
            pat: P(Pat {
                id: DUMMY_NODE_ID,
                kind: PatKind::Ident(BindingMode::ByValue(mutbl), eself_ident, None),
                span,
            }),
            span,
            ty,
            id: DUMMY_NODE_ID,
            is_placeholder: false
        };
        match eself.node {
            SelfKind::Explicit(ty, mutbl) => param(mutbl, ty),
            SelfKind::Value(mutbl) => param(mutbl, infer_ty),
            SelfKind::Region(lt, mutbl) => param(
                Mutability::Immutable,
                P(Ty {
                    id: DUMMY_NODE_ID,
                    kind: TyKind::Rptr(
                        lt,
                        MutTy {
                            ty: infer_ty,
                            mutbl,
                        },
                    ),
                    span,
                }),
            ),
        }
    }
}

/// A signature (not the body) of a function declaration.
///
/// E.g., `fn foo(bar: baz)`.
///
/// Please note that it's different from `FnHeader` structure
/// which contains metadata about function safety, asyncness, constness and ABI.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct FnDecl {
    pub inputs: Vec<Param>,
    pub output: FunctionRetTy,
}

impl FnDecl {
    pub fn get_self(&self) -> Option<ExplicitSelf> {
        self.inputs.get(0).and_then(Param::to_self)
    }
    pub fn has_self(&self) -> bool {
        self.inputs.get(0).map_or(false, Param::is_self)
    }
    pub fn c_variadic(&self) -> bool {
        self.inputs.last().map_or(false, |arg| match arg.ty.kind {
            TyKind::CVarArgs => true,
            _ => false,
        })
    }
}

/// Is the trait definition an auto trait?
#[derive(Copy, Clone, PartialEq, RustcEncodable, RustcDecodable, Debug, HashStable_Generic)]
pub enum IsAuto {
    Yes,
    No,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash,
         RustcEncodable, RustcDecodable, Debug, HashStable_Generic)]
pub enum Unsafety {
    Unsafe,
    Normal,
}

impl Unsafety {
    pub fn prefix_str(&self) -> &'static str {
        match self {
            Unsafety::Unsafe => "unsafe ",
            Unsafety::Normal => "",
        }
    }
}

impl fmt::Display for Unsafety {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(
            match *self {
                Unsafety::Normal => "normal",
                Unsafety::Unsafe => "unsafe",
            },
            f,
        )
    }
}

#[derive(Copy, Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum IsAsync {
    Async {
        closure_id: NodeId,
        return_impl_trait_id: NodeId,
    },
    NotAsync,
}

impl IsAsync {
    pub fn is_async(self) -> bool {
        if let IsAsync::Async { .. } = self {
            true
        } else {
            false
        }
    }

    /// In ths case this is an `async` return, the `NodeId` for the generated `impl Trait` item.
    pub fn opt_return_id(self) -> Option<NodeId> {
        match self {
            IsAsync::Async {
                return_impl_trait_id,
                ..
            } => Some(return_impl_trait_id),
            IsAsync::NotAsync => None,
        }
    }
}

#[derive(Copy, Clone, PartialEq, RustcEncodable, RustcDecodable, Debug, HashStable_Generic)]
pub enum Constness {
    Const,
    NotConst,
}

/// Item defaultness.
/// For details see the [RFC #2532](https://github.com/rust-lang/rfcs/pull/2532).
#[derive(Copy, Clone, PartialEq, RustcEncodable, RustcDecodable, Debug, HashStable_Generic)]
pub enum Defaultness {
    Default,
    Final,
}

#[derive(Copy, Clone, PartialEq, RustcEncodable, RustcDecodable, HashStable_Generic)]
pub enum ImplPolarity {
    /// `impl Trait for Type`
    Positive,
    /// `impl !Trait for Type`
    Negative,
}

impl fmt::Debug for ImplPolarity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ImplPolarity::Positive => "positive".fmt(f),
            ImplPolarity::Negative => "negative".fmt(f),
        }
    }
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum FunctionRetTy {
    /// Returns type is not specified.
    ///
    /// Functions default to `()` and closures default to inference.
    /// Span points to where return type would be inserted.
    Default(Span),
    /// Everything else.
    Ty(P<Ty>),
}

impl FunctionRetTy {
    pub fn span(&self) -> Span {
        match *self {
            FunctionRetTy::Default(span) => span,
            FunctionRetTy::Ty(ref ty) => ty.span,
        }
    }
}

/// Module declaration.
///
/// E.g., `mod foo;` or `mod foo { .. }`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Mod {
    /// A span from the first token past `{` to the last token until `}`.
    /// For `mod foo;`, the inner span ranges from the first token
    /// to the last token in the external file.
    pub inner: Span,
    pub items: Vec<P<Item>>,
    /// `true` for `mod foo { .. }`; `false` for `mod foo;`.
    pub inline: bool,
}

/// Foreign module declaration.
///
/// E.g., `extern { .. }` or `extern C { .. }`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct ForeignMod {
    pub abi: Option<StrLit>,
    pub items: Vec<ForeignItem>,
}

/// Global inline assembly.
///
/// Also known as "module-level assembly" or "file-scoped assembly".
#[derive(Clone, RustcEncodable, RustcDecodable, Debug, Copy)]
pub struct GlobalAsm {
    pub asm: Symbol,
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct EnumDef {
    pub variants: Vec<Variant>,
}
/// Enum variant.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Variant {
    /// Attributes of the variant.
    pub attrs: Vec<Attribute>,
    /// Id of the variant (not the constructor, see `VariantData::ctor_id()`).
    pub id: NodeId,
    /// Span
    pub span: Span,
    /// The visibility of the variant. Syntactically accepted but not semantically.
    pub vis: Visibility,
    /// Name of the variant.
    pub ident: Ident,

    /// Fields and constructor id of the variant.
    pub data: VariantData,
    /// Explicit discriminant, e.g., `Foo = 1`.
    pub disr_expr: Option<AnonConst>,
    /// Is a macro placeholder
    pub is_placeholder: bool,
}

/// Part of `use` item to the right of its prefix.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum UseTreeKind {
    /// `use prefix` or `use prefix as rename`
    ///
    /// The extra `NodeId`s are for HIR lowering, when additional statements are created for each
    /// namespace.
    Simple(Option<Ident>, NodeId, NodeId),
    /// `use prefix::{...}`
    Nested(Vec<(UseTree, NodeId)>),
    /// `use prefix::*`
    Glob,
}

/// A tree of paths sharing common prefixes.
/// Used in `use` items both at top-level and inside of braces in import groups.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct UseTree {
    pub prefix: Path,
    pub kind: UseTreeKind,
    pub span: Span,
}

impl UseTree {
    pub fn ident(&self) -> Ident {
        match self.kind {
            UseTreeKind::Simple(Some(rename), ..) => rename,
            UseTreeKind::Simple(None, ..) => {
                self.prefix
                    .segments
                    .last()
                    .expect("empty prefix in a simple import")
                    .ident
            }
            _ => panic!("`UseTree::ident` can only be used on a simple import"),
        }
    }
}

/// Distinguishes between `Attribute`s that decorate items and Attributes that
/// are contained as statements within items. These two cases need to be
/// distinguished for pretty-printing.
#[derive(Clone, PartialEq, RustcEncodable, RustcDecodable, Debug, Copy, HashStable_Generic)]
pub enum AttrStyle {
    Outer,
    Inner,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord, Copy)]
pub struct AttrId(pub usize);

impl Idx for AttrId {
    fn new(idx: usize) -> Self {
        AttrId(idx)
    }
    fn index(self) -> usize {
        self.0
    }
}

impl rustc_serialize::Encodable for AttrId {
    fn encode<S: Encoder>(&self, s: &mut S) -> Result<(), S::Error> {
        s.emit_unit()
    }
}

impl rustc_serialize::Decodable for AttrId {
    fn decode<D: Decoder>(d: &mut D) -> Result<AttrId, D::Error> {
        d.read_nil().map(|_| crate::attr::mk_attr_id())
    }
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct AttrItem {
    pub path: Path,
    pub tokens: TokenStream,
}

/// Metadata associated with an item.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Attribute {
    pub kind: AttrKind,
    pub id: AttrId,
    /// Denotes if the attribute decorates the following construct (outer)
    /// or the construct this attribute is contained within (inner).
    pub style: AttrStyle,
    pub span: Span,
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum AttrKind {
    /// A normal attribute.
    Normal(AttrItem),

    /// A doc comment (e.g. `/// ...`, `//! ...`, `/** ... */`, `/*! ... */`).
    /// Doc attributes (e.g. `#[doc="..."]`) are represented with the `Normal`
    /// variant (which is much less compact and thus more expensive).
    ///
    /// Note: `self.has_name(sym::doc)` and `self.check_name(sym::doc)` succeed
    /// for this variant, but this may change in the future.
    /// ```
    DocComment(Symbol),
}

/// `TraitRef`s appear in impls.
///
/// Resolution maps each `TraitRef`'s `ref_id` to its defining trait; that's all
/// that the `ref_id` is for. The `impl_id` maps to the "self type" of this impl.
/// If this impl is an `ItemKind::Impl`, the `impl_id` is redundant (it could be the
/// same as the impl's `NodeId`).
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct TraitRef {
    pub path: Path,
    pub ref_id: NodeId,
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct PolyTraitRef {
    /// The `'a` in `<'a> Foo<&'a T>`.
    pub bound_generic_params: Vec<GenericParam>,

    /// The `Foo<&'a T>` in `<'a> Foo<&'a T>`.
    pub trait_ref: TraitRef,

    pub span: Span,
}

impl PolyTraitRef {
    pub fn new(generic_params: Vec<GenericParam>, path: Path, span: Span) -> Self {
        PolyTraitRef {
            bound_generic_params: generic_params,
            trait_ref: TraitRef {
                path,
                ref_id: DUMMY_NODE_ID,
            },
            span,
        }
    }
}

#[derive(Copy, Clone, RustcEncodable, RustcDecodable, Debug, HashStable_Generic)]
pub enum CrateSugar {
    /// Source is `pub(crate)`.
    PubCrate,

    /// Source is (just) `crate`.
    JustCrate,
}

pub type Visibility = Spanned<VisibilityKind>;

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum VisibilityKind {
    Public,
    Crate(CrateSugar),
    Restricted { path: P<Path>, id: NodeId },
    Inherited,
}

impl VisibilityKind {
    pub fn is_pub(&self) -> bool {
        if let VisibilityKind::Public = *self {
            true
        } else {
            false
        }
    }
}

/// Field of a struct.
///
/// E.g., `bar: usize` as in `struct Foo { bar: usize }`.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct StructField {
    pub attrs: Vec<Attribute>,
    pub id: NodeId,
    pub span: Span,
    pub vis: Visibility,
    pub ident: Option<Ident>,

    pub ty: P<Ty>,
    pub is_placeholder: bool,
}

/// Fields and constructor ids of enum variants and structs.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum VariantData {
    /// Struct variant.
    ///
    /// E.g., `Bar { .. }` as in `enum Foo { Bar { .. } }`.
    Struct(Vec<StructField>, bool),
    /// Tuple variant.
    ///
    /// E.g., `Bar(..)` as in `enum Foo { Bar(..) }`.
    Tuple(Vec<StructField>, NodeId),
    /// Unit variant.
    ///
    /// E.g., `Bar = ..` as in `enum Foo { Bar = .. }`.
    Unit(NodeId),
}

impl VariantData {
    /// Return the fields of this variant.
    pub fn fields(&self) -> &[StructField] {
        match *self {
            VariantData::Struct(ref fields, ..) | VariantData::Tuple(ref fields, _) => fields,
            _ => &[],
        }
    }

    /// Return the `NodeId` of this variant's constructor, if it has one.
    pub fn ctor_id(&self) -> Option<NodeId> {
        match *self {
            VariantData::Struct(..) => None,
            VariantData::Tuple(_, id) | VariantData::Unit(id) => Some(id),
        }
    }
}

/// An item.
///
/// The name might be a dummy name in case of anonymous items.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct Item {
    pub attrs: Vec<Attribute>,
    pub id: NodeId,
    pub span: Span,
    pub vis: Visibility,
    pub ident: Ident,

    pub kind: ItemKind,

    /// Original tokens this item was parsed from. This isn't necessarily
    /// available for all items, although over time more and more items should
    /// have this be `Some`. Right now this is primarily used for procedural
    /// macros, notably custom attributes.
    ///
    /// Note that the tokens here do not include the outer attributes, but will
    /// include inner attributes.
    pub tokens: Option<TokenStream>,
}

impl Item {
    /// Return the span that encompasses the attributes.
    pub fn span_with_attributes(&self) -> Span {
        self.attrs.iter().fold(self.span, |acc, attr| acc.to(attr.span))
    }
}

/// `extern` qualifier on a function item or function type.
#[derive(Clone, Copy, RustcEncodable, RustcDecodable, Debug)]
pub enum Extern {
    None,
    Implicit,
    Explicit(StrLit),
}

impl Extern {
    pub fn from_abi(abi: Option<StrLit>) -> Extern {
        abi.map_or(Extern::Implicit, Extern::Explicit)
    }
}

/// A function header.
///
/// All the information between the visibility and the name of the function is
/// included in this struct (e.g., `async unsafe fn` or `const extern "C" fn`).
#[derive(Clone, Copy, RustcEncodable, RustcDecodable, Debug)]
pub struct FnHeader {
    pub unsafety: Unsafety,
    pub asyncness: Spanned<IsAsync>,
    pub constness: Spanned<Constness>,
    pub ext: Extern,
}

impl Default for FnHeader {
    fn default() -> FnHeader {
        FnHeader {
            unsafety: Unsafety::Normal,
            asyncness: dummy_spanned(IsAsync::NotAsync),
            constness: dummy_spanned(Constness::NotConst),
            ext: Extern::None,
        }
    }
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum ItemKind {
    /// An `extern crate` item, with the optional *original* crate name if the crate was renamed.
    ///
    /// E.g., `extern crate foo` or `extern crate foo_bar as foo`.
    ExternCrate(Option<Name>),
    /// A use declaration item (`use`).
    ///
    /// E.g., `use foo;`, `use foo::bar;` or `use foo::bar as FooBar;`.
    Use(P<UseTree>),
    /// A static item (`static`).
    ///
    /// E.g., `static FOO: i32 = 42;` or `static FOO: &'static str = "bar";`.
    Static(P<Ty>, Mutability, P<Expr>),
    /// A constant item (`const`).
    ///
    /// E.g., `const FOO: i32 = 42;`.
    Const(P<Ty>, P<Expr>),
    /// A function declaration (`fn`).
    ///
    /// E.g., `fn foo(bar: usize) -> usize { .. }`.
    Fn(FnSig, Generics, P<Block>),
    /// A module declaration (`mod`).
    ///
    /// E.g., `mod foo;` or `mod foo { .. }`.
    Mod(Mod),
    /// An external module (`extern`).
    ///
    /// E.g., `extern {}` or `extern "C" {}`.
    ForeignMod(ForeignMod),
    /// Module-level inline assembly (from `global_asm!()`).
    GlobalAsm(P<GlobalAsm>),
    /// A type alias (`type`).
    ///
    /// E.g., `type Foo = Bar<u8>;`.
    TyAlias(P<Ty>, Generics),
    /// An enum definition (`enum`).
    ///
    /// E.g., `enum Foo<A, B> { C<A>, D<B> }`.
    Enum(EnumDef, Generics),
    /// A struct definition (`struct`).
    ///
    /// E.g., `struct Foo<A> { x: A }`.
    Struct(VariantData, Generics),
    /// A union definition (`union`).
    ///
    /// E.g., `union Foo<A, B> { x: A, y: B }`.
    Union(VariantData, Generics),
    /// A trait declaration (`trait`).
    ///
    /// E.g., `trait Foo { .. }`, `trait Foo<T> { .. }` or `auto trait Foo {}`.
    Trait(IsAuto, Unsafety, Generics, GenericBounds, Vec<TraitItem>),
    /// Trait alias
    ///
    /// E.g., `trait Foo = Bar + Quux;`.
    TraitAlias(Generics, GenericBounds),
    /// An implementation.
    ///
    /// E.g., `impl<A> Foo<A> { .. }` or `impl<A> Trait for Foo<A> { .. }`.
    Impl(
        Unsafety,
        ImplPolarity,
        Defaultness,
        Generics,
        Option<TraitRef>, // (optional) trait this impl implements
        P<Ty>,            // self
        Vec<ImplItem>,
    ),
    /// A macro invocation.
    ///
    /// E.g., `foo!(..)`.
    Mac(Mac),

    /// A macro definition.
    MacroDef(MacroDef),
}

impl ItemKind {
    pub fn descriptive_variant(&self) -> &str {
        match *self {
            ItemKind::ExternCrate(..) => "extern crate",
            ItemKind::Use(..) => "use",
            ItemKind::Static(..) => "static item",
            ItemKind::Const(..) => "constant item",
            ItemKind::Fn(..) => "function",
            ItemKind::Mod(..) => "module",
            ItemKind::ForeignMod(..) => "foreign module",
            ItemKind::GlobalAsm(..) => "global asm",
            ItemKind::TyAlias(..) => "type alias",
            ItemKind::Enum(..) => "enum",
            ItemKind::Struct(..) => "struct",
            ItemKind::Union(..) => "union",
            ItemKind::Trait(..) => "trait",
            ItemKind::TraitAlias(..) => "trait alias",
            ItemKind::Mac(..) | ItemKind::MacroDef(..) | ItemKind::Impl(..) => "item",
        }
    }
}

#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub struct ForeignItem {
    pub attrs: Vec<Attribute>,
    pub id: NodeId,
    pub span: Span,
    pub vis: Visibility,
    pub ident: Ident,

    pub kind: ForeignItemKind,
}

/// An item within an `extern` block.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug)]
pub enum ForeignItemKind {
    /// A foreign function.
    Fn(P<FnDecl>, Generics),
    /// A foreign static item (`static ext: u8`).
    Static(P<Ty>, Mutability),
    /// A foreign type.
    Ty,
    /// A macro invocation.
    Macro(Mac),
}

impl ForeignItemKind {
    pub fn descriptive_variant(&self) -> &str {
        match *self {
            ForeignItemKind::Fn(..) => "foreign function",
            ForeignItemKind::Static(..) => "foreign static item",
            ForeignItemKind::Ty => "foreign type",
            ForeignItemKind::Macro(..) => "macro in foreign module",
        }
    }
}
