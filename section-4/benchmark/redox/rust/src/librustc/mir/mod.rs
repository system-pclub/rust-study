// ignore-tidy-filelength

//! MIR datatypes and passes. See the [rustc guide] for more info.
//!
//! [rustc guide]: https://rust-lang.github.io/rustc-guide/mir/index.html

use crate::hir::def::{CtorKind, Namespace};
use crate::hir::def_id::DefId;
use crate::hir;
use crate::mir::interpret::{GlobalAlloc, PanicInfo, Scalar};
use crate::mir::visit::MirVisitable;
use crate::ty::adjustment::PointerCast;
use crate::ty::fold::{TypeFoldable, TypeFolder, TypeVisitor};
use crate::ty::layout::VariantIdx;
use crate::ty::print::{FmtPrinter, Printer};
use crate::ty::subst::{Subst, SubstsRef};
use crate::ty::{
    self, AdtDef, CanonicalUserTypeAnnotations, List, Region, Ty, TyCtxt, UserTypeAnnotationIndex,
};

use polonius_engine::Atom;
use rustc_index::bit_set::BitMatrix;
use rustc_data_structures::fx::FxHashSet;
use rustc_data_structures::graph::dominators::{dominators, Dominators};
use rustc_data_structures::graph::{self, GraphPredecessors, GraphSuccessors};
use rustc_index::vec::{Idx, IndexVec};
use rustc_data_structures::sync::Lrc;
use rustc_data_structures::sync::MappedReadGuard;
use rustc_macros::HashStable;
use rustc_serialize::{Encodable, Decodable};
use smallvec::SmallVec;
use std::borrow::Cow;
use std::fmt::{self, Debug, Display, Formatter, Write};
use std::ops::{Index, IndexMut};
use std::slice;
use std::vec::IntoIter;
use std::{iter, mem, option, u32};
use syntax::ast::Name;
use syntax::symbol::Symbol;
use syntax_pos::{Span, DUMMY_SP};

pub use crate::mir::interpret::AssertMessage;

mod cache;
pub mod interpret;
pub mod mono;
pub mod tcx;
pub mod traversal;
pub mod visit;

/// Types for locals
type LocalDecls<'tcx> = IndexVec<Local, LocalDecl<'tcx>>;

pub trait HasLocalDecls<'tcx> {
    fn local_decls(&self) -> &LocalDecls<'tcx>;
}

impl<'tcx> HasLocalDecls<'tcx> for LocalDecls<'tcx> {
    fn local_decls(&self) -> &LocalDecls<'tcx> {
        self
    }
}

impl<'tcx> HasLocalDecls<'tcx> for Body<'tcx> {
    fn local_decls(&self) -> &LocalDecls<'tcx> {
        &self.local_decls
    }
}

/// The various "big phases" that MIR goes through.
///
/// Warning: ordering of variants is significant.
#[derive(Copy, Clone, RustcEncodable, RustcDecodable, HashStable,
         Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MirPhase {
    Build = 0,
    Const = 1,
    Validated = 2,
    Optimized = 3,
}

impl MirPhase {
    /// Gets the index of the current MirPhase within the set of all `MirPhase`s.
    pub fn phase_index(&self) -> usize {
        *self as usize
    }
}

/// The lowered representation of a single function.
#[derive(Clone, RustcEncodable, RustcDecodable, Debug, HashStable, TypeFoldable)]
pub struct Body<'tcx> {
    /// A list of basic blocks. References to basic block use a newtyped index type `BasicBlock`
    /// that indexes into this vector.
    basic_blocks: IndexVec<BasicBlock, BasicBlockData<'tcx>>,

    /// Records how far through the "desugaring and optimization" process this particular
    /// MIR has traversed. This is particularly useful when inlining, since in that context
    /// we instantiate the promoted constants and add them to our promoted vector -- but those
    /// promoted items have already been optimized, whereas ours have not. This field allows
    /// us to see the difference and forego optimization on the inlined promoted items.
    pub phase: MirPhase,

    /// A list of source scopes; these are referenced by statements
    /// and used for debuginfo. Indexed by a `SourceScope`.
    pub source_scopes: IndexVec<SourceScope, SourceScopeData>,

    /// Crate-local information for each source scope, that can't (and
    /// needn't) be tracked across crates.
    pub source_scope_local_data: ClearCrossCrate<IndexVec<SourceScope, SourceScopeLocalData>>,

    /// The yield type of the function, if it is a generator.
    pub yield_ty: Option<Ty<'tcx>>,

    /// Generator drop glue.
    pub generator_drop: Option<Box<Body<'tcx>>>,

    /// The layout of a generator. Produced by the state transformation.
    pub generator_layout: Option<GeneratorLayout<'tcx>>,

    /// Declarations of locals.
    ///
    /// The first local is the return value pointer, followed by `arg_count`
    /// locals for the function arguments, followed by any user-declared
    /// variables and temporaries.
    pub local_decls: LocalDecls<'tcx>,

    /// User type annotations.
    pub user_type_annotations: CanonicalUserTypeAnnotations<'tcx>,

    /// The number of arguments this function takes.
    ///
    /// Starting at local 1, `arg_count` locals will be provided by the caller
    /// and can be assumed to be initialized.
    ///
    /// If this MIR was built for a constant, this will be 0.
    pub arg_count: usize,

    /// Mark an argument local (which must be a tuple) as getting passed as
    /// its individual components at the LLVM level.
    ///
    /// This is used for the "rust-call" ABI.
    pub spread_arg: Option<Local>,

    /// Names and capture modes of all the closure upvars, assuming
    /// the first argument is either the closure or a reference to it.
    //
    // NOTE(eddyb) This is *strictly* a temporary hack for codegen
    // debuginfo generation, and will be removed at some point.
    // Do **NOT** use it for anything else; upvar information should not be
    // in the MIR, so please rely on local crate HIR or other side-channels.
    pub __upvar_debuginfo_codegen_only_do_not_use: Vec<UpvarDebuginfo>,

    /// Mark this MIR of a const context other than const functions as having converted a `&&` or
    /// `||` expression into `&` or `|` respectively. This is problematic because if we ever stop
    /// this conversion from happening and use short circuiting, we will cause the following code
    /// to change the value of `x`: `let mut x = 42; false && { x = 55; true };`
    ///
    /// List of places where control flow was destroyed. Used for error reporting.
    pub control_flow_destroyed: Vec<(Span, String)>,

    /// A span representing this MIR, for error reporting.
    pub span: Span,

    /// A cache for various calculations.
    cache: cache::Cache,
}

impl<'tcx> Body<'tcx> {
    pub fn new(
        basic_blocks: IndexVec<BasicBlock, BasicBlockData<'tcx>>,
        source_scopes: IndexVec<SourceScope, SourceScopeData>,
        source_scope_local_data: ClearCrossCrate<IndexVec<SourceScope, SourceScopeLocalData>>,
        yield_ty: Option<Ty<'tcx>>,
        local_decls: LocalDecls<'tcx>,
        user_type_annotations: CanonicalUserTypeAnnotations<'tcx>,
        arg_count: usize,
        __upvar_debuginfo_codegen_only_do_not_use: Vec<UpvarDebuginfo>,
        span: Span,
        control_flow_destroyed: Vec<(Span, String)>,
    ) -> Self {
        // We need `arg_count` locals, and one for the return place.
        assert!(
            local_decls.len() >= arg_count + 1,
            "expected at least {} locals, got {}",
            arg_count + 1,
            local_decls.len()
        );

        Body {
            phase: MirPhase::Build,
            basic_blocks,
            source_scopes,
            source_scope_local_data,
            yield_ty,
            generator_drop: None,
            generator_layout: None,
            local_decls,
            user_type_annotations,
            arg_count,
            __upvar_debuginfo_codegen_only_do_not_use,
            spread_arg: None,
            span,
            cache: cache::Cache::new(),
            control_flow_destroyed,
        }
    }

    #[inline]
    pub fn basic_blocks(&self) -> &IndexVec<BasicBlock, BasicBlockData<'tcx>> {
        &self.basic_blocks
    }

    #[inline]
    pub fn basic_blocks_mut(&mut self) -> &mut IndexVec<BasicBlock, BasicBlockData<'tcx>> {
        self.cache.invalidate();
        &mut self.basic_blocks
    }

    #[inline]
    pub fn basic_blocks_and_local_decls_mut(
        &mut self,
    ) -> (&mut IndexVec<BasicBlock, BasicBlockData<'tcx>>, &mut LocalDecls<'tcx>) {
        self.cache.invalidate();
        (&mut self.basic_blocks, &mut self.local_decls)
    }

    #[inline]
    pub fn predecessors(&self) -> MappedReadGuard<'_, IndexVec<BasicBlock, Vec<BasicBlock>>> {
        self.cache.predecessors(self)
    }

    #[inline]
    pub fn predecessors_for(&self, bb: BasicBlock) -> MappedReadGuard<'_, Vec<BasicBlock>> {
        MappedReadGuard::map(self.predecessors(), |p| &p[bb])
    }

    #[inline]
    pub fn predecessor_locations(&self, loc: Location) -> impl Iterator<Item = Location> + '_ {
        let if_zero_locations = if loc.statement_index == 0 {
            let predecessor_blocks = self.predecessors_for(loc.block);
            let num_predecessor_blocks = predecessor_blocks.len();
            Some(
                (0..num_predecessor_blocks)
                    .map(move |i| predecessor_blocks[i])
                    .map(move |bb| self.terminator_loc(bb)),
            )
        } else {
            None
        };

        let if_not_zero_locations = if loc.statement_index == 0 {
            None
        } else {
            Some(Location { block: loc.block, statement_index: loc.statement_index - 1 })
        };

        if_zero_locations.into_iter().flatten().chain(if_not_zero_locations)
    }

    #[inline]
    pub fn dominators(&self) -> Dominators<BasicBlock> {
        dominators(self)
    }

    /// Returns `true` if a cycle exists in the control-flow graph that is reachable from the
    /// `START_BLOCK`.
    pub fn is_cfg_cyclic(&self) -> bool {
        graph::is_cyclic(self)
    }

    #[inline]
    pub fn local_kind(&self, local: Local) -> LocalKind {
        let index = local.as_usize();
        if index == 0 {
            debug_assert!(
                self.local_decls[local].mutability == Mutability::Mut,
                "return place should be mutable"
            );

            LocalKind::ReturnPointer
        } else if index < self.arg_count + 1 {
            LocalKind::Arg
        } else if self.local_decls[local].name.is_some() {
            LocalKind::Var
        } else {
            LocalKind::Temp
        }
    }

    /// Returns an iterator over all temporaries.
    #[inline]
    pub fn temps_iter<'a>(&'a self) -> impl Iterator<Item = Local> + 'a {
        (self.arg_count + 1..self.local_decls.len()).filter_map(move |index| {
            let local = Local::new(index);
            if self.local_decls[local].is_user_variable() {
                None
            } else {
                Some(local)
            }
        })
    }

    /// Returns an iterator over all user-declared locals.
    #[inline]
    pub fn vars_iter<'a>(&'a self) -> impl Iterator<Item = Local> + 'a {
        (self.arg_count + 1..self.local_decls.len()).filter_map(move |index| {
            let local = Local::new(index);
            if self.local_decls[local].is_user_variable() {
                Some(local)
            } else {
                None
            }
        })
    }

    /// Returns an iterator over all user-declared mutable locals.
    #[inline]
    pub fn mut_vars_iter<'a>(&'a self) -> impl Iterator<Item = Local> + 'a {
        (self.arg_count + 1..self.local_decls.len()).filter_map(move |index| {
            let local = Local::new(index);
            let decl = &self.local_decls[local];
            if decl.is_user_variable() && decl.mutability == Mutability::Mut {
                Some(local)
            } else {
                None
            }
        })
    }

    /// Returns an iterator over all user-declared mutable arguments and locals.
    #[inline]
    pub fn mut_vars_and_args_iter<'a>(&'a self) -> impl Iterator<Item = Local> + 'a {
        (1..self.local_decls.len()).filter_map(move |index| {
            let local = Local::new(index);
            let decl = &self.local_decls[local];
            if (decl.is_user_variable() || index < self.arg_count + 1)
                && decl.mutability == Mutability::Mut
            {
                Some(local)
            } else {
                None
            }
        })
    }

    /// Returns an iterator over all function arguments.
    #[inline]
    pub fn args_iter(&self) -> impl Iterator<Item = Local> {
        let arg_count = self.arg_count;
        (1..=arg_count).map(Local::new)
    }

    /// Returns an iterator over all user-defined variables and compiler-generated temporaries (all
    /// locals that are neither arguments nor the return place).
    #[inline]
    pub fn vars_and_temps_iter(&self) -> impl Iterator<Item = Local> {
        let arg_count = self.arg_count;
        let local_count = self.local_decls.len();
        (arg_count + 1..local_count).map(Local::new)
    }

    /// Changes a statement to a nop. This is both faster than deleting instructions and avoids
    /// invalidating statement indices in `Location`s.
    pub fn make_statement_nop(&mut self, location: Location) {
        let block = &mut self[location.block];
        debug_assert!(location.statement_index < block.statements.len());
        block.statements[location.statement_index].make_nop()
    }

    /// Returns the source info associated with `location`.
    pub fn source_info(&self, location: Location) -> &SourceInfo {
        let block = &self[location.block];
        let stmts = &block.statements;
        let idx = location.statement_index;
        if idx < stmts.len() {
            &stmts[idx].source_info
        } else {
            assert_eq!(idx, stmts.len());
            &block.terminator().source_info
        }
    }

    /// Checks if `sub` is a sub scope of `sup`
    pub fn is_sub_scope(&self, mut sub: SourceScope, sup: SourceScope) -> bool {
        while sub != sup {
            match self.source_scopes[sub].parent_scope {
                None => return false,
                Some(p) => sub = p,
            }
        }
        true
    }

    /// Returns the return type; it always return first element from `local_decls` array.
    pub fn return_ty(&self) -> Ty<'tcx> {
        self.local_decls[RETURN_PLACE].ty
    }

    /// Gets the location of the terminator for the given block.
    pub fn terminator_loc(&self, bb: BasicBlock) -> Location {
        Location { block: bb, statement_index: self[bb].statements.len() }
    }
}

