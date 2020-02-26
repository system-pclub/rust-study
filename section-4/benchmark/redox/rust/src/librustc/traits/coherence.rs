//! See Rustc Guide chapters on [trait-resolution] and [trait-specialization] for more info on how
//! this works.
//!
//! [trait-resolution]: https://rust-lang.github.io/rustc-guide/traits/resolution.html
//! [trait-specialization]: https://rust-lang.github.io/rustc-guide/traits/specialization.html

use crate::infer::{CombinedSnapshot, InferOk};
use crate::hir::def_id::{DefId, LOCAL_CRATE};
use crate::traits::{self, Normalized, SelectionContext, Obligation, ObligationCause};
use crate::traits::IntercrateMode;
use crate::traits::select::IntercrateAmbiguityCause;
use crate::ty::{self, Ty, TyCtxt};
use crate::ty::fold::TypeFoldable;
use crate::ty::subst::Subst;
use syntax::symbol::sym;
use syntax_pos::DUMMY_SP;

/// Whether we do the orphan check relative to this crate or
/// to some remote crate.
#[derive(Copy, Clone, Debug)]
enum InCrate {
    Local,
    Remote
}

#[derive(Debug, Copy, Clone)]
pub enum Conflict {
    Upstream,
    Downstream { used_to_be_broken: bool }
}

pub struct OverlapResult<'tcx> {
    pub impl_header: ty::ImplHeader<'tcx>,
    pub intercrate_ambiguity_causes: Vec<IntercrateAmbiguityCause>,

    /// `true` if the overlap might've been permitted before the shift
    /// to universes.
    pub involves_placeholder: bool,
}

pub fn add_placeholder_note(err: &mut errors::DiagnosticBuilder<'_>) {
    err.note(&format!(
        "this behavior recently changed as a result of a bug fix; \
         see rust-lang/rust#56105 for details"
    ));
}

/// If there are types that satisfy both impls, invokes `on_overlap`
/// with a suitably-freshened `ImplHeader` with those types
/// substituted. Otherwise, invokes `no_overlap`.
pub fn overlapping_impls<F1, F2, R>(
    tcx: TyCtxt<'_>,
    impl1_def_id: DefId,
    impl2_def_id: DefId,
    intercrate_mode: IntercrateMode,
    on_overlap: F1,
    no_overlap: F2,
) -> R
where
    F1: FnOnce(OverlapResult<'_>) -> R,
    F2: FnOnce() -> R,
{
    debug!("overlapping_impls(\
           impl1_def_id={:?}, \
           impl2_def_id={:?},
           intercrate_mode={:?})",
           impl1_def_id,
           impl2_def_id,
           intercrate_mode);

    let overlaps = tcx.infer_ctxt().enter(|infcx| {
        let selcx = &mut SelectionContext::intercrate(&infcx, intercrate_mode);
        overlap(selcx, impl1_def_id, impl2_def_id).is_some()
    });

    if !overlaps {
        return no_overlap();
    }

    // In the case where we detect an error, run the check again, but
    // this time tracking intercrate ambuiguity causes for better
    // diagnostics. (These take time and can lead to false errors.)
    tcx.infer_ctxt().enter(|infcx| {
        let selcx = &mut SelectionContext::intercrate(&infcx, intercrate_mode);
        selcx.enable_tracking_intercrate_ambiguity_causes();
        on_overlap(overlap(selcx, impl1_def_id, impl2_def_id).unwrap())
    })
}

fn with_fresh_ty_vars<'cx, 'tcx>(
    selcx: &mut SelectionContext<'cx, 'tcx>,
    param_env: ty::ParamEnv<'tcx>,
    impl_def_id: DefId,
) -> ty::ImplHeader<'tcx> {
    let tcx = selcx.tcx();
    let impl_substs = selcx.infcx().fresh_substs_for_item(DUMMY_SP, impl_def_id);

    let header = ty::ImplHeader {
        impl_def_id,
        self_ty: tcx.type_of(impl_def_id).subst(tcx, impl_substs),
        trait_ref: tcx.impl_trait_ref(impl_def_id).subst(tcx, impl_substs),
        predicates: tcx.predicates_of(impl_def_id).instantiate(tcx, impl_substs).predicates,
    };

    let Normalized { value: mut header, obligations } =
        traits::normalize(selcx, param_env, ObligationCause::dummy(), &header);

    header.predicates.extend(obligations.into_iter().map(|o| o.predicate));
    header
}