#[derive(Copy, Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub enum Safety {
    Safe,
    /// Unsafe because of a PushUnsafeBlock
    BuiltinUnsafe,
    /// Unsafe because of an unsafe fn
    FnUnsafe,
    /// Unsafe because of an `unsafe` block
    ExplicitUnsafe(hir::HirId),
}

impl<'tcx> Index<BasicBlock> for Body<'tcx> {
    type Output = BasicBlockData<'tcx>;

    #[inline]
    fn index(&self, index: BasicBlock) -> &BasicBlockData<'tcx> {
        &self.basic_blocks()[index]
    }
}

impl<'tcx> IndexMut<BasicBlock> for Body<'tcx> {
    #[inline]
    fn index_mut(&mut self, index: BasicBlock) -> &mut BasicBlockData<'tcx> {
        &mut self.basic_blocks_mut()[index]
    }
}

#[derive(Copy, Clone, Debug, HashStable, TypeFoldable)]
pub enum ClearCrossCrate<T> {
    Clear,
    Set(T),
}

impl<T> ClearCrossCrate<T> {
    pub fn assert_crate_local(self) -> T {
        match self {
            ClearCrossCrate::Clear => bug!("unwrapping cross-crate data"),
            ClearCrossCrate::Set(v) => v,
        }
    }
}

impl<T: Encodable> rustc_serialize::UseSpecializedEncodable for ClearCrossCrate<T> {}
impl<T: Decodable> rustc_serialize::UseSpecializedDecodable for ClearCrossCrate<T> {}

/// Grouped information about the source code origin of a MIR entity.
/// Intended to be inspected by diagnostics and debuginfo.
/// Most passes can work with it as a whole, within a single function.
// The unoffical Cranelift backend, at least as of #65828, needs `SourceInfo` to implement `Eq` and
// `Hash`. Please ping @bjorn3 if removing them.
#[derive(Copy, Clone, Debug, Eq, PartialEq, RustcEncodable, RustcDecodable, Hash, HashStable)]
pub struct SourceInfo {
    /// The source span for the AST pertaining to this MIR entity.
    pub span: Span,

    /// The source scope, keeping track of which bindings can be
    /// seen by debuginfo, active lint levels, `unsafe {...}`, etc.
    pub scope: SourceScope,
}

///////////////////////////////////////////////////////////////////////////
// Mutability and borrow kinds

#[derive(Copy, Clone, Debug, PartialEq, Eq, RustcEncodable, RustcDecodable, HashStable)]
pub enum Mutability {
    Mut,
    Not,
}

impl From<Mutability> for hir::Mutability {
    fn from(m: Mutability) -> Self {
        match m {
            Mutability::Mut => hir::Mutability::Mutable,
            Mutability::Not => hir::Mutability::Immutable,
        }
    }
}

#[derive(
    Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, RustcEncodable, RustcDecodable, HashStable,
)]
pub enum BorrowKind {
    /// Data must be immutable and is aliasable.
    Shared,

    /// The immediately borrowed place must be immutable, but projections from
    /// it don't need to be. For example, a shallow borrow of `a.b` doesn't
    /// conflict with a mutable borrow of `a.b.c`.
    ///
    /// This is used when lowering matches: when matching on a place we want to
    /// ensure that place have the same value from the start of the match until
    /// an arm is selected. This prevents this code from compiling:
    ///
    ///     let mut x = &Some(0);
    ///     match *x {
    ///         None => (),
    ///         Some(_) if { x = &None; false } => (),
    ///         Some(_) => (),
    ///     }
    ///
    /// This can't be a shared borrow because mutably borrowing (*x as Some).0
    /// should not prevent `if let None = x { ... }`, for example, because the
    /// mutating `(*x as Some).0` can't affect the discriminant of `x`.
    /// We can also report errors with this kind of borrow differently.
    Shallow,

    /// Data must be immutable but not aliasable. This kind of borrow
    /// cannot currently be expressed by the user and is used only in
    /// implicit closure bindings. It is needed when the closure is
    /// borrowing or mutating a mutable referent, e.g.:
    ///
    ///     let x: &mut isize = ...;
    ///     let y = || *x += 5;
    ///
    /// If we were to try to translate this closure into a more explicit
    /// form, we'd encounter an error with the code as written:
    ///
    ///     struct Env { x: & &mut isize }
    ///     let x: &mut isize = ...;
    ///     let y = (&mut Env { &x }, fn_ptr);  // Closure is pair of env and fn
    ///     fn fn_ptr(env: &mut Env) { **env.x += 5; }
    ///
    /// This is then illegal because you cannot mutate an `&mut` found
    /// in an aliasable location. To solve, you'd have to translate with
    /// an `&mut` borrow:
    ///
    ///     struct Env { x: & &mut isize }
    ///     let x: &mut isize = ...;
    ///     let y = (&mut Env { &mut x }, fn_ptr); // changed from &x to &mut x
    ///     fn fn_ptr(env: &mut Env) { **env.x += 5; }
    ///
    /// Now the assignment to `**env.x` is legal, but creating a
    /// mutable pointer to `x` is not because `x` is not mutable. We
    /// could fix this by declaring `x` as `let mut x`. This is ok in
    /// user code, if awkward, but extra weird for closures, since the
    /// borrow is hidden.
    ///
    /// So we introduce a "unique imm" borrow -- the referent is
    /// immutable, but not aliasable. This solves the problem. For
    /// simplicity, we don't give users the way to express this
    /// borrow, it's just used when translating closures.
    Unique,

    /// Data is mutable and not aliasable.
    Mut {
        /// `true` if this borrow arose from method-call auto-ref
        /// (i.e., `adjustment::Adjust::Borrow`).
        allow_two_phase_borrow: bool,
    },
}

impl BorrowKind {
    pub fn allows_two_phase_borrow(&self) -> bool {
        match *self {
            BorrowKind::Shared | BorrowKind::Shallow | BorrowKind::Unique => false,
            BorrowKind::Mut { allow_two_phase_borrow } => allow_two_phase_borrow,
        }
    }
}

///////////////////////////////////////////////////////////////////////////
// Variables and temps

rustc_index::newtype_index! {
    pub struct Local {
        derive [HashStable]
        DEBUG_FORMAT = "_{}",
        const RETURN_PLACE = 0,
    }
}

impl Atom for Local {
    fn index(self) -> usize {
        Idx::index(self)
    }
}

/// Classifies locals into categories. See `Body::local_kind`.
#[derive(PartialEq, Eq, Debug, HashStable)]
pub enum LocalKind {
    /// User-declared variable binding.
    Var,
    /// Compiler-introduced temporary.
    Temp,
    /// Function argument.
    Arg,
    /// Location of function's return value.
    ReturnPointer,
}

#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub struct VarBindingForm<'tcx> {
    /// Is variable bound via `x`, `mut x`, `ref x`, or `ref mut x`?
    pub binding_mode: ty::BindingMode,
    /// If an explicit type was provided for this variable binding,
    /// this holds the source Span of that type.
    ///
    /// NOTE: if you want to change this to a `HirId`, be wary that
    /// doing so breaks incremental compilation (as of this writing),
    /// while a `Span` does not cause our tests to fail.
    pub opt_ty_info: Option<Span>,
    /// Place of the RHS of the =, or the subject of the `match` where this
    /// variable is initialized. None in the case of `let PATTERN;`.
    /// Some((None, ..)) in the case of and `let [mut] x = ...` because
    /// (a) the right-hand side isn't evaluated as a place expression.
    /// (b) it gives a way to separate this case from the remaining cases
    ///     for diagnostics.
    pub opt_match_place: Option<(Option<Place<'tcx>>, Span)>,
    /// The span of the pattern in which this variable was bound.
    pub pat_span: Span,
}

#[derive(Clone, Debug, RustcEncodable, RustcDecodable)]
pub enum BindingForm<'tcx> {
    /// This is a binding for a non-`self` binding, or a `self` that has an explicit type.
    Var(VarBindingForm<'tcx>),
    /// Binding for a `self`/`&self`/`&mut self` binding where the type is implicit.
    ImplicitSelf(ImplicitSelfKind),
    /// Reference used in a guard expression to ensure immutability.
    RefForGuard,
}

/// Represents what type of implicit self a function has, if any.
#[derive(Clone, Copy, PartialEq, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub enum ImplicitSelfKind {
    /// Represents a `fn x(self);`.
    Imm,
    /// Represents a `fn x(mut self);`.
    Mut,
    /// Represents a `fn x(&self);`.
    ImmRef,
    /// Represents a `fn x(&mut self);`.
    MutRef,
    /// Represents when a function does not have a self argument or
    /// when a function has a `self: X` argument.
    None,
}

CloneTypeFoldableAndLiftImpls! { BindingForm<'tcx>, }

mod binding_form_impl {
    use crate::ich::StableHashingContext;
    use rustc_data_structures::stable_hasher::{HashStable, StableHasher};

    impl<'a, 'tcx> HashStable<StableHashingContext<'a>> for super::BindingForm<'tcx> {
        fn hash_stable(&self, hcx: &mut StableHashingContext<'a>, hasher: &mut StableHasher) {
            use super::BindingForm::*;
            ::std::mem::discriminant(self).hash_stable(hcx, hasher);

            match self {
                Var(binding) => binding.hash_stable(hcx, hasher),
                ImplicitSelf(kind) => kind.hash_stable(hcx, hasher),
                RefForGuard => (),
            }
        }
    }
}

/// `BlockTailInfo` is attached to the `LocalDecl` for temporaries
/// created during evaluation of expressions in a block tail
/// expression; that is, a block like `{ STMT_1; STMT_2; EXPR }`.
///
/// It is used to improve diagnostics when such temporaries are
/// involved in borrow_check errors, e.g., explanations of where the
/// temporaries come from, when their destructors are run, and/or how
/// one might revise the code to satisfy the borrow checker's rules.
#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub struct BlockTailInfo {
    /// If `true`, then the value resulting from evaluating this tail
    /// expression is ignored by the block's expression context.
    ///
    /// Examples include `{ ...; tail };` and `let _ = { ...; tail };`
    /// but not e.g., `let _x = { ...; tail };`
    pub tail_result_is_ignored: bool,
}

/// A MIR local.
///
/// This can be a binding declared by the user, a temporary inserted by the compiler, a function
/// argument, or the return place.
#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable, TypeFoldable)]
pub struct LocalDecl<'tcx> {
    /// Whether this is a mutable minding (i.e., `let x` or `let mut x`).
    ///
    /// Temporaries and the return place are always mutable.
    pub mutability: Mutability,

    // FIXME(matthewjasper) Don't store in this in `Body`
    pub local_info: LocalInfo<'tcx>,

    /// `true` if this is an internal local.
    ///
    /// These locals are not based on types in the source code and are only used
    /// for a few desugarings at the moment.
    ///
    /// The generator transformation will sanity check the locals which are live
    /// across a suspension point against the type components of the generator
    /// which type checking knows are live across a suspension point. We need to
    /// flag drop flags to avoid triggering this check as they are introduced
    /// after typeck.
    ///
    /// Unsafety checking will also ignore dereferences of these locals,
    /// so they can be used for raw pointers only used in a desugaring.
    ///
    /// This should be sound because the drop flags are fully algebraic, and
    /// therefore don't affect the OIBIT or outlives properties of the
    /// generator.
    pub internal: bool,

    /// If this local is a temporary and `is_block_tail` is `Some`,
    /// then it is a temporary created for evaluation of some
    /// subexpression of some block's tail expression (with no
    /// intervening statement context).
    // FIXME(matthewjasper) Don't store in this in `Body`
    pub is_block_tail: Option<BlockTailInfo>,

    /// The type of this local.
    pub ty: Ty<'tcx>,

    /// If the user manually ascribed a type to this variable,
    /// e.g., via `let x: T`, then we carry that type here. The MIR
    /// borrow checker needs this information since it can affect
    /// region inference.
    // FIXME(matthewjasper) Don't store in this in `Body`
    pub user_ty: UserTypeProjections,

    /// The name of the local, used in debuginfo and pretty-printing.
    ///
    /// Note that function arguments can also have this set to `Some(_)`
    /// to generate better debuginfo.
    pub name: Option<Name>,

    /// The *syntactic* (i.e., not visibility) source scope the local is defined
    /// in. If the local was defined in a let-statement, this
    /// is *within* the let-statement, rather than outside
    /// of it.
    ///
    /// This is needed because the visibility source scope of locals within
    /// a let-statement is weird.
    ///
    /// The reason is that we want the local to be *within* the let-statement
    /// for lint purposes, but we want the local to be *after* the let-statement
    /// for names-in-scope purposes.
    ///
    /// That's it, if we have a let-statement like the one in this
    /// function:
    ///
    /// ```
    /// fn foo(x: &str) {
    ///     #[allow(unused_mut)]
    ///     let mut x: u32 = { // <- one unused mut
    ///         let mut y: u32 = x.parse().unwrap();
    ///         y + 2
    ///     };
    ///     drop(x);
    /// }
    /// ```
    ///
    /// Then, from a lint point of view, the declaration of `x: u32`
    /// (and `y: u32`) are within the `#[allow(unused_mut)]` scope - the
    /// lint scopes are the same as the AST/HIR nesting.
    ///
    /// However, from a name lookup point of view, the scopes look more like
    /// as if the let-statements were `match` expressions:
    ///
    /// ```
    /// fn foo(x: &str) {
    ///     match {
    ///         match x.parse().unwrap() {
    ///             y => y + 2
    ///         }
    ///     } {
    ///         x => drop(x)
    ///     };
    /// }
    /// ```
    ///
    /// We care about the name-lookup scopes for debuginfo - if the
    /// debuginfo instruction pointer is at the call to `x.parse()`, we
    /// want `x` to refer to `x: &str`, but if it is at the call to
    /// `drop(x)`, we want it to refer to `x: u32`.
    ///
    /// To allow both uses to work, we need to have more than a single scope
    /// for a local. We have the `source_info.scope` represent the
    /// "syntactic" lint scope (with a variable being under its let
    /// block) while the `visibility_scope` represents the "local variable"
    /// scope (where the "rest" of a block is under all prior let-statements).
    ///
    /// The end result looks like this:
    ///
    /// ```text
    /// ROOT SCOPE
    ///  │{ argument x: &str }
    ///  │
    ///  │ │{ #[allow(unused_mut)] } // This is actually split into 2 scopes
    ///  │ │                         // in practice because I'm lazy.
    ///  │ │
    ///  │ │← x.source_info.scope
    ///  │ │← `x.parse().unwrap()`
    ///  │ │
    ///  │ │ │← y.source_info.scope
    ///  │ │
    ///  │ │ │{ let y: u32 }
    ///  │ │ │
    ///  │ │ │← y.visibility_scope
    ///  │ │ │← `y + 2`
    ///  │
    ///  │ │{ let x: u32 }
    ///  │ │← x.visibility_scope
    ///  │ │← `drop(x)` // This accesses `x: u32`.
    /// ```
    pub source_info: SourceInfo,

    /// Source scope within which the local is visible (for debuginfo)
    /// (see `source_info` for more details).
    pub visibility_scope: SourceScope,
}