/// Can both impl `a` and impl `b` be satisfied by a common type (including
/// where-clauses)? If so, returns an `ImplHeader` that unifies the two impls.
fn overlap<'cx, 'tcx>(
    selcx: &mut SelectionContext<'cx, 'tcx>,
    a_def_id: DefId,
    b_def_id: DefId,
) -> Option<OverlapResult<'tcx>> {
    debug!("overlap(a_def_id={:?}, b_def_id={:?})", a_def_id, b_def_id);

    selcx.infcx().probe(|snapshot| overlap_within_probe(selcx, a_def_id, b_def_id, snapshot))
}

fn overlap_within_probe(
    selcx: &mut SelectionContext<'cx, 'tcx>,
    a_def_id: DefId,
    b_def_id: DefId,
    snapshot: &CombinedSnapshot<'_, 'tcx>,
) -> Option<OverlapResult<'tcx>> {
    // For the purposes of this check, we don't bring any placeholder
    // types into scope; instead, we replace the generic types with
    // fresh type variables, and hence we do our evaluations in an
    // empty environment.
    let param_env = ty::ParamEnv::empty();

    let a_impl_header = with_fresh_ty_vars(selcx, param_env, a_def_id);
    let b_impl_header = with_fresh_ty_vars(selcx, param_env, b_def_id);

    debug!("overlap: a_impl_header={:?}", a_impl_header);
    debug!("overlap: b_impl_header={:?}", b_impl_header);

    // Do `a` and `b` unify? If not, no overlap.
    let obligations = match selcx.infcx().at(&ObligationCause::dummy(), param_env)
                                         .eq_impl_headers(&a_impl_header, &b_impl_header)
    {
        Ok(InferOk { obligations, value: () }) => obligations,
        Err(_) => return None
    };

    debug!("overlap: unification check succeeded");

    // Are any of the obligations unsatisfiable? If so, no overlap.
    let infcx = selcx.infcx();
    let opt_failing_obligation =
        a_impl_header.predicates
                     .iter()
                     .chain(&b_impl_header.predicates)
                     .map(|p| infcx.resolve_vars_if_possible(p))
                     .map(|p| Obligation { cause: ObligationCause::dummy(),
                                           param_env,
                                           recursion_depth: 0,
                                           predicate: p })
                     .chain(obligations)
                     .find(|o| !selcx.predicate_may_hold_fatal(o));
    // FIXME: the call to `selcx.predicate_may_hold_fatal` above should be ported
    // to the canonical trait query form, `infcx.predicate_may_hold`, once
    // the new system supports intercrate mode (which coherence needs).

    if let Some(failing_obligation) = opt_failing_obligation {
        debug!("overlap: obligation unsatisfiable {:?}", failing_obligation);
        return None
    }

    let impl_header = selcx.infcx().resolve_vars_if_possible(&a_impl_header);
    let intercrate_ambiguity_causes = selcx.take_intercrate_ambiguity_causes();
    debug!("overlap: intercrate_ambiguity_causes={:#?}", intercrate_ambiguity_causes);

    let involves_placeholder = match selcx.infcx().region_constraints_added_in_snapshot(snapshot) {
        Some(true) => true,
        _ => false,
    };

    Some(OverlapResult { impl_header, intercrate_ambiguity_causes, involves_placeholder })
}

pub fn trait_ref_is_knowable<'tcx>(
    tcx: TyCtxt<'tcx>,
    trait_ref: ty::TraitRef<'tcx>,
) -> Option<Conflict> {
    debug!("trait_ref_is_knowable(trait_ref={:?})", trait_ref);
    if orphan_check_trait_ref(tcx, trait_ref, InCrate::Remote).is_ok() {
        // A downstream or cousin crate is allowed to implement some
        // substitution of this trait-ref.

        // A trait can be implementable for a trait ref by both the current
        // crate and crates downstream of it. Older versions of rustc
        // were not aware of this, causing incoherence (issue #43355).
        let used_to_be_broken =
            orphan_check_trait_ref(tcx, trait_ref, InCrate::Local).is_ok();
        if used_to_be_broken {
            debug!("trait_ref_is_knowable({:?}) - USED TO BE BROKEN", trait_ref);
        }
        return Some(Conflict::Downstream { used_to_be_broken });
    }

    if trait_ref_is_local_or_fundamental(tcx, trait_ref) {
        // This is a local or fundamental trait, so future-compatibility
        // is no concern. We know that downstream/cousin crates are not
        // allowed to implement a substitution of this trait ref, which
        // means impls could only come from dependencies of this crate,
        // which we already know about.
        return None;
    }

    // This is a remote non-fundamental trait, so if another crate
    // can be the "final owner" of a substitution of this trait-ref,
    // they are allowed to implement it future-compatibly.
    //
    // However, if we are a final owner, then nobody else can be,
    // and if we are an intermediate owner, then we don't care
    // about future-compatibility, which means that we're OK if
    // we are an owner.
    if orphan_check_trait_ref(tcx, trait_ref, InCrate::Local).is_ok() {
        debug!("trait_ref_is_knowable: orphan check passed");
        return None;
    } else {
        debug!("trait_ref_is_knowable: nonlocal, nonfundamental, unowned");
        return Some(Conflict::Upstream);
    }
}

pub fn trait_ref_is_local_or_fundamental<'tcx>(
    tcx: TyCtxt<'tcx>,
    trait_ref: ty::TraitRef<'tcx>,
) -> bool {
    trait_ref.def_id.krate == LOCAL_CRATE || tcx.has_attr(trait_ref.def_id, sym::fundamental)
}

pub enum OrphanCheckErr<'tcx> {
    NonLocalInputType(Vec<(Ty<'tcx>, bool /* Is this the first input type? */)>),
    UncoveredTy(Ty<'tcx>, Option<Ty<'tcx>>),
}

/// Checks the coherence orphan rules. `impl_def_id` should be the
/// `DefId` of a trait impl. To pass, either the trait must be local, or else
/// two conditions must be satisfied:
///
/// 1. All type parameters in `Self` must be "covered" by some local type constructor.
/// 2. Some local type must appear in `Self`.
pub fn orphan_check(
    tcx: TyCtxt<'_>,
    impl_def_id: DefId,
) -> Result<(), OrphanCheckErr<'_>> {
    debug!("orphan_check({:?})", impl_def_id);

    // We only except this routine to be invoked on implementations
    // of a trait, not inherent implementations.
    let trait_ref = tcx.impl_trait_ref(impl_def_id).unwrap();
    debug!("orphan_check: trait_ref={:?}", trait_ref);

    // If the *trait* is local to the crate, ok.
    if trait_ref.def_id.is_local() {
        debug!("trait {:?} is local to current crate",
               trait_ref.def_id);
        return Ok(());
    }

    orphan_check_trait_ref(tcx, trait_ref, InCrate::Local)
}