/// Extra information about a local that's used for diagnostics.
#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable, TypeFoldable)]
pub enum LocalInfo<'tcx> {
    /// A user-defined local variable or function parameter
    ///
    /// The `BindingForm` is solely used for local diagnostics when generating
    /// warnings/errors when compiling the current crate, and therefore it need
    /// not be visible across crates.
    User(ClearCrossCrate<BindingForm<'tcx>>),
    /// A temporary created that references the static with the given `DefId`.
    StaticRef { def_id: DefId, is_thread_local: bool },
    /// Any other temporary, the return place, or an anonymous function parameter.
    Other,
}

impl<'tcx> LocalDecl<'tcx> {
    /// Returns `true` only if local is a binding that can itself be
    /// made mutable via the addition of the `mut` keyword, namely
    /// something like the occurrences of `x` in:
    /// - `fn foo(x: Type) { ... }`,
    /// - `let x = ...`,
    /// - or `match ... { C(x) => ... }`
    pub fn can_be_made_mutable(&self) -> bool {
        match self.local_info {
            LocalInfo::User(ClearCrossCrate::Set(BindingForm::Var(VarBindingForm {
                binding_mode: ty::BindingMode::BindByValue(_),
                opt_ty_info: _,
                opt_match_place: _,
                pat_span: _,
            }))) => true,

            LocalInfo::User(
                ClearCrossCrate::Set(BindingForm::ImplicitSelf(ImplicitSelfKind::Imm)),
            ) => true,

            _ => false,
        }
    }

    /// Returns `true` if local is definitely not a `ref ident` or
    /// `ref mut ident` binding. (Such bindings cannot be made into
    /// mutable bindings, but the inverse does not necessarily hold).
    pub fn is_nonref_binding(&self) -> bool {
        match self.local_info {
            LocalInfo::User(ClearCrossCrate::Set(BindingForm::Var(VarBindingForm {
                binding_mode: ty::BindingMode::BindByValue(_),
                opt_ty_info: _,
                opt_match_place: _,
                pat_span: _,
            }))) => true,

            LocalInfo::User(ClearCrossCrate::Set(BindingForm::ImplicitSelf(_))) => true,

            _ => false,
        }
    }

    /// Returns `true` if this variable is a named variable or function
    /// parameter declared by the user.
    #[inline]
    pub fn is_user_variable(&self) -> bool {
        match self.local_info {
            LocalInfo::User(_) => true,
            _ => false,
        }
    }

    /// Returns `true` if this is a reference to a variable bound in a `match`
    /// expression that is used to access said variable for the guard of the
    /// match arm.
    pub fn is_ref_for_guard(&self) -> bool {
        match self.local_info {
            LocalInfo::User(ClearCrossCrate::Set(BindingForm::RefForGuard)) => true,
            _ => false,
        }
    }

    /// Returns `Some` if this is a reference to a static item that is used to
    /// access that static
    pub fn is_ref_to_static(&self) -> bool {
        match self.local_info {
            LocalInfo::StaticRef { .. } => true,
            _ => false,
        }
    }

    /// Returns `Some` if this is a reference to a static item that is used to
    /// access that static
    pub fn is_ref_to_thread_local(&self) -> bool {
        match self.local_info {
            LocalInfo::StaticRef { is_thread_local, .. } => is_thread_local,
            _ => false,
        }
    }

    /// Returns `true` is the local is from a compiler desugaring, e.g.,
    /// `__next` from a `for` loop.
    #[inline]
    pub fn from_compiler_desugaring(&self) -> bool {
        self.source_info.span.desugaring_kind().is_some()
    }

    /// Creates a new `LocalDecl` for a temporary.
    #[inline]
    pub fn new_temp(ty: Ty<'tcx>, span: Span) -> Self {
        Self::new_local(ty, Mutability::Mut, false, span)
    }

    /// Converts `self` into same `LocalDecl` except tagged as immutable.
    #[inline]
    pub fn immutable(mut self) -> Self {
        self.mutability = Mutability::Not;
        self
    }

    /// Converts `self` into same `LocalDecl` except tagged as internal temporary.
    #[inline]
    pub fn block_tail(mut self, info: BlockTailInfo) -> Self {
        assert!(self.is_block_tail.is_none());
        self.is_block_tail = Some(info);
        self
    }

    /// Creates a new `LocalDecl` for a internal temporary.
    #[inline]
    pub fn new_internal(ty: Ty<'tcx>, span: Span) -> Self {
        Self::new_local(ty, Mutability::Mut, true, span)
    }

    #[inline]
    fn new_local(ty: Ty<'tcx>, mutability: Mutability, internal: bool, span: Span) -> Self {
        LocalDecl {
            mutability,
            ty,
            user_ty: UserTypeProjections::none(),
            name: None,
            source_info: SourceInfo { span, scope: OUTERMOST_SOURCE_SCOPE },
            visibility_scope: OUTERMOST_SOURCE_SCOPE,
            internal,
            local_info: LocalInfo::Other,
            is_block_tail: None,
        }
    }

    /// Builds a `LocalDecl` for the return place.
    ///
    /// This must be inserted into the `local_decls` list as the first local.
    #[inline]
    pub fn new_return_place(return_ty: Ty<'_>, span: Span) -> LocalDecl<'_> {
        LocalDecl {
            mutability: Mutability::Mut,
            ty: return_ty,
            user_ty: UserTypeProjections::none(),
            source_info: SourceInfo { span, scope: OUTERMOST_SOURCE_SCOPE },
            visibility_scope: OUTERMOST_SOURCE_SCOPE,
            internal: false,
            is_block_tail: None,
            name: None, // FIXME maybe we do want some name here?
            local_info: LocalInfo::Other,
        }
    }
}

/// A closure capture, with its name and mode.
#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub struct UpvarDebuginfo {
    pub debug_name: Name,

    /// If true, the capture is behind a reference.
    pub by_ref: bool,
}

///////////////////////////////////////////////////////////////////////////
// BasicBlock

rustc_index::newtype_index! {
    pub struct BasicBlock {
        derive [HashStable]
        DEBUG_FORMAT = "bb{}",
        const START_BLOCK = 0,
    }
}

impl BasicBlock {
    pub fn start_location(self) -> Location {
        Location { block: self, statement_index: 0 }
    }
}

///////////////////////////////////////////////////////////////////////////
// BasicBlockData and Terminator

#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable, TypeFoldable)]
pub struct BasicBlockData<'tcx> {
    /// List of statements in this block.
    pub statements: Vec<Statement<'tcx>>,

    /// Terminator for this block.
    ///
    /// N.B., this should generally ONLY be `None` during construction.
    /// Therefore, you should generally access it via the
    /// `terminator()` or `terminator_mut()` methods. The only
    /// exception is that certain passes, such as `simplify_cfg`, swap
    /// out the terminator temporarily with `None` while they continue
    /// to recurse over the set of basic blocks.
    pub terminator: Option<Terminator<'tcx>>,

    /// If true, this block lies on an unwind path. This is used
    /// during codegen where distinct kinds of basic blocks may be
    /// generated (particularly for MSVC cleanup). Unwind blocks must
    /// only branch to other unwind blocks.
    pub is_cleanup: bool,
}

#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub struct Terminator<'tcx> {
    pub source_info: SourceInfo,
    pub kind: TerminatorKind<'tcx>,
}

#[derive(Clone, RustcEncodable, RustcDecodable, HashStable, PartialEq)]
pub enum TerminatorKind<'tcx> {
    /// Block should have one successor in the graph; we jump there.
    Goto { target: BasicBlock },

    /// Operand evaluates to an integer; jump depending on its value
    /// to one of the targets, and otherwise fallback to `otherwise`.
    SwitchInt {
        /// The discriminant value being tested.
        discr: Operand<'tcx>,

        /// The type of value being tested.
        switch_ty: Ty<'tcx>,

        /// Possible values. The locations to branch to in each case
        /// are found in the corresponding indices from the `targets` vector.
        values: Cow<'tcx, [u128]>,

        /// Possible branch sites. The last element of this vector is used
        /// for the otherwise branch, so targets.len() == values.len() + 1
        /// should hold.
        //
        // This invariant is quite non-obvious and also could be improved.
        // One way to make this invariant is to have something like this instead:
        //
        // branches: Vec<(ConstInt, BasicBlock)>,
        // otherwise: Option<BasicBlock> // exhaustive if None
        //
        // However we’ve decided to keep this as-is until we figure a case
        // where some other approach seems to be strictly better than other.
        targets: Vec<BasicBlock>,
    },

    /// Indicates that the landing pad is finished and unwinding should
    /// continue. Emitted by `build::scope::diverge_cleanup`.
    Resume,

    /// Indicates that the landing pad is finished and that the process
    /// should abort. Used to prevent unwinding for foreign items.
    Abort,

    /// Indicates a normal return. The return place should have
    /// been filled in by now. This should occur at most once.
    Return,

    /// Indicates a terminator that can never be reached.
    Unreachable,

    /// Drop the `Place`.
    Drop { location: Place<'tcx>, target: BasicBlock, unwind: Option<BasicBlock> },

    /// Drop the `Place` and assign the new value over it. This ensures
    /// that the assignment to `P` occurs *even if* the destructor for
    /// place unwinds. Its semantics are best explained by the
    /// elaboration:
    ///
    /// ```
    /// BB0 {
    ///   DropAndReplace(P <- V, goto BB1, unwind BB2)
    /// }
    /// ```
    ///
    /// becomes
    ///
    /// ```
    /// BB0 {
    ///   Drop(P, goto BB1, unwind BB2)
    /// }
    /// BB1 {
    ///   // P is now uninitialized
    ///   P <- V
    /// }
    /// BB2 {
    ///   // P is now uninitialized -- its dtor panicked
    ///   P <- V
    /// }
    /// ```
    DropAndReplace {
        location: Place<'tcx>,
        value: Operand<'tcx>,
        target: BasicBlock,
        unwind: Option<BasicBlock>,
    },

    /// Block ends with a call of a converging function.
    Call {
        /// The function that’s being called.
        func: Operand<'tcx>,
        /// Arguments the function is called with.
        /// These are owned by the callee, which is free to modify them.
        /// This allows the memory occupied by "by-value" arguments to be
        /// reused across function calls without duplicating the contents.
        args: Vec<Operand<'tcx>>,
        /// Destination for the return value. If some, the call is converging.
        destination: Option<(Place<'tcx>, BasicBlock)>,
        /// Cleanups to be done if the call unwinds.
        cleanup: Option<BasicBlock>,
        /// `true` if this is from a call in HIR rather than from an overloaded
        /// operator. True for overloaded function call.
        from_hir_call: bool,
    },

    /// Jump to the target if the condition has the expected value,
    /// otherwise panic with a message and a cleanup target.
    Assert {
        cond: Operand<'tcx>,
        expected: bool,
        msg: AssertMessage<'tcx>,
        target: BasicBlock,
        cleanup: Option<BasicBlock>,
    },

    /// A suspend point.
    Yield {
        /// The value to return.
        value: Operand<'tcx>,
        /// Where to resume to.
        resume: BasicBlock,
        /// Cleanup to be done if the generator is dropped at this suspend point.
        drop: Option<BasicBlock>,
    },

    /// Indicates the end of the dropping of a generator.
    GeneratorDrop,

    /// A block where control flow only ever takes one real path, but borrowck
    /// needs to be more conservative.
    FalseEdges {
        /// The target normal control flow will take.
        real_target: BasicBlock,
        /// A block control flow could conceptually jump to, but won't in
        /// practice.
        imaginary_target: BasicBlock,
    },
    /// A terminator for blocks that only take one path in reality, but where we
    /// reserve the right to unwind in borrowck, even if it won't happen in practice.
    /// This can arise in infinite loops with no function calls for example.
    FalseUnwind {
        /// The target normal control flow will take.
        real_target: BasicBlock,
        /// The imaginary cleanup block link. This particular path will never be taken
        /// in practice, but in order to avoid fragility we want to always
        /// consider it in borrowck. We don't want to accept programs which
        /// pass borrowck only when `panic=abort` or some assertions are disabled
        /// due to release vs. debug mode builds. This needs to be an `Option` because
        /// of the `remove_noop_landing_pads` and `no_landing_pads` passes.
        unwind: Option<BasicBlock>,
    },
}

pub type Successors<'a> =
    iter::Chain<option::IntoIter<&'a BasicBlock>, slice::Iter<'a, BasicBlock>>;
pub type SuccessorsMut<'a> =
    iter::Chain<option::IntoIter<&'a mut BasicBlock>, slice::IterMut<'a, BasicBlock>>;

impl<'tcx> Terminator<'tcx> {
    pub fn successors(&self) -> Successors<'_> {
        self.kind.successors()
    }

    pub fn successors_mut(&mut self) -> SuccessorsMut<'_> {
        self.kind.successors_mut()
    }

    pub fn unwind(&self) -> Option<&Option<BasicBlock>> {
        self.kind.unwind()
    }

    pub fn unwind_mut(&mut self) -> Option<&mut Option<BasicBlock>> {
        self.kind.unwind_mut()
    }
}

impl<'tcx> TerminatorKind<'tcx> {
    pub fn if_(
        tcx: TyCtxt<'tcx>,
        cond: Operand<'tcx>,
        t: BasicBlock,
        f: BasicBlock,
    ) -> TerminatorKind<'tcx> {
        static BOOL_SWITCH_FALSE: &'static [u128] = &[0];
        TerminatorKind::SwitchInt {
            discr: cond,
            switch_ty: tcx.types.bool,
            values: From::from(BOOL_SWITCH_FALSE),
            targets: vec![f, t],
        }
    }

    pub fn successors(&self) -> Successors<'_> {
        use self::TerminatorKind::*;
        match *self {
            Resume
            | Abort
            | GeneratorDrop
            | Return
            | Unreachable
            | Call { destination: None, cleanup: None, .. } => None.into_iter().chain(&[]),
            Goto { target: ref t }
            | Call { destination: None, cleanup: Some(ref t), .. }
            | Call { destination: Some((_, ref t)), cleanup: None, .. }
            | Yield { resume: ref t, drop: None, .. }
            | DropAndReplace { target: ref t, unwind: None, .. }
            | Drop { target: ref t, unwind: None, .. }
            | Assert { target: ref t, cleanup: None, .. }
            | FalseUnwind { real_target: ref t, unwind: None } => Some(t).into_iter().chain(&[]),
            Call { destination: Some((_, ref t)), cleanup: Some(ref u), .. }
            | Yield { resume: ref t, drop: Some(ref u), .. }
            | DropAndReplace { target: ref t, unwind: Some(ref u), .. }
            | Drop { target: ref t, unwind: Some(ref u), .. }
            | Assert { target: ref t, cleanup: Some(ref u), .. }
            | FalseUnwind { real_target: ref t, unwind: Some(ref u) } => {
                Some(t).into_iter().chain(slice::from_ref(u))
            }
            SwitchInt { ref targets, .. } => None.into_iter().chain(&targets[..]),
            FalseEdges { ref real_target, ref imaginary_target } => {
                Some(real_target).into_iter().chain(slice::from_ref(imaginary_target))
            }
        }
    }

    pub fn successors_mut(&mut self) -> SuccessorsMut<'_> {
        use self::TerminatorKind::*;
        match *self {
            Resume
            | Abort
            | GeneratorDrop
            | Return
            | Unreachable
            | Call { destination: None, cleanup: None, .. } => None.into_iter().chain(&mut []),
            Goto { target: ref mut t }
            | Call { destination: None, cleanup: Some(ref mut t), .. }
            | Call { destination: Some((_, ref mut t)), cleanup: None, .. }
            | Yield { resume: ref mut t, drop: None, .. }
            | DropAndReplace { target: ref mut t, unwind: None, .. }
            | Drop { target: ref mut t, unwind: None, .. }
            | Assert { target: ref mut t, cleanup: None, .. }
            | FalseUnwind { real_target: ref mut t, unwind: None } => {
                Some(t).into_iter().chain(&mut [])
            }
            Call { destination: Some((_, ref mut t)), cleanup: Some(ref mut u), .. }
            | Yield { resume: ref mut t, drop: Some(ref mut u), .. }
            | DropAndReplace { target: ref mut t, unwind: Some(ref mut u), .. }
            | Drop { target: ref mut t, unwind: Some(ref mut u), .. }
            | Assert { target: ref mut t, cleanup: Some(ref mut u), .. }
            | FalseUnwind { real_target: ref mut t, unwind: Some(ref mut u) } => {
                Some(t).into_iter().chain(slice::from_mut(u))
            }
            SwitchInt { ref mut targets, .. } => None.into_iter().chain(&mut targets[..]),
            FalseEdges { ref mut real_target, ref mut imaginary_target } => {
                Some(real_target).into_iter().chain(slice::from_mut(imaginary_target))
            }
        }
    }

    pub fn unwind(&self) -> Option<&Option<BasicBlock>> {
        match *self {
            TerminatorKind::Goto { .. }
            | TerminatorKind::Resume
            | TerminatorKind::Abort
            | TerminatorKind::Return
            | TerminatorKind::Unreachable
            | TerminatorKind::GeneratorDrop
            | TerminatorKind::Yield { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::FalseEdges { .. } => None,
            TerminatorKind::Call { cleanup: ref unwind, .. }
            | TerminatorKind::Assert { cleanup: ref unwind, .. }
            | TerminatorKind::DropAndReplace { ref unwind, .. }
            | TerminatorKind::Drop { ref unwind, .. }
            | TerminatorKind::FalseUnwind { ref unwind, .. } => Some(unwind),
        }
    }

    pub fn unwind_mut(&mut self) -> Option<&mut Option<BasicBlock>> {
        match *self {
            TerminatorKind::Goto { .. }
            | TerminatorKind::Resume
            | TerminatorKind::Abort
            | TerminatorKind::Return
            | TerminatorKind::Unreachable
            | TerminatorKind::GeneratorDrop
            | TerminatorKind::Yield { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::FalseEdges { .. } => None,
            TerminatorKind::Call { cleanup: ref mut unwind, .. }
            | TerminatorKind::Assert { cleanup: ref mut unwind, .. }
            | TerminatorKind::DropAndReplace { ref mut unwind, .. }
            | TerminatorKind::Drop { ref mut unwind, .. }
            | TerminatorKind::FalseUnwind { ref mut unwind, .. } => Some(unwind),
        }
    }
}

impl<'tcx> BasicBlockData<'tcx> {
    pub fn new(terminator: Option<Terminator<'tcx>>) -> BasicBlockData<'tcx> {
        BasicBlockData { statements: vec![], terminator, is_cleanup: false }
    }

    /// Accessor for terminator.
    ///
    /// Terminator may not be None after construction of the basic block is complete. This accessor
    /// provides a convenience way to reach the terminator.
    pub fn terminator(&self) -> &Terminator<'tcx> {
        self.terminator.as_ref().expect("invalid terminator state")
    }

    pub fn terminator_mut(&mut self) -> &mut Terminator<'tcx> {
        self.terminator.as_mut().expect("invalid terminator state")
    }

    pub fn retain_statements<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Statement<'_>) -> bool,
    {
        for s in &mut self.statements {
            if !f(s) {
                s.make_nop();
            }
        }
    }

    pub fn expand_statements<F, I>(&mut self, mut f: F)
    where
        F: FnMut(&mut Statement<'tcx>) -> Option<I>,
        I: iter::TrustedLen<Item = Statement<'tcx>>,
    {
        // Gather all the iterators we'll need to splice in, and their positions.
        let mut splices: Vec<(usize, I)> = vec![];
        let mut extra_stmts = 0;
        for (i, s) in self.statements.iter_mut().enumerate() {
            if let Some(mut new_stmts) = f(s) {
                if let Some(first) = new_stmts.next() {
                    // We can already store the first new statement.
                    *s = first;

                    // Save the other statements for optimized splicing.
                    let remaining = new_stmts.size_hint().0;
                    if remaining > 0 {
                        splices.push((i + 1 + extra_stmts, new_stmts));
                        extra_stmts += remaining;
                    }
                } else {
                    s.make_nop();
                }
            }
        }

        // Splice in the new statements, from the end of the block.
        // FIXME(eddyb) This could be more efficient with a "gap buffer"
        // where a range of elements ("gap") is left uninitialized, with
        // splicing adding new elements to the end of that gap and moving
        // existing elements from before the gap to the end of the gap.
        // For now, this is safe code, emulating a gap but initializing it.
        let mut gap = self.statements.len()..self.statements.len() + extra_stmts;
        self.statements.resize(
            gap.end,
            Statement {
                source_info: SourceInfo { span: DUMMY_SP, scope: OUTERMOST_SOURCE_SCOPE },
                kind: StatementKind::Nop,
            },
        );
        for (splice_start, new_stmts) in splices.into_iter().rev() {
            let splice_end = splice_start + new_stmts.size_hint().0;
            while gap.end > splice_end {
                gap.start -= 1;
                gap.end -= 1;
                self.statements.swap(gap.start, gap.end);
            }
            self.statements.splice(splice_start..splice_end, new_stmts);
            gap.end = splice_start;
        }
    }

    pub fn visitable(&self, index: usize) -> &dyn MirVisitable<'tcx> {
        if index < self.statements.len() {
            &self.statements[index]
        } else {
            &self.terminator
        }
    }
}

impl<'tcx> Debug for TerminatorKind<'tcx> {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        self.fmt_head(fmt)?;
        let successor_count = self.successors().count();
        let labels = self.fmt_successor_labels();
        assert_eq!(successor_count, labels.len());

        match successor_count {
            0 => Ok(()),

            1 => write!(fmt, " -> {:?}", self.successors().nth(0).unwrap()),

            _ => {
                write!(fmt, " -> [")?;
                for (i, target) in self.successors().enumerate() {
                    if i > 0 {
                        write!(fmt, ", ")?;
                    }
                    write!(fmt, "{}: {:?}", labels[i], target)?;
                }
                write!(fmt, "]")
            }
        }
    }
}

impl<'tcx> TerminatorKind<'tcx> {
    /// Writes the "head" part of the terminator; that is, its name and the data it uses to pick the
    /// successor basic block, if any. The only information not included is the list of possible
    /// successors, which may be rendered differently between the text and the graphviz format.
    pub fn fmt_head<W: Write>(&self, fmt: &mut W) -> fmt::Result {
        use self::TerminatorKind::*;
        match *self {
            Goto { .. } => write!(fmt, "goto"),
            SwitchInt { discr: ref place, .. } => write!(fmt, "switchInt({:?})", place),
            Return => write!(fmt, "return"),
            GeneratorDrop => write!(fmt, "generator_drop"),
            Resume => write!(fmt, "resume"),
            Abort => write!(fmt, "abort"),
            Yield { ref value, .. } => write!(fmt, "_1 = suspend({:?})", value),
            Unreachable => write!(fmt, "unreachable"),
            Drop { ref location, .. } => write!(fmt, "drop({:?})", location),
            DropAndReplace { ref location, ref value, .. } => {
                write!(fmt, "replace({:?} <- {:?})", location, value)
            }
            Call { ref func, ref args, ref destination, .. } => {
                if let Some((ref destination, _)) = *destination {
                    write!(fmt, "{:?} = ", destination)?;
                }
                write!(fmt, "{:?}(", func)?;
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        write!(fmt, ", ")?;
                    }
                    write!(fmt, "{:?}", arg)?;
                }
                write!(fmt, ")")
            }
            Assert { ref cond, expected, ref msg, .. } => {
                write!(fmt, "assert(")?;
                if !expected {
                    write!(fmt, "!")?;
                }
                write!(fmt, "{:?}, \"{:?}\")", cond, msg)
            }
            FalseEdges { .. } => write!(fmt, "falseEdges"),
            FalseUnwind { .. } => write!(fmt, "falseUnwind"),
        }
    }

    /// Returns the list of labels for the edges to the successor basic blocks.
    pub fn fmt_successor_labels(&self) -> Vec<Cow<'static, str>> {
        use self::TerminatorKind::*;
        match *self {
            Return | Resume | Abort | Unreachable | GeneratorDrop => vec![],
            Goto { .. } => vec!["".into()],
            SwitchInt { ref values, switch_ty, .. } => ty::tls::with(|tcx| {
                let param_env = ty::ParamEnv::empty();
                let switch_ty = tcx.lift(&switch_ty).unwrap();
                let size = tcx.layout_of(param_env.and(switch_ty)).unwrap().size;
                values
                    .iter()
                    .map(|&u| {
                        ty::Const::from_scalar(
                            tcx,
                            Scalar::from_uint(u, size).into(),
                            switch_ty,
                        )
                        .to_string()
                        .into()
                    })
                    .chain(iter::once("otherwise".into()))
                    .collect()
            }),
            Call { destination: Some(_), cleanup: Some(_), .. } => {
                vec!["return".into(), "unwind".into()]
            }
            Call { destination: Some(_), cleanup: None, .. } => vec!["return".into()],
            Call { destination: None, cleanup: Some(_), .. } => vec!["unwind".into()],
            Call { destination: None, cleanup: None, .. } => vec![],
            Yield { drop: Some(_), .. } => vec!["resume".into(), "drop".into()],
            Yield { drop: None, .. } => vec!["resume".into()],
            DropAndReplace { unwind: None, .. } | Drop { unwind: None, .. } => {
                vec!["return".into()]
            }
            DropAndReplace { unwind: Some(_), .. } | Drop { unwind: Some(_), .. } => {
                vec!["return".into(), "unwind".into()]
            }
            Assert { cleanup: None, .. } => vec!["".into()],
            Assert { .. } => vec!["success".into(), "unwind".into()],
            FalseEdges { .. } => vec!["real".into(), "imaginary".into()],
            FalseUnwind { unwind: Some(_), .. } => vec!["real".into(), "cleanup".into()],
            FalseUnwind { unwind: None, .. } => vec!["real".into()],
        }
    }
}

///////////////////////////////////////////////////////////////////////////
// Statements

#[derive(Clone, RustcEncodable, RustcDecodable, HashStable, TypeFoldable)]
pub struct Statement<'tcx> {
    pub source_info: SourceInfo,
    pub kind: StatementKind<'tcx>,
}

// `Statement` is used a lot. Make sure it doesn't unintentionally get bigger.
#[cfg(target_arch = "x86_64")]
static_assert_size!(Statement<'_>, 32);

impl Statement<'_> {
    /// Changes a statement to a nop. This is both faster than deleting instructions and avoids
    /// invalidating statement indices in `Location`s.
    pub fn make_nop(&mut self) {
        self.kind = StatementKind::Nop
    }

    /// Changes a statement to a nop and returns the original statement.
    pub fn replace_nop(&mut self) -> Self {
        Statement {
            source_info: self.source_info,
            kind: mem::replace(&mut self.kind, StatementKind::Nop),
        }
    }
}