/// Checks whether a trait-ref is potentially implementable by a crate.
///
/// The current rule is that a trait-ref orphan checks in a crate C:
///
/// 1. Order the parameters in the trait-ref in subst order - Self first,
///    others linearly (e.g., `<U as Foo<V, W>>` is U < V < W).
/// 2. Of these type parameters, there is at least one type parameter
///    in which, walking the type as a tree, you can reach a type local
///    to C where all types in-between are fundamental types. Call the
///    first such parameter the "local key parameter".
///     - e.g., `Box<LocalType>` is OK, because you can visit LocalType
///       going through `Box`, which is fundamental.
///     - similarly, `FundamentalPair<Vec<()>, Box<LocalType>>` is OK for
///       the same reason.
///     - but (knowing that `Vec<T>` is non-fundamental, and assuming it's
///       not local), `Vec<LocalType>` is bad, because `Vec<->` is between
///       the local type and the type parameter.
/// 3. Every type parameter before the local key parameter is fully known in C.
///     - e.g., `impl<T> T: Trait<LocalType>` is bad, because `T` might be
///       an unknown type.
///     - but `impl<T> LocalType: Trait<T>` is OK, because `LocalType`
///       occurs before `T`.
/// 4. Every type in the local key parameter not known in C, going
///    through the parameter's type tree, must appear only as a subtree of
///    a type local to C, with only fundamental types between the type
///    local to C and the local key parameter.
///     - e.g., `Vec<LocalType<T>>>` (or equivalently `Box<Vec<LocalType<T>>>`)
///     is bad, because the only local type with `T` as a subtree is
///     `LocalType<T>`, and `Vec<->` is between it and the type parameter.
///     - similarly, `FundamentalPair<LocalType<T>, T>` is bad, because
///     the second occurrence of `T` is not a subtree of *any* local type.
///     - however, `LocalType<Vec<T>>` is OK, because `T` is a subtree of
///     `LocalType<Vec<T>>`, which is local and has no types between it and
///     the type parameter.
///
/// The orphan rules actually serve several different purposes:
///
/// 1. They enable link-safety - i.e., 2 mutually-unknowing crates (where
///    every type local to one crate is unknown in the other) can't implement
///    the same trait-ref. This follows because it can be seen that no such
///    type can orphan-check in 2 such crates.
///
///    To check that a local impl follows the orphan rules, we check it in
///    InCrate::Local mode, using type parameters for the "generic" types.
///
/// 2. They ground negative reasoning for coherence. If a user wants to
///    write both a conditional blanket impl and a specific impl, we need to
///    make sure they do not overlap. For example, if we write
///    ```
///    impl<T> IntoIterator for Vec<T>
///    impl<T: Iterator> IntoIterator for T
///    ```
///    We need to be able to prove that `Vec<$0>: !Iterator` for every type $0.
///    We can observe that this holds in the current crate, but we need to make
///    sure this will also hold in all unknown crates (both "independent" crates,
///    which we need for link-safety, and also child crates, because we don't want
///    child crates to get error for impl conflicts in a *dependency*).
///
///    For that, we only allow negative reasoning if, for every assignment to the
///    inference variables, every unknown crate would get an orphan error if they
///    try to implement this trait-ref. To check for this, we use InCrate::Remote
///    mode. That is sound because we already know all the impls from known crates.
///
/// 3. For non-#[fundamental] traits, they guarantee that parent crates can
///    add "non-blanket" impls without breaking negative reasoning in dependent
///    crates. This is the "rebalancing coherence" (RFC 1023) restriction.
///
///    For that, we only a allow crate to perform negative reasoning on
///    non-local-non-#[fundamental] only if there's a local key parameter as per (2).
///
///    Because we never perform negative reasoning generically (coherence does
///    not involve type parameters), this can be interpreted as doing the full
///    orphan check (using InCrate::Local mode), substituting non-local known
///    types for all inference variables.
///
///    This allows for crates to future-compatibly add impls as long as they
///    can't apply to types with a key parameter in a child crate - applying
///    the rules, this basically means that every type parameter in the impl
///    must appear behind a non-fundamental type (because this is not a
///    type-system requirement, crate owners might also go for "semantic
///    future-compatibility" involving things such as sealed traits, but
///    the above requirement is sufficient, and is necessary in "open world"
///    cases).
///
/// Note that this function is never called for types that have both type
/// parameters and inference variables.
fn orphan_check_trait_ref<'tcx>(
    tcx: TyCtxt<'tcx>,
    trait_ref: ty::TraitRef<'tcx>,
    in_crate: InCrate,
) -> Result<(), OrphanCheckErr<'tcx>> {
    debug!("orphan_check_trait_ref(trait_ref={:?}, in_crate={:?})",
           trait_ref, in_crate);

    if trait_ref.needs_infer() && trait_ref.needs_subst() {
        bug!("can't orphan check a trait ref with both params and inference variables {:?}",
             trait_ref);
    }

    // Given impl<P1..=Pn> Trait<T1..=Tn> for T0, an impl is valid only
    // if at least one of the following is true:
    //
    // - Trait is a local trait
    // (already checked in orphan_check prior to calling this function)
    // - All of
    //     - At least one of the types T0..=Tn must be a local type.
    //      Let Ti be the first such type.
    //     - No uncovered type parameters P1..=Pn may appear in T0..Ti (excluding Ti)
    //
    fn uncover_fundamental_ty<'tcx>(
        tcx: TyCtxt<'tcx>,
        ty: Ty<'tcx>,
        in_crate: InCrate,
    ) -> Vec<Ty<'tcx>> {
        if fundamental_ty(ty) && ty_is_non_local(tcx, ty, in_crate).is_some() {
            ty.walk_shallow().flat_map(|ty| uncover_fundamental_ty(tcx, ty, in_crate)).collect()
        } else {
            vec![ty]
        }
    }

    let mut non_local_spans = vec![];
    for (i, input_ty) in trait_ref
        .input_types()
        .flat_map(|ty| uncover_fundamental_ty(tcx, ty, in_crate))
        .enumerate()
    {
        debug!("orphan_check_trait_ref: check ty `{:?}`", input_ty);
        let non_local_tys = ty_is_non_local(tcx, input_ty, in_crate);
        if non_local_tys.is_none() {
            debug!("orphan_check_trait_ref: ty_is_local `{:?}`", input_ty);
            return Ok(());
        } else if let ty::Param(_) = input_ty.kind {
            debug!("orphan_check_trait_ref: uncovered ty: `{:?}`", input_ty);
            let local_type = trait_ref
                .input_types()
                .flat_map(|ty| uncover_fundamental_ty(tcx, ty, in_crate))
                .filter(|ty| ty_is_non_local_constructor(tcx, ty, in_crate).is_none())
                .next();

            debug!("orphan_check_trait_ref: uncovered ty local_type: `{:?}`", local_type);

            return Err(OrphanCheckErr::UncoveredTy(input_ty, local_type))
        }
        if let Some(non_local_tys) = non_local_tys {
            for input_ty in non_local_tys {
                non_local_spans.push((input_ty, i == 0));
            }
        }
    }
    // If we exit above loop, never found a local type.
    debug!("orphan_check_trait_ref: no local type");
    Err(OrphanCheckErr::NonLocalInputType(non_local_spans))
}