#[derive(Clone, Debug, PartialEq, RustcEncodable, RustcDecodable, HashStable, TypeFoldable)]
pub enum StatementKind<'tcx> {
    /// Write the RHS Rvalue to the LHS Place.
    Assign(Box<(Place<'tcx>, Rvalue<'tcx>)>),

    /// This represents all the reading that a pattern match may do
    /// (e.g., inspecting constants and discriminant values), and the
    /// kind of pattern it comes from. This is in order to adapt potential
    /// error messages to these specific patterns.
    ///
    /// Note that this also is emitted for regular `let` bindings to ensure that locals that are
    /// never accessed still get some sanity checks for, e.g., `let x: ! = ..;`
    FakeRead(FakeReadCause, Box<Place<'tcx>>),

    /// Write the discriminant for a variant to the enum Place.
    SetDiscriminant { place: Box<Place<'tcx>>, variant_index: VariantIdx },

    /// Start a live range for the storage of the local.
    StorageLive(Local),

    /// End the current live range for the storage of the local.
    StorageDead(Local),

    /// Executes a piece of inline Assembly. Stored in a Box to keep the size
    /// of `StatementKind` low.
    InlineAsm(Box<InlineAsm<'tcx>>),

    /// Retag references in the given place, ensuring they got fresh tags. This is
    /// part of the Stacked Borrows model. These statements are currently only interpreted
    /// by miri and only generated when "-Z mir-emit-retag" is passed.
    /// See <https://internals.rust-lang.org/t/stacked-borrows-an-aliasing-model-for-rust/8153/>
    /// for more details.
    Retag(RetagKind, Box<Place<'tcx>>),

    /// Encodes a user's type ascription. These need to be preserved
    /// intact so that NLL can respect them. For example:
    ///
    ///     let a: T = y;
    ///
    /// The effect of this annotation is to relate the type `T_y` of the place `y`
    /// to the user-given type `T`. The effect depends on the specified variance:
    ///
    /// - `Covariant` -- requires that `T_y <: T`
    /// - `Contravariant` -- requires that `T_y :> T`
    /// - `Invariant` -- requires that `T_y == T`
    /// - `Bivariant` -- no effect
    AscribeUserType(Box<(Place<'tcx>, UserTypeProjection)>, ty::Variance),

    /// No-op. Useful for deleting instructions without affecting statement indices.
    Nop,
}

/// Describes what kind of retag is to be performed.
#[derive(Copy, Clone, RustcEncodable, RustcDecodable, Debug, PartialEq, Eq, HashStable)]
pub enum RetagKind {
    /// The initial retag when entering a function.
    FnEntry,
    /// Retag preparing for a two-phase borrow.
    TwoPhase,
    /// Retagging raw pointers.
    Raw,
    /// A "normal" retag.
    Default,
}

/// The `FakeReadCause` describes the type of pattern why a FakeRead statement exists.
#[derive(Copy, Clone, RustcEncodable, RustcDecodable, Debug, HashStable, PartialEq)]
pub enum FakeReadCause {
    /// Inject a fake read of the borrowed input at the end of each guards
    /// code.
    ///
    /// This should ensure that you cannot change the variant for an enum while
    /// you are in the midst of matching on it.
    ForMatchGuard,

    /// `let x: !; match x {}` doesn't generate any read of x so we need to
    /// generate a read of x to check that it is initialized and safe.
    ForMatchedPlace,

    /// A fake read of the RefWithinGuard version of a bind-by-value variable
    /// in a match guard to ensure that it's value hasn't change by the time
    /// we create the OutsideGuard version.
    ForGuardBinding,

    /// Officially, the semantics of
    ///
    /// `let pattern = <expr>;`
    ///
    /// is that `<expr>` is evaluated into a temporary and then this temporary is
    /// into the pattern.
    ///
    /// However, if we see the simple pattern `let var = <expr>`, we optimize this to
    /// evaluate `<expr>` directly into the variable `var`. This is mostly unobservable,
    /// but in some cases it can affect the borrow checker, as in #53695.
    /// Therefore, we insert a "fake read" here to ensure that we get
    /// appropriate errors.
    ForLet,

    /// If we have an index expression like
    ///
    /// (*x)[1][{ x = y; 4}]
    ///
    /// then the first bounds check is invalidated when we evaluate the second
    /// index expression. Thus we create a fake borrow of `x` across the second
    /// indexer, which will cause a borrow check error.
    ForIndex,
}

#[derive(Clone, Debug, PartialEq, RustcEncodable, RustcDecodable, HashStable, TypeFoldable)]
pub struct InlineAsm<'tcx> {
    pub asm: hir::InlineAsmInner,
    pub outputs: Box<[Place<'tcx>]>,
    pub inputs: Box<[(Span, Operand<'tcx>)]>,
}

impl Debug for Statement<'_> {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        use self::StatementKind::*;
        match self.kind {
            Assign(box(ref place, ref rv)) => write!(fmt, "{:?} = {:?}", place, rv),
            FakeRead(ref cause, ref place) => write!(fmt, "FakeRead({:?}, {:?})", cause, place),
            Retag(ref kind, ref place) => write!(
                fmt,
                "Retag({}{:?})",
                match kind {
                    RetagKind::FnEntry => "[fn entry] ",
                    RetagKind::TwoPhase => "[2phase] ",
                    RetagKind::Raw => "[raw] ",
                    RetagKind::Default => "",
                },
                place,
            ),
            StorageLive(ref place) => write!(fmt, "StorageLive({:?})", place),
            StorageDead(ref place) => write!(fmt, "StorageDead({:?})", place),
            SetDiscriminant { ref place, variant_index } => {
                write!(fmt, "discriminant({:?}) = {:?}", place, variant_index)
            }
            InlineAsm(ref asm) => {
                write!(fmt, "asm!({:?} : {:?} : {:?})", asm.asm, asm.outputs, asm.inputs)
            }
            AscribeUserType(box(ref place, ref c_ty), ref variance) => {
                write!(fmt, "AscribeUserType({:?}, {:?}, {:?})", place, variance, c_ty)
            }
            Nop => write!(fmt, "nop"),
        }
    }
}

///////////////////////////////////////////////////////////////////////////
// Places

/// A path to a value; something that can be evaluated without
/// changing or disturbing program state.
#[derive(
    Clone, PartialEq, Eq, PartialOrd, Ord, Hash, RustcEncodable, HashStable,
)]
pub struct Place<'tcx> {
    pub base: PlaceBase<'tcx>,

    /// projection out of a place (access a field, deref a pointer, etc)
    pub projection: &'tcx List<PlaceElem<'tcx>>,
}

impl<'tcx> rustc_serialize::UseSpecializedDecodable for Place<'tcx> {}

#[derive(
    Clone, PartialEq, Eq, PartialOrd, Ord, Hash, RustcEncodable, RustcDecodable, HashStable,
)]
pub enum PlaceBase<'tcx> {
    /// local variable
    Local(Local),

    /// static or static mut variable
    Static(Box<Static<'tcx>>),
}

/// We store the normalized type to avoid requiring normalization when reading MIR
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
         RustcEncodable, RustcDecodable, HashStable)]
pub struct Static<'tcx> {
    pub ty: Ty<'tcx>,
    pub kind: StaticKind<'tcx>,
    /// The `DefId` of the item this static was declared in. For promoted values, usually, this is
    /// the same as the `DefId` of the `mir::Body` containing the `Place` this promoted appears in.
    /// However, after inlining, that might no longer be the case as inlined `Place`s are copied
    /// into the calling frame.
    pub def_id: DefId,
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, HashStable, RustcEncodable, RustcDecodable,
)]
pub enum StaticKind<'tcx> {
    /// Promoted references consist of an id (`Promoted`) and the substs necessary to monomorphize
    /// it. Usually, these substs are just the identity substs for the item. However, the inliner
    /// will adjust these substs when it inlines a function based on the substs at the callsite.
    Promoted(Promoted, SubstsRef<'tcx>),
    Static,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(RustcEncodable, RustcDecodable, HashStable)]
pub enum ProjectionElem<V, T> {
    Deref,
    Field(Field, T),
    Index(V),

    /// These indices are generated by slice patterns. Easiest to explain
    /// by example:
    ///
    /// ```
    /// [X, _, .._, _, _] => { offset: 0, min_length: 4, from_end: false },
    /// [_, X, .._, _, _] => { offset: 1, min_length: 4, from_end: false },
    /// [_, _, .._, X, _] => { offset: 2, min_length: 4, from_end: true },
    /// [_, _, .._, _, X] => { offset: 1, min_length: 4, from_end: true },
    /// ```
    ConstantIndex {
        /// index or -index (in Python terms), depending on from_end
        offset: u32,
        /// thing being indexed must be at least this long
        min_length: u32,
        /// counting backwards from end?
        from_end: bool,
    },

    /// These indices are generated by slice patterns.
    ///
    /// slice[from:-to] in Python terms.
    Subslice {
        from: u32,
        to: u32,
    },

    /// "Downcast" to a variant of an ADT. Currently, we only introduce
    /// this for ADTs with more than one variant. It may be better to
    /// just introduce it always, or always for enums.
    ///
    /// The included Symbol is the name of the variant, used for printing MIR.
    Downcast(Option<Symbol>, VariantIdx),
}

impl<V, T> ProjectionElem<V, T> {
    /// Returns `true` if the target of this projection may refer to a different region of memory
    /// than the base.
    fn is_indirect(&self) -> bool {
        match self {
            Self::Deref => true,

            | Self::Field(_, _)
            | Self::Index(_)
            | Self::ConstantIndex { .. }
            | Self::Subslice { .. }
            | Self::Downcast(_, _)
            => false
        }
    }
}

/// Alias for projections as they appear in places, where the base is a place
/// and the index is a local.
pub type PlaceElem<'tcx> = ProjectionElem<Local, Ty<'tcx>>;

impl<'tcx> Copy for PlaceElem<'tcx> { }

// At least on 64 bit systems, `PlaceElem` should not be larger than two pointers.
#[cfg(target_arch = "x86_64")]
static_assert_size!(PlaceElem<'_>, 16);

/// Alias for projections as they appear in `UserTypeProjection`, where we
/// need neither the `V` parameter for `Index` nor the `T` for `Field`.
pub type ProjectionKind = ProjectionElem<(), ()>;

rustc_index::newtype_index! {
    pub struct Field {
        derive [HashStable]
        DEBUG_FORMAT = "field[{}]"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlaceRef<'a, 'tcx> {
    pub base: &'a PlaceBase<'tcx>,
    pub projection: &'a [PlaceElem<'tcx>],
}

impl<'tcx> Place<'tcx> {
    // FIXME change this to a const fn by also making List::empty a const fn.
    pub fn return_place() -> Place<'tcx> {
        Place {
            base: PlaceBase::Local(RETURN_PLACE),
            projection: List::empty(),
        }
    }

    /// Returns `true` if this `Place` contains a `Deref` projection.
    ///
    /// If `Place::is_indirect` returns false, the caller knows that the `Place` refers to the
    /// same region of memory as its base.
    pub fn is_indirect(&self) -> bool {
        self.projection.iter().any(|elem| elem.is_indirect())
    }

    /// Finds the innermost `Local` from this `Place`, *if* it is either a local itself or
    /// a single deref of a local.
    //
    // FIXME: can we safely swap the semantics of `fn base_local` below in here instead?
    pub fn local_or_deref_local(&self) -> Option<Local> {
        match self.as_ref() {
            PlaceRef {
                base: &PlaceBase::Local(local),
                projection: &[],
            } |
            PlaceRef {
                base: &PlaceBase::Local(local),
                projection: &[ProjectionElem::Deref],
            } => Some(local),
            _ => None,
        }
    }

    /// If this place represents a local variable like `_X` with no
    /// projections, return `Some(_X)`.
    pub fn as_local(&self) -> Option<Local> {
        self.as_ref().as_local()
    }

    pub fn as_ref(&self) -> PlaceRef<'_, 'tcx> {
        PlaceRef {
            base: &self.base,
            projection: &self.projection,
        }
    }
}

impl From<Local> for Place<'_> {
    fn from(local: Local) -> Self {
        Place {
            base: local.into(),
            projection: List::empty(),
        }
    }
}

impl From<Local> for PlaceBase<'_> {
    fn from(local: Local) -> Self {
        PlaceBase::Local(local)
    }
}

impl<'a, 'tcx> PlaceRef<'a, 'tcx> {
    /// Finds the innermost `Local` from this `Place`, *if* it is either a local itself or
    /// a single deref of a local.
    //
    // FIXME: can we safely swap the semantics of `fn base_local` below in here instead?
    pub fn local_or_deref_local(&self) -> Option<Local> {
        match self {
            PlaceRef {
                base: PlaceBase::Local(local),
                projection: [],
            } |
            PlaceRef {
                base: PlaceBase::Local(local),
                projection: [ProjectionElem::Deref],
            } => Some(*local),
            _ => None,
        }
    }

    /// If this place represents a local variable like `_X` with no
    /// projections, return `Some(_X)`.
    pub fn as_local(&self) -> Option<Local> {
        match self {
            PlaceRef { base: PlaceBase::Local(l), projection: [] } => Some(*l),
            _ => None,
        }
    }
}

impl Debug for Place<'_> {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        for elem in self.projection.iter().rev() {
            match elem {
                ProjectionElem::Downcast(_, _) | ProjectionElem::Field(_, _) => {
                    write!(fmt, "(").unwrap();
                }
                ProjectionElem::Deref => {
                    write!(fmt, "(*").unwrap();
                }
                ProjectionElem::Index(_)
                | ProjectionElem::ConstantIndex { .. }
                | ProjectionElem::Subslice { .. } => {}
            }
        }

        write!(fmt, "{:?}", self.base)?;

        for elem in self.projection.iter() {
            match elem {
                ProjectionElem::Downcast(Some(name), _index) => {
                    write!(fmt, " as {})", name)?;
                }
                ProjectionElem::Downcast(None, index) => {
                    write!(fmt, " as variant#{:?})", index)?;
                }
                ProjectionElem::Deref => {
                    write!(fmt, ")")?;
                }
                ProjectionElem::Field(field, ty) => {
                    write!(fmt, ".{:?}: {:?})", field.index(), ty)?;
                }
                ProjectionElem::Index(ref index) => {
                    write!(fmt, "[{:?}]", index)?;
                }
                ProjectionElem::ConstantIndex { offset, min_length, from_end: false } => {
                    write!(fmt, "[{:?} of {:?}]", offset, min_length)?;
                }
                ProjectionElem::ConstantIndex { offset, min_length, from_end: true } => {
                    write!(fmt, "[-{:?} of {:?}]", offset, min_length)?;
                }
                ProjectionElem::Subslice { from, to } if *to == 0 => {
                    write!(fmt, "[{:?}:]", from)?;
                }
                ProjectionElem::Subslice { from, to } if *from == 0 => {
                    write!(fmt, "[:-{:?}]", to)?;
                }
                ProjectionElem::Subslice { from, to } => {
                    write!(fmt, "[{:?}:-{:?}]", from, to)?;
                }
            }
        }

        Ok(())
    }
}

impl Debug for PlaceBase<'_> {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        match *self {
            PlaceBase::Local(id) => write!(fmt, "{:?}", id),
            PlaceBase::Static(box self::Static { ty, kind: StaticKind::Static, def_id }) => {
                write!(fmt, "({}: {:?})", ty::tls::with(|tcx| tcx.def_path_str(def_id)), ty)
            }
            PlaceBase::Static(box self::Static {
                ty, kind: StaticKind::Promoted(promoted, _), def_id: _
            }) => {
                write!(fmt, "({:?}: {:?})", promoted, ty)
            }
        }
    }
}

///////////////////////////////////////////////////////////////////////////
// Scopes

rustc_index::newtype_index! {
    pub struct SourceScope {
        derive [HashStable]
        DEBUG_FORMAT = "scope[{}]",
        const OUTERMOST_SOURCE_SCOPE = 0,
    }
}

#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub struct SourceScopeData {
    pub span: Span,
    pub parent_scope: Option<SourceScope>,
}

#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub struct SourceScopeLocalData {
    /// An `HirId` with lint levels equivalent to this scope's lint levels.
    pub lint_root: hir::HirId,
    /// The unsafe block that contains this node.
    pub safety: Safety,
}

///////////////////////////////////////////////////////////////////////////
// Operands

/// These are values that can appear inside an rvalue. They are intentionally
/// limited to prevent rvalues from being nested in one another.
#[derive(Clone, PartialEq, RustcEncodable, RustcDecodable, HashStable)]
pub enum Operand<'tcx> {
    /// Copy: The value must be available for use afterwards.
    ///
    /// This implies that the type of the place must be `Copy`; this is true
    /// by construction during build, but also checked by the MIR type checker.
    Copy(Place<'tcx>),

    /// Move: The value (including old borrows of it) will not be used again.
    ///
    /// Safe for values of all types (modulo future developments towards `?Move`).
    /// Correct usage patterns are enforced by the borrow checker for safe code.
    /// `Copy` may be converted to `Move` to enable "last-use" optimizations.
    Move(Place<'tcx>),

    /// Synthesizes a constant value.
    Constant(Box<Constant<'tcx>>),
}

impl<'tcx> Debug for Operand<'tcx> {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        use self::Operand::*;
        match *self {
            Constant(ref a) => write!(fmt, "{:?}", a),
            Copy(ref place) => write!(fmt, "{:?}", place),
            Move(ref place) => write!(fmt, "move {:?}", place),
        }
    }
}

impl<'tcx> Operand<'tcx> {
    /// Convenience helper to make a constant that refers to the fn
    /// with given `DefId` and substs. Since this is used to synthesize
    /// MIR, assumes `user_ty` is None.
    pub fn function_handle(
        tcx: TyCtxt<'tcx>,
        def_id: DefId,
        substs: SubstsRef<'tcx>,
        span: Span,
    ) -> Self {
        let ty = tcx.type_of(def_id).subst(tcx, substs);
        Operand::Constant(box Constant {
            span,
            user_ty: None,
            literal: ty::Const::zero_sized(tcx, ty),
        })
    }

    pub fn to_copy(&self) -> Self {
        match *self {
            Operand::Copy(_) | Operand::Constant(_) => self.clone(),
            Operand::Move(ref place) => Operand::Copy(place.clone()),
        }
    }
}

///////////////////////////////////////////////////////////////////////////
/// Rvalues

#[derive(Clone, RustcEncodable, RustcDecodable, HashStable, PartialEq)]
pub enum Rvalue<'tcx> {
    /// x (either a move or copy, depending on type of x)
    Use(Operand<'tcx>),

    /// [x; 32]
    Repeat(Operand<'tcx>, u64),

    /// &x or &mut x
    Ref(Region<'tcx>, BorrowKind, Place<'tcx>),

    /// length of a [X] or [X;n] value
    Len(Place<'tcx>),

    Cast(CastKind, Operand<'tcx>, Ty<'tcx>),

    BinaryOp(BinOp, Operand<'tcx>, Operand<'tcx>),
    CheckedBinaryOp(BinOp, Operand<'tcx>, Operand<'tcx>),

    NullaryOp(NullOp, Ty<'tcx>),
    UnaryOp(UnOp, Operand<'tcx>),

    /// Read the discriminant of an ADT.
    ///
    /// Undefined (i.e., no effort is made to make it defined, but there’s no reason why it cannot
    /// be defined to return, say, a 0) if ADT is not an enum.
    Discriminant(Place<'tcx>),

    /// Creates an aggregate value, like a tuple or struct. This is
    /// only needed because we want to distinguish `dest = Foo { x:
    /// ..., y: ... }` from `dest.x = ...; dest.y = ...;` in the case
    /// that `Foo` has a destructor. These rvalues can be optimized
    /// away after type-checking and before lowering.
    Aggregate(Box<AggregateKind<'tcx>>, Vec<Operand<'tcx>>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, RustcEncodable, RustcDecodable, HashStable)]
pub enum CastKind {
    Misc,
    Pointer(PointerCast),
}

#[derive(Clone, Debug, PartialEq, Eq, RustcEncodable, RustcDecodable, HashStable)]
pub enum AggregateKind<'tcx> {
    /// The type is of the element
    Array(Ty<'tcx>),
    Tuple,

    /// The second field is the variant index. It's equal to 0 for struct
    /// and union expressions. The fourth field is
    /// active field number and is present only for union expressions
    /// -- e.g., for a union expression `SomeUnion { c: .. }`, the
    /// active field index would identity the field `c`
    Adt(&'tcx AdtDef, VariantIdx, SubstsRef<'tcx>, Option<UserTypeAnnotationIndex>, Option<usize>),

    Closure(DefId, SubstsRef<'tcx>),
    Generator(DefId, SubstsRef<'tcx>, hir::Movability),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, RustcEncodable, RustcDecodable, HashStable)]
pub enum BinOp {
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
    /// The `ptr.offset` operator
    Offset,
}

impl BinOp {
    pub fn is_checkable(self) -> bool {
        use self::BinOp::*;
        match self {
            Add | Sub | Mul | Shl | Shr => true,
            _ => false,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, RustcEncodable, RustcDecodable, HashStable)]
pub enum NullOp {
    /// Returns the size of a value of that type
    SizeOf,
    /// Creates a new uninitialized box for a value of that type
    Box,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, RustcEncodable, RustcDecodable, HashStable)]
pub enum UnOp {
    /// The `!` operator for logical inversion
    Not,
    /// The `-` operator for negation
    Neg,
}

impl<'tcx> Debug for Rvalue<'tcx> {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        use self::Rvalue::*;

        match *self {
            Use(ref place) => write!(fmt, "{:?}", place),
            Repeat(ref a, ref b) => write!(fmt, "[{:?}; {:?}]", a, b),
            Len(ref a) => write!(fmt, "Len({:?})", a),
            Cast(ref kind, ref place, ref ty) => {
                write!(fmt, "{:?} as {:?} ({:?})", place, ty, kind)
            }
            BinaryOp(ref op, ref a, ref b) => write!(fmt, "{:?}({:?}, {:?})", op, a, b),
            CheckedBinaryOp(ref op, ref a, ref b) => {
                write!(fmt, "Checked{:?}({:?}, {:?})", op, a, b)
            }
            UnaryOp(ref op, ref a) => write!(fmt, "{:?}({:?})", op, a),
            Discriminant(ref place) => write!(fmt, "discriminant({:?})", place),
            NullaryOp(ref op, ref t) => write!(fmt, "{:?}({:?})", op, t),
            Ref(region, borrow_kind, ref place) => {
                let kind_str = match borrow_kind {
                    BorrowKind::Shared => "",
                    BorrowKind::Shallow => "shallow ",
                    BorrowKind::Mut { .. } | BorrowKind::Unique => "mut ",
                };

                // When printing regions, add trailing space if necessary.
                let print_region = ty::tls::with(|tcx| {
                    tcx.sess.verbose() || tcx.sess.opts.debugging_opts.identify_regions
                });
                let region = if print_region {
                    let mut region = region.to_string();
                    if region.len() > 0 {
                        region.push(' ');
                    }
                    region
                } else {
                    // Do not even print 'static
                    String::new()
                };
                write!(fmt, "&{}{}{:?}", region, kind_str, place)
            }

            Aggregate(ref kind, ref places) => {
                fn fmt_tuple(fmt: &mut Formatter<'_>, places: &[Operand<'_>]) -> fmt::Result {
                    let mut tuple_fmt = fmt.debug_tuple("");
                    for place in places {
                        tuple_fmt.field(place);
                    }
                    tuple_fmt.finish()
                }

                match **kind {
                    AggregateKind::Array(_) => write!(fmt, "{:?}", places),

                    AggregateKind::Tuple => match places.len() {
                        0 => write!(fmt, "()"),
                        1 => write!(fmt, "({:?},)", places[0]),
                        _ => fmt_tuple(fmt, places),
                    },

                    AggregateKind::Adt(adt_def, variant, substs, _user_ty, _) => {
                        let variant_def = &adt_def.variants[variant];

                        let f = &mut *fmt;
                        ty::tls::with(|tcx| {
                            let substs = tcx.lift(&substs).expect("could not lift for printing");
                            FmtPrinter::new(tcx, f, Namespace::ValueNS)
                                .print_def_path(variant_def.def_id, substs)?;
                            Ok(())
                        })?;

                        match variant_def.ctor_kind {
                            CtorKind::Const => Ok(()),
                            CtorKind::Fn => fmt_tuple(fmt, places),
                            CtorKind::Fictive => {
                                let mut struct_fmt = fmt.debug_struct("");
                                for (field, place) in variant_def.fields.iter().zip(places) {
                                    struct_fmt.field(&field.ident.as_str(), place);
                                }
                                struct_fmt.finish()
                            }
                        }
                    }

                    AggregateKind::Closure(def_id, _) => ty::tls::with(|tcx| {
                        if let Some(hir_id) = tcx.hir().as_local_hir_id(def_id) {
                            let name = if tcx.sess.opts.debugging_opts.span_free_formats {
                                format!("[closure@{:?}]", hir_id)
                            } else {
                                format!("[closure@{:?}]", tcx.hir().span(hir_id))
                            };
                            let mut struct_fmt = fmt.debug_struct(&name);

                            if let Some(upvars) = tcx.upvars(def_id) {
                                for (&var_id, place) in upvars.keys().zip(places) {
                                    let var_name = tcx.hir().name(var_id);
                                    struct_fmt.field(&var_name.as_str(), place);
                                }
                            }

                            struct_fmt.finish()
                        } else {
                            write!(fmt, "[closure]")
                        }
                    }),

                    AggregateKind::Generator(def_id, _, _) => ty::tls::with(|tcx| {
                        if let Some(hir_id) = tcx.hir().as_local_hir_id(def_id) {
                            let name = format!("[generator@{:?}]", tcx.hir().span(hir_id));
                            let mut struct_fmt = fmt.debug_struct(&name);

                            if let Some(upvars) = tcx.upvars(def_id) {
                                for (&var_id, place) in upvars.keys().zip(places) {
                                    let var_name = tcx.hir().name(var_id);
                                    struct_fmt.field(&var_name.as_str(), place);
                                }
                            }

                            struct_fmt.finish()
                        } else {
                            write!(fmt, "[generator]")
                        }
                    }),
                }
            }
        }
    }
}

///////////////////////////////////////////////////////////////////////////
/// Constants
///
/// Two constants are equal if they are the same constant. Note that
/// this does not necessarily mean that they are "==" in Rust -- in
/// particular one must be wary of `NaN`!

#[derive(Clone, PartialEq, RustcEncodable, RustcDecodable, HashStable)]
pub struct Constant<'tcx> {
    pub span: Span,

    /// Optional user-given type: for something like
    /// `collect::<Vec<_>>`, this would be present and would
    /// indicate that `Vec<_>` was explicitly specified.
    ///
    /// Needed for NLL to impose user-given type constraints.
    pub user_ty: Option<UserTypeAnnotationIndex>,

    pub literal: &'tcx ty::Const<'tcx>,
}

impl Constant<'tcx> {
    pub fn check_static_ptr(&self, tcx: TyCtxt<'_>) -> Option<DefId> {
        match self.literal.val.try_to_scalar() {
            Some(Scalar::Ptr(ptr)) => match tcx.alloc_map.lock().get(ptr.alloc_id) {
                Some(GlobalAlloc::Static(def_id)) => Some(def_id),
                Some(_) => None,
                None => {
                    tcx.sess.delay_span_bug(
                        DUMMY_SP, "MIR cannot contain dangling const pointers",
                    );
                    None
                },
            },
            _ => None,
        }
    }
}

/// A collection of projections into user types.
///
/// They are projections because a binding can occur a part of a
/// parent pattern that has been ascribed a type.
///
/// Its a collection because there can be multiple type ascriptions on
/// the path from the root of the pattern down to the binding itself.
///
/// An example:
///
/// ```rust
/// struct S<'a>((i32, &'a str), String);
/// let S((_, w): (i32, &'static str), _): S = ...;
/// //    ------  ^^^^^^^^^^^^^^^^^^^ (1)
/// //  ---------------------------------  ^ (2)
/// ```
///
/// The highlights labelled `(1)` show the subpattern `(_, w)` being
/// ascribed the type `(i32, &'static str)`.
///
/// The highlights labelled `(2)` show the whole pattern being
/// ascribed the type `S`.
///
/// In this example, when we descend to `w`, we will have built up the
/// following two projected types:
///
///   * base: `S`,                   projection: `(base.0).1`
///   * base: `(i32, &'static str)`, projection: `base.1`
///
/// The first will lead to the constraint `w: &'1 str` (for some
/// inferred region `'1`). The second will lead to the constraint `w:
/// &'static str`.
#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable, TypeFoldable)]
pub struct UserTypeProjections {
    pub(crate) contents: Vec<(UserTypeProjection, Span)>,
}

impl<'tcx> UserTypeProjections {
    pub fn none() -> Self {
        UserTypeProjections { contents: vec![] }
    }

    pub fn from_projections(projs: impl Iterator<Item = (UserTypeProjection, Span)>) -> Self {
        UserTypeProjections { contents: projs.collect() }
    }

    pub fn projections_and_spans(&self) -> impl Iterator<Item = &(UserTypeProjection, Span)> {
        self.contents.iter()
    }

    pub fn projections(&self) -> impl Iterator<Item = &UserTypeProjection> {
        self.contents.iter().map(|&(ref user_type, _span)| user_type)
    }

    pub fn push_projection(mut self, user_ty: &UserTypeProjection, span: Span) -> Self {
        self.contents.push((user_ty.clone(), span));
        self
    }

    fn map_projections(
        mut self,
        mut f: impl FnMut(UserTypeProjection) -> UserTypeProjection,
    ) -> Self {
        self.contents = self.contents.drain(..).map(|(proj, span)| (f(proj), span)).collect();
        self
    }

    pub fn index(self) -> Self {
        self.map_projections(|pat_ty_proj| pat_ty_proj.index())
    }

    pub fn subslice(self, from: u32, to: u32) -> Self {
        self.map_projections(|pat_ty_proj| pat_ty_proj.subslice(from, to))
    }

    pub fn deref(self) -> Self {
        self.map_projections(|pat_ty_proj| pat_ty_proj.deref())
    }

    pub fn leaf(self, field: Field) -> Self {
        self.map_projections(|pat_ty_proj| pat_ty_proj.leaf(field))
    }

    pub fn variant(self, adt_def: &'tcx AdtDef, variant_index: VariantIdx, field: Field) -> Self {
        self.map_projections(|pat_ty_proj| pat_ty_proj.variant(adt_def, variant_index, field))
    }
}

/// Encodes the effect of a user-supplied type annotation on the
/// subcomponents of a pattern. The effect is determined by applying the
/// given list of proejctions to some underlying base type. Often,
/// the projection element list `projs` is empty, in which case this
/// directly encodes a type in `base`. But in the case of complex patterns with
/// subpatterns and bindings, we want to apply only a *part* of the type to a variable,
/// in which case the `projs` vector is used.
///
/// Examples:
///
/// * `let x: T = ...` -- here, the `projs` vector is empty.
///
/// * `let (x, _): T = ...` -- here, the `projs` vector would contain
///   `field[0]` (aka `.0`), indicating that the type of `s` is
///   determined by finding the type of the `.0` field from `T`.
#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable, PartialEq)]
pub struct UserTypeProjection {
    pub base: UserTypeAnnotationIndex,
    pub projs: Vec<ProjectionKind>,
}

impl Copy for ProjectionKind {}

impl UserTypeProjection {
    pub(crate) fn index(mut self) -> Self {
        self.projs.push(ProjectionElem::Index(()));
        self
    }

    pub(crate) fn subslice(mut self, from: u32, to: u32) -> Self {
        self.projs.push(ProjectionElem::Subslice { from, to });
        self
    }

    pub(crate) fn deref(mut self) -> Self {
        self.projs.push(ProjectionElem::Deref);
        self
    }

    pub(crate) fn leaf(mut self, field: Field) -> Self {
        self.projs.push(ProjectionElem::Field(field, ()));
        self
    }

    pub(crate) fn variant(
        mut self,
        adt_def: &'tcx AdtDef,
        variant_index: VariantIdx,
        field: Field,
    ) -> Self {
        self.projs.push(ProjectionElem::Downcast(
            Some(adt_def.variants[variant_index].ident.name),
            variant_index,
        ));
        self.projs.push(ProjectionElem::Field(field, ()));
        self
    }
}

CloneTypeFoldableAndLiftImpls! { ProjectionKind, }

impl<'tcx> TypeFoldable<'tcx> for UserTypeProjection {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        use crate::mir::ProjectionElem::*;

        let base = self.base.fold_with(folder);
        let projs: Vec<_> = self
            .projs
            .iter()
            .map(|elem| match elem {
                Deref => Deref,
                Field(f, ()) => Field(f.clone(), ()),
                Index(()) => Index(()),
                elem => elem.clone(),
            })
            .collect();

        UserTypeProjection { base, projs }
    }

    fn super_visit_with<Vs: TypeVisitor<'tcx>>(&self, visitor: &mut Vs) -> bool {
        self.base.visit_with(visitor)
        // Note: there's nothing in `self.proj` to visit.
    }
}

rustc_index::newtype_index! {
    pub struct Promoted {
        derive [HashStable]
        DEBUG_FORMAT = "promoted[{}]"
    }
}

impl<'tcx> Debug for Constant<'tcx> {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}", self)
    }
}