fn ty_is_non_local<'t>(tcx: TyCtxt<'t>, ty: Ty<'t>, in_crate: InCrate) -> Option<Vec<Ty<'t>>> {
    match ty_is_non_local_constructor(tcx, ty, in_crate) {
        Some(ty) => if !fundamental_ty(ty) {
            Some(vec![ty])
        } else {
            let tys: Vec<_> = ty.walk_shallow()
                .filter_map(|t| ty_is_non_local(tcx, t, in_crate))
                .flat_map(|i| i)
                .collect();
            if tys.is_empty() {
                None
            } else {
                Some(tys)
            }
        },
        None => None,
    }
}

fn fundamental_ty(ty: Ty<'_>) -> bool {
    match ty.kind {
        ty::Ref(..) => true,
        ty::Adt(def, _) => def.is_fundamental(),
        _ => false
    }
}

fn def_id_is_local(def_id: DefId, in_crate: InCrate) -> bool {
    match in_crate {
        // The type is local to *this* crate - it will not be
        // local in any other crate.
        InCrate::Remote => false,
        InCrate::Local => def_id.is_local()
    }
}

fn ty_is_non_local_constructor<'tcx>(
    tcx: TyCtxt<'tcx>,
    ty: Ty<'tcx>,
    in_crate: InCrate,
) -> Option<Ty<'tcx>> {
    debug!("ty_is_non_local_constructor({:?})", ty);

    match ty.kind {
        ty::Bool |
        ty::Char |
        ty::Int(..) |
        ty::Uint(..) |
        ty::Float(..) |
        ty::Str |
        ty::FnDef(..) |
        ty::FnPtr(_) |
        ty::Array(..) |
        ty::Slice(..) |
        ty::RawPtr(..) |
        ty::Ref(..) |
        ty::Never |
        ty::Tuple(..) |
        ty::Param(..) |
        ty::Projection(..) => {
            Some(ty)
        }

        ty::Placeholder(..) | ty::Bound(..) | ty::Infer(..) => match in_crate {
            InCrate::Local => Some(ty),
            // The inference variable might be unified with a local
            // type in that remote crate.
            InCrate::Remote => None,
        },

        ty::Adt(def, _) => if def_id_is_local(def.did, in_crate) {
            None
        } else {
            Some(ty)
        },
        ty::Foreign(did) => if def_id_is_local(did, in_crate) {
            None
        } else {
            Some(ty)
        },
        ty::Opaque(did, _) => {
            // Check the underlying type that this opaque
            // type resolves to.
            // This recursion will eventually terminate,
            // since we've already managed to successfully
            // resolve all opaque types by this point
            let real_ty = tcx.type_of(did);
            ty_is_non_local_constructor(tcx, real_ty, in_crate)
        }

        ty::Dynamic(ref tt, ..) => {
            if let Some(principal) = tt.principal() {
                if def_id_is_local(principal.def_id(), in_crate) {
                    None
                } else {
                    Some(ty)
                }
            } else {
                Some(ty)
            }
        }

        ty::Error => None,

        ty::UnnormalizedProjection(..) |
        ty::Closure(..) |
        ty::Generator(..) |
        ty::GeneratorWitness(..) => {
            bug!("ty_is_local invoked on unexpected type: {:?}", ty)
        }
    }
}