impl<'tcx> Display for Constant<'tcx> {
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        write!(fmt, "const ")?;
        // FIXME make the default pretty printing of raw pointers more detailed. Here we output the
        // debug representation of raw pointers, so that the raw pointers in the mir dump output are
        // detailed and just not '{pointer}'.
        if let ty::RawPtr(_) = self.literal.ty.kind {
            write!(fmt, "{:?} : {}", self.literal.val, self.literal.ty)
        } else {
            write!(fmt, "{}", self.literal)
        }
    }
}

impl<'tcx> graph::DirectedGraph for Body<'tcx> {
    type Node = BasicBlock;
}

impl<'tcx> graph::WithNumNodes for Body<'tcx> {
    fn num_nodes(&self) -> usize {
        self.basic_blocks.len()
    }
}

impl<'tcx> graph::WithStartNode for Body<'tcx> {
    fn start_node(&self) -> Self::Node {
        START_BLOCK
    }
}

impl<'tcx> graph::WithPredecessors for Body<'tcx> {
    fn predecessors(
        &self,
        node: Self::Node,
    ) -> <Self as GraphPredecessors<'_>>::Iter {
        self.predecessors_for(node).clone().into_iter()
    }
}

impl<'tcx> graph::WithSuccessors for Body<'tcx> {
    fn successors(
        &self,
        node: Self::Node,
    ) -> <Self as GraphSuccessors<'_>>::Iter {
        self.basic_blocks[node].terminator().successors().cloned()
    }
}

impl<'a, 'b> graph::GraphPredecessors<'b> for Body<'a> {
    type Item = BasicBlock;
    type Iter = IntoIter<BasicBlock>;
}

impl<'a, 'b> graph::GraphSuccessors<'b> for Body<'a> {
    type Item = BasicBlock;
    type Iter = iter::Cloned<Successors<'b>>;
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, HashStable)]
pub struct Location {
    /// The block that the location is within.
    pub block: BasicBlock,

    /// The location is the position of the start of the statement; or, if
    /// `statement_index` equals the number of statements, then the start of the
    /// terminator.
    pub statement_index: usize,
}

impl fmt::Debug for Location {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{:?}[{}]", self.block, self.statement_index)
    }
}

impl Location {
    pub const START: Location = Location { block: START_BLOCK, statement_index: 0 };

    /// Returns the location immediately after this one within the enclosing block.
    ///
    /// Note that if this location represents a terminator, then the
    /// resulting location would be out of bounds and invalid.
    pub fn successor_within_block(&self) -> Location {
        Location { block: self.block, statement_index: self.statement_index + 1 }
    }

    /// Returns `true` if `other` is earlier in the control flow graph than `self`.
    pub fn is_predecessor_of<'tcx>(&self, other: Location, body: &Body<'tcx>) -> bool {
        // If we are in the same block as the other location and are an earlier statement
        // then we are a predecessor of `other`.
        if self.block == other.block && self.statement_index < other.statement_index {
            return true;
        }

        // If we're in another block, then we want to check that block is a predecessor of `other`.
        let mut queue: Vec<BasicBlock> = body.predecessors_for(other.block).clone();
        let mut visited = FxHashSet::default();

        while let Some(block) = queue.pop() {
            // If we haven't visited this block before, then make sure we visit it's predecessors.
            if visited.insert(block) {
                queue.append(&mut body.predecessors_for(block).clone());
            } else {
                continue;
            }

            // If we found the block that `self` is in, then we are a predecessor of `other` (since
            // we found that block by looking at the predecessors of `other`).
            if self.block == block {
                return true;
            }
        }

        false
    }

    pub fn dominates(&self, other: Location, dominators: &Dominators<BasicBlock>) -> bool {
        if self.block == other.block {
            self.statement_index <= other.statement_index
        } else {
            dominators.is_dominated_by(other.block, self.block)
        }
    }
}

#[derive(Copy, Clone, PartialEq, RustcEncodable, RustcDecodable, HashStable)]
pub enum UnsafetyViolationKind {
    General,
    /// Permitted both in `const fn`s and regular `fn`s.
    GeneralAndConstFn,
    BorrowPacked(hir::HirId),
}

#[derive(Copy, Clone, PartialEq, RustcEncodable, RustcDecodable, HashStable)]
pub struct UnsafetyViolation {
    pub source_info: SourceInfo,
    pub description: Symbol,
    pub details: Symbol,
    pub kind: UnsafetyViolationKind,
}

#[derive(Clone, RustcEncodable, RustcDecodable, HashStable)]
pub struct UnsafetyCheckResult {
    /// Violations that are propagated *upwards* from this function.
    pub violations: Lrc<[UnsafetyViolation]>,
    /// `unsafe` blocks in this function, along with whether they are used. This is
    /// used for the "unused_unsafe" lint.
    pub unsafe_blocks: Lrc<[(hir::HirId, bool)]>,
}

rustc_index::newtype_index! {
    pub struct GeneratorSavedLocal {
        derive [HashStable]
        DEBUG_FORMAT = "_{}",
    }
}

/// The layout of generator state.
#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable, TypeFoldable)]
pub struct GeneratorLayout<'tcx> {
    /// The type of every local stored inside the generator.
    pub field_tys: IndexVec<GeneratorSavedLocal, Ty<'tcx>>,

    /// Which of the above fields are in each variant. Note that one field may
    /// be stored in multiple variants.
    pub variant_fields: IndexVec<VariantIdx, IndexVec<Field, GeneratorSavedLocal>>,

    /// Which saved locals are storage-live at the same time. Locals that do not
    /// have conflicts with each other are allowed to overlap in the computed
    /// layout.
    pub storage_conflicts: BitMatrix<GeneratorSavedLocal, GeneratorSavedLocal>,

    /// The names and scopes of all the stored generator locals.
    ///
    /// N.B., this is *strictly* a temporary hack for codegen
    /// debuginfo generation, and will be removed at some point.
    /// Do **NOT** use it for anything else, local information should not be
    /// in the MIR, please rely on local crate HIR or other side-channels.
    //
    // FIXME(tmandry): see above.
    pub __local_debuginfo_codegen_only_do_not_use: IndexVec<GeneratorSavedLocal, LocalDecl<'tcx>>,
}

#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub struct BorrowCheckResult<'tcx> {
    pub closure_requirements: Option<ClosureRegionRequirements<'tcx>>,
    pub used_mut_upvars: SmallVec<[Field; 8]>,
}

/// The result of the `mir_const_qualif` query.
///
/// Each field corresponds to an implementer of the `Qualif` trait in
/// `librustc_mir/transform/check_consts/qualifs.rs`. See that file for more information on each
/// `Qualif`.
#[derive(Clone, Copy, Debug, Default, RustcEncodable, RustcDecodable, HashStable)]
pub struct ConstQualifs {
    pub has_mut_interior: bool,
    pub needs_drop: bool,
}

/// After we borrow check a closure, we are left with various
/// requirements that we have inferred between the free regions that
/// appear in the closure's signature or on its field types. These
/// requirements are then verified and proved by the closure's
/// creating function. This struct encodes those requirements.
///
/// The requirements are listed as being between various
/// `RegionVid`. The 0th region refers to `'static`; subsequent region
/// vids refer to the free regions that appear in the closure (or
/// generator's) type, in order of appearance. (This numbering is
/// actually defined by the `UniversalRegions` struct in the NLL
/// region checker. See for example
/// `UniversalRegions::closure_mapping`.) Note that we treat the free
/// regions in the closure's type "as if" they were erased, so their
/// precise identity is not important, only their position.
///
/// Example: If type check produces a closure with the closure substs:
///
/// ```text
/// ClosureSubsts = [
///     i8,                                  // the "closure kind"
///     for<'x> fn(&'a &'x u32) -> &'x u32,  // the "closure signature"
///     &'a String,                          // some upvar
/// ]
/// ```
///
/// here, there is one unique free region (`'a`) but it appears
/// twice. We would "renumber" each occurrence to a unique vid, as follows:
///
/// ```text
/// ClosureSubsts = [
///     i8,                                  // the "closure kind"
///     for<'x> fn(&'1 &'x u32) -> &'x u32,  // the "closure signature"
///     &'2 String,                          // some upvar
/// ]
/// ```
///
/// Now the code might impose a requirement like `'1: '2`. When an
/// instance of the closure is created, the corresponding free regions
/// can be extracted from its type and constrained to have the given
/// outlives relationship.
///
/// In some cases, we have to record outlives requirements between
/// types and regions as well. In that case, if those types include
/// any regions, those regions are recorded as `ReClosureBound`
/// instances assigned one of these same indices. Those regions will
/// be substituted away by the creator. We use `ReClosureBound` in
/// that case because the regions must be allocated in the global
/// `TyCtxt`, and hence we cannot use `ReVar` (which is what we use
/// internally within the rest of the NLL code).
#[derive(Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub struct ClosureRegionRequirements<'tcx> {
    /// The number of external regions defined on the closure. In our
    /// example above, it would be 3 -- one for `'static`, then `'1`
    /// and `'2`. This is just used for a sanity check later on, to
    /// make sure that the number of regions we see at the callsite
    /// matches.
    pub num_external_vids: usize,

    /// Requirements between the various free regions defined in
    /// indices.
    pub outlives_requirements: Vec<ClosureOutlivesRequirement<'tcx>>,
}

/// Indicates an outlives-constraint between a type or between two
/// free regions declared on the closure.
#[derive(Copy, Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub struct ClosureOutlivesRequirement<'tcx> {
    // This region or type ...
    pub subject: ClosureOutlivesSubject<'tcx>,

    // ... must outlive this one.
    pub outlived_free_region: ty::RegionVid,

    // If not, report an error here ...
    pub blame_span: Span,

    // ... due to this reason.
    pub category: ConstraintCategory,
}

/// Outlives-constraints can be categorized to determine whether and why they
/// are interesting (for error reporting). Order of variants indicates sort
/// order of the category, thereby influencing diagnostic output.
///
/// See also [rustc_mir::borrow_check::nll::constraints].
#[derive(
    Copy,
    Clone,
    Debug,
    Eq,
    PartialEq,
    PartialOrd,
    Ord,
    Hash,
    RustcEncodable,
    RustcDecodable,
    HashStable,
)]
pub enum ConstraintCategory {
    Return,
    Yield,
    UseAsConst,
    UseAsStatic,
    TypeAnnotation,
    Cast,

    /// A constraint that came from checking the body of a closure.
    ///
    /// We try to get the category that the closure used when reporting this.
    ClosureBounds,
    CallArgument,
    CopyBound,
    SizedBound,
    Assignment,
    OpaqueType,

    /// A "boring" constraint (caused by the given location) is one that
    /// the user probably doesn't want to see described in diagnostics,
    /// because it is kind of an artifact of the type system setup.
    /// Example: `x = Foo { field: y }` technically creates
    /// intermediate regions representing the "type of `Foo { field: y
    /// }`", and data flows from `y` into those variables, but they
    /// are not very interesting. The assignment into `x` on the other
    /// hand might be.
    Boring,
    // Boring and applicable everywhere.
    BoringNoLocation,

    /// A constraint that doesn't correspond to anything the user sees.
    Internal,
}

/// The subject of a `ClosureOutlivesRequirement` -- that is, the thing
/// that must outlive some region.
#[derive(Copy, Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub enum ClosureOutlivesSubject<'tcx> {
    /// Subject is a type, typically a type parameter, but could also
    /// be a projection. Indicates a requirement like `T: 'a` being
    /// passed to the caller, where the type here is `T`.
    ///
    /// The type here is guaranteed not to contain any free regions at
    /// present.
    Ty(Ty<'tcx>),

    /// Subject is a free region from the closure. Indicates a requirement
    /// like `'a: 'b` being passed to the caller; the region here is `'a`.
    Region(ty::RegionVid),
}

/*
 * `TypeFoldable` implementations for MIR types
*/

CloneTypeFoldableAndLiftImpls! {
    BlockTailInfo,
    MirPhase,
    Mutability,
    SourceInfo,
    UpvarDebuginfo,
    FakeReadCause,
    RetagKind,
    SourceScope,
    SourceScopeData,
    SourceScopeLocalData,
    UserTypeAnnotationIndex,
}

impl<'tcx> TypeFoldable<'tcx> for Terminator<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        use crate::mir::TerminatorKind::*;

        let kind = match self.kind {
            Goto { target } => Goto { target },
            SwitchInt { ref discr, switch_ty, ref values, ref targets } => SwitchInt {
                discr: discr.fold_with(folder),
                switch_ty: switch_ty.fold_with(folder),
                values: values.clone(),
                targets: targets.clone(),
            },
            Drop { ref location, target, unwind } => {
                Drop { location: location.fold_with(folder), target, unwind }
            }
            DropAndReplace { ref location, ref value, target, unwind } => DropAndReplace {
                location: location.fold_with(folder),
                value: value.fold_with(folder),
                target,
                unwind,
            },
            Yield { ref value, resume, drop } => {
                Yield { value: value.fold_with(folder), resume: resume, drop: drop }
            }
            Call { ref func, ref args, ref destination, cleanup, from_hir_call } => {
                let dest =
                    destination.as_ref().map(|&(ref loc, dest)| (loc.fold_with(folder), dest));

                Call {
                    func: func.fold_with(folder),
                    args: args.fold_with(folder),
                    destination: dest,
                    cleanup,
                    from_hir_call,
                }
            }
            Assert { ref cond, expected, ref msg, target, cleanup } => {
                use PanicInfo::*;
                let msg = match msg {
                    BoundsCheck { ref len, ref index } =>
                        BoundsCheck {
                            len: len.fold_with(folder),
                            index: index.fold_with(folder),
                        },
                    Panic { .. } | Overflow(_) | OverflowNeg | DivisionByZero | RemainderByZero |
                    GeneratorResumedAfterReturn | GeneratorResumedAfterPanic =>
                        msg.clone(),
                };
                Assert { cond: cond.fold_with(folder), expected, msg, target, cleanup }
            }
            GeneratorDrop => GeneratorDrop,
            Resume => Resume,
            Abort => Abort,
            Return => Return,
            Unreachable => Unreachable,
            FalseEdges { real_target, imaginary_target } => {
                FalseEdges { real_target, imaginary_target }
            }
            FalseUnwind { real_target, unwind } => FalseUnwind { real_target, unwind },
        };
        Terminator { source_info: self.source_info, kind }
    }

    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, visitor: &mut V) -> bool {
        use crate::mir::TerminatorKind::*;

        match self.kind {
            SwitchInt { ref discr, switch_ty, .. } => {
                discr.visit_with(visitor) || switch_ty.visit_with(visitor)
            }
            Drop { ref location, .. } => location.visit_with(visitor),
            DropAndReplace { ref location, ref value, .. } => {
                location.visit_with(visitor) || value.visit_with(visitor)
            }
            Yield { ref value, .. } => value.visit_with(visitor),
            Call { ref func, ref args, ref destination, .. } => {
                let dest = if let Some((ref loc, _)) = *destination {
                    loc.visit_with(visitor)
                } else {
                    false
                };
                dest || func.visit_with(visitor) || args.visit_with(visitor)
            }
            Assert { ref cond, ref msg, .. } => {
                if cond.visit_with(visitor) {
                    use PanicInfo::*;
                    match msg {
                        BoundsCheck { ref len, ref index } =>
                            len.visit_with(visitor) || index.visit_with(visitor),
                        Panic { .. } | Overflow(_) | OverflowNeg |
                        DivisionByZero | RemainderByZero |
                        GeneratorResumedAfterReturn | GeneratorResumedAfterPanic =>
                            false
                    }
                } else {
                    false
                }
            }
            Goto { .. }
            | Resume
            | Abort
            | Return
            | GeneratorDrop
            | Unreachable
            | FalseEdges { .. }
            | FalseUnwind { .. } => false,
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for Place<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        Place {
            base: self.base.fold_with(folder),
            projection: self.projection.fold_with(folder),
        }
    }

    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, visitor: &mut V) -> bool {
        self.base.visit_with(visitor) || self.projection.visit_with(visitor)
    }
}

impl<'tcx> TypeFoldable<'tcx> for PlaceBase<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        match self {
            PlaceBase::Local(local) => PlaceBase::Local(local.fold_with(folder)),
            PlaceBase::Static(static_) => PlaceBase::Static(static_.fold_with(folder)),
        }
    }

    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, visitor: &mut V) -> bool {
        match self {
            PlaceBase::Local(local) => local.visit_with(visitor),
            PlaceBase::Static(static_) => (**static_).visit_with(visitor),
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for &'tcx ty::List<PlaceElem<'tcx>> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        let v = self.iter().map(|t| t.fold_with(folder)).collect::<Vec<_>>();
        folder.tcx().intern_place_elems(&v)
    }

    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, visitor: &mut V) -> bool {
        self.iter().any(|t| t.visit_with(visitor))
    }
}

impl<'tcx> TypeFoldable<'tcx> for Static<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        Static {
            ty: self.ty.fold_with(folder),
            kind: self.kind.fold_with(folder),
            def_id: self.def_id,
        }
    }

    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, visitor: &mut V) -> bool {
        let Static { ty, kind, def_id: _ } = self;

        ty.visit_with(visitor) || kind.visit_with(visitor)
    }
}

impl<'tcx> TypeFoldable<'tcx> for StaticKind<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        match self {
            StaticKind::Promoted(promoted, substs) =>
                StaticKind::Promoted(promoted.fold_with(folder), substs.fold_with(folder)),
            StaticKind::Static => StaticKind::Static
        }
    }

    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, visitor: &mut V) -> bool {
        match self {
            StaticKind::Promoted(promoted, substs) =>
                promoted.visit_with(visitor) || substs.visit_with(visitor),
            StaticKind::Static => { false }
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for Rvalue<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        use crate::mir::Rvalue::*;
        match *self {
            Use(ref op) => Use(op.fold_with(folder)),
            Repeat(ref op, len) => Repeat(op.fold_with(folder), len),
            Ref(region, bk, ref place) => {
                Ref(region.fold_with(folder), bk, place.fold_with(folder))
            }
            Len(ref place) => Len(place.fold_with(folder)),
            Cast(kind, ref op, ty) => Cast(kind, op.fold_with(folder), ty.fold_with(folder)),
            BinaryOp(op, ref rhs, ref lhs) => {
                BinaryOp(op, rhs.fold_with(folder), lhs.fold_with(folder))
            }
            CheckedBinaryOp(op, ref rhs, ref lhs) => {
                CheckedBinaryOp(op, rhs.fold_with(folder), lhs.fold_with(folder))
            }
            UnaryOp(op, ref val) => UnaryOp(op, val.fold_with(folder)),
            Discriminant(ref place) => Discriminant(place.fold_with(folder)),
            NullaryOp(op, ty) => NullaryOp(op, ty.fold_with(folder)),
            Aggregate(ref kind, ref fields) => {
                let kind = box match **kind {
                    AggregateKind::Array(ty) => AggregateKind::Array(ty.fold_with(folder)),
                    AggregateKind::Tuple => AggregateKind::Tuple,
                    AggregateKind::Adt(def, v, substs, user_ty, n) => AggregateKind::Adt(
                        def,
                        v,
                        substs.fold_with(folder),
                        user_ty.fold_with(folder),
                        n,
                    ),
                    AggregateKind::Closure(id, substs) => {
                        AggregateKind::Closure(id, substs.fold_with(folder))
                    }
                    AggregateKind::Generator(id, substs, movablity) => {
                        AggregateKind::Generator(id, substs.fold_with(folder), movablity)
                    }
                };
                Aggregate(kind, fields.fold_with(folder))
            }
        }
    }

    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, visitor: &mut V) -> bool {
        use crate::mir::Rvalue::*;
        match *self {
            Use(ref op) => op.visit_with(visitor),
            Repeat(ref op, _) => op.visit_with(visitor),
            Ref(region, _, ref place) => region.visit_with(visitor) || place.visit_with(visitor),
            Len(ref place) => place.visit_with(visitor),
            Cast(_, ref op, ty) => op.visit_with(visitor) || ty.visit_with(visitor),
            BinaryOp(_, ref rhs, ref lhs) | CheckedBinaryOp(_, ref rhs, ref lhs) => {
                rhs.visit_with(visitor) || lhs.visit_with(visitor)
            }
            UnaryOp(_, ref val) => val.visit_with(visitor),
            Discriminant(ref place) => place.visit_with(visitor),
            NullaryOp(_, ty) => ty.visit_with(visitor),
            Aggregate(ref kind, ref fields) => {
                (match **kind {
                    AggregateKind::Array(ty) => ty.visit_with(visitor),
                    AggregateKind::Tuple => false,
                    AggregateKind::Adt(_, _, substs, user_ty, _) => {
                        substs.visit_with(visitor) || user_ty.visit_with(visitor)
                    }
                    AggregateKind::Closure(_, substs) => substs.visit_with(visitor),
                    AggregateKind::Generator(_, substs, _) => substs.visit_with(visitor),
                }) || fields.visit_with(visitor)
            }
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for Operand<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        match *self {
            Operand::Copy(ref place) => Operand::Copy(place.fold_with(folder)),
            Operand::Move(ref place) => Operand::Move(place.fold_with(folder)),
            Operand::Constant(ref c) => Operand::Constant(c.fold_with(folder)),
        }
    }

    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, visitor: &mut V) -> bool {
        match *self {
            Operand::Copy(ref place) | Operand::Move(ref place) => place.visit_with(visitor),
            Operand::Constant(ref c) => c.visit_with(visitor),
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for PlaceElem<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        use crate::mir::ProjectionElem::*;

        match self {
            Deref => Deref,
            Field(f, ty) => Field(*f, ty.fold_with(folder)),
            Index(v) => Index(v.fold_with(folder)),
            elem => elem.clone(),
        }
    }

    fn super_visit_with<Vs: TypeVisitor<'tcx>>(&self, visitor: &mut Vs) -> bool {
        use crate::mir::ProjectionElem::*;

        match self {
            Field(_, ty) => ty.visit_with(visitor),
            Index(v) => v.visit_with(visitor),
            _ => false,
        }
    }
}

impl<'tcx> TypeFoldable<'tcx> for Field {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, _: &mut F) -> Self {
        *self
    }
    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, _: &mut V) -> bool {
        false
    }
}

impl<'tcx> TypeFoldable<'tcx> for GeneratorSavedLocal {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, _: &mut F) -> Self {
        *self
    }
    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, _: &mut V) -> bool {
        false
    }
}

impl<'tcx, R: Idx, C: Idx> TypeFoldable<'tcx> for BitMatrix<R, C> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, _: &mut F) -> Self {
        self.clone()
    }
    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, _: &mut V) -> bool {
        false
    }
}

impl<'tcx> TypeFoldable<'tcx> for Constant<'tcx> {
    fn super_fold_with<F: TypeFolder<'tcx>>(&self, folder: &mut F) -> Self {
        Constant {
            span: self.span.clone(),
            user_ty: self.user_ty.fold_with(folder),
            literal: self.literal.fold_with(folder),
        }
    }
    fn super_visit_with<V: TypeVisitor<'tcx>>(&self, visitor: &mut V) -> bool {
        self.literal.visit_with(visitor)
    }
}
