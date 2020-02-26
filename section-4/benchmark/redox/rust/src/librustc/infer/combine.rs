///////////////////////////////////////////////////////////////////////////
// # Type combining
//
// There are four type combiners: equate, sub, lub, and glb.  Each
// implements the trait `Combine` and contains methods for combining
// two instances of various things and yielding a new instance.  These
// combiner methods always yield a `Result<T>`.  There is a lot of
// common code for these operations, implemented as default methods on
// the `Combine` trait.
//
// Each operation may have side-effects on the inference context,
// though these can be unrolled using snapshots. On success, the
// LUB/GLB operations return the appropriate bound. The Eq and Sub
// operations generally return the first operand.
//
// ## Contravariance
//
// When you are relating two things which have a contravariant
// relationship, you should use `contratys()` or `contraregions()`,
// rather than inversing the order of arguments!  This is necessary
// because the order of arguments is not relevant for LUB and GLB.  It
// is also useful to track which value is the "expected" value in
// terms of error reporting.

use super::equate::Equate;
use super::glb::Glb;
use super::{InferCtxt, MiscVariable, TypeTrace};
use super::lub::Lub;
use super::sub::Sub;
use super::type_variable::TypeVariableValue;
use super::unify_key::{ConstVarValue, ConstVariableValue};
use super::unify_key::{ConstVariableOrigin, ConstVariableOriginKind};
use super::unify_key::replace_if_possible;

use crate::hir::def_id::DefId;
use crate::ty::{IntType, UintType};
use crate::ty::{self, Ty, TyCtxt, InferConst};
use crate::ty::error::TypeError;
use crate::ty::relate::{self, Relate, RelateResult, TypeRelation};
use crate::ty::subst::SubstsRef;
use crate::traits::{Obligation, PredicateObligations};

use syntax::ast;
use syntax_pos::{Span, DUMMY_SP};

#[derive(Clone)]
pub struct CombineFields<'infcx, 'tcx> {
    pub infcx: &'infcx InferCtxt<'infcx, 'tcx>,
    pub trace: TypeTrace<'tcx>,
    pub cause: Option<ty::relate::Cause>,
    pub param_env: ty::ParamEnv<'tcx>,
    pub obligations: PredicateObligations<'tcx>,
}

#[derive(Copy, Clone, Debug)]
pub enum RelationDir {
    SubtypeOf, SupertypeOf, EqTo
}

impl<'infcx, 'tcx> InferCtxt<'infcx, 'tcx> {
    pub fn super_combine_tys<R>(
        &self,
        relation: &mut R,
        a: Ty<'tcx>,
        b: Ty<'tcx>,
    ) -> RelateResult<'tcx, Ty<'tcx>>
    where
        R: TypeRelation<'tcx>,
    {
        let a_is_expected = relation.a_is_expected();

        match (&a.kind, &b.kind) {
            // Relate integral variables to other types
            (&ty::Infer(ty::IntVar(a_id)), &ty::Infer(ty::IntVar(b_id))) => {
                self.int_unification_table
                    .borrow_mut()
                    .unify_var_var(a_id, b_id)
                    .map_err(|e| int_unification_error(a_is_expected, e))?;
                Ok(a)
            }
            (&ty::Infer(ty::IntVar(v_id)), &ty::Int(v)) => {
                self.unify_integral_variable(a_is_expected, v_id, IntType(v))
            }
            (&ty::Int(v), &ty::Infer(ty::IntVar(v_id))) => {
                self.unify_integral_variable(!a_is_expected, v_id, IntType(v))
            }
            (&ty::Infer(ty::IntVar(v_id)), &ty::Uint(v)) => {
                self.unify_integral_variable(a_is_expected, v_id, UintType(v))
            }
            (&ty::Uint(v), &ty::Infer(ty::IntVar(v_id))) => {
                self.unify_integral_variable(!a_is_expected, v_id, UintType(v))
            }

            // Relate floating-point variables to other types
            (&ty::Infer(ty::FloatVar(a_id)), &ty::Infer(ty::FloatVar(b_id))) => {
                self.float_unification_table
                    .borrow_mut()
                    .unify_var_var(a_id, b_id)
                    .map_err(|e| float_unification_error(relation.a_is_expected(), e))?;
                Ok(a)
            }
            (&ty::Infer(ty::FloatVar(v_id)), &ty::Float(v)) => {
                self.unify_float_variable(a_is_expected, v_id, v)
            }
            (&ty::Float(v), &ty::Infer(ty::FloatVar(v_id))) => {
                self.unify_float_variable(!a_is_expected, v_id, v)
            }

            // All other cases of inference are errors
            (&ty::Infer(_), _) |
            (_, &ty::Infer(_)) => {
                Err(TypeError::Sorts(ty::relate::expected_found(relation, &a, &b)))
            }

            _ => {
                ty::relate::super_relate_tys(relation, a, b)
            }
        }
    }

    pub fn super_combine_consts<R>(
        &self,
        relation: &mut R,
        a: &'tcx ty::Const<'tcx>,
        b: &'tcx ty::Const<'tcx>,
    ) -> RelateResult<'tcx, &'tcx ty::Const<'tcx>>
    where
        R: TypeRelation<'tcx>,
    {
        debug!("{}.consts({:?}, {:?})", relation.tag(), a, b);
        if a == b { return Ok(a); }

        let a = replace_if_possible(self.const_unification_table.borrow_mut(), a);
        let b = replace_if_possible(self.const_unification_table.borrow_mut(), b);

        let a_is_expected = relation.a_is_expected();

        match (a.val, b.val) {
            (ty::ConstKind::Infer(InferConst::Var(a_vid)),
                ty::ConstKind::Infer(InferConst::Var(b_vid))) => {
                self.const_unification_table
                    .borrow_mut()
                    .unify_var_var(a_vid, b_vid)
                    .map_err(|e| const_unification_error(a_is_expected, e))?;
                return Ok(a);
            }

            // All other cases of inference with other variables are errors.
            (ty::ConstKind::Infer(InferConst::Var(_)), ty::ConstKind::Infer(_)) |
            (ty::ConstKind::Infer(_), ty::ConstKind::Infer(InferConst::Var(_))) => {
                bug!("tried to combine ConstKind::Infer/ConstKind::Infer(InferConst::Var)")
            }

            (ty::ConstKind::Infer(InferConst::Var(vid)), _) => {
                return self.unify_const_variable(a_is_expected, vid, b);
            }

            (_, ty::ConstKind::Infer(InferConst::Var(vid))) => {
                return self.unify_const_variable(!a_is_expected, vid, a);
            }

            _ => {}
        }

        ty::relate::super_relate_consts(relation, a, b)
    }

    pub fn unify_const_variable(
        &self,
        vid_is_expected: bool,
        vid: ty::ConstVid<'tcx>,
        value: &'tcx ty::Const<'tcx>,
    ) -> RelateResult<'tcx, &'tcx ty::Const<'tcx>> {
        self.const_unification_table
            .borrow_mut()
            .unify_var_value(vid, ConstVarValue {
                origin: ConstVariableOrigin {
                    kind: ConstVariableOriginKind::ConstInference,
                    span: DUMMY_SP,
                },
                val: ConstVariableValue::Known { value },
            })
            .map_err(|e| const_unification_error(vid_is_expected, e))?;
        Ok(value)
    }

    fn unify_integral_variable(&self,
                               vid_is_expected: bool,
                               vid: ty::IntVid,
                               val: ty::IntVarValue)
                               -> RelateResult<'tcx, Ty<'tcx>>
    {
        self.int_unification_table
            .borrow_mut()
            .unify_var_value(vid, Some(val))
            .map_err(|e| int_unification_error(vid_is_expected, e))?;
        match val {
            IntType(v) => Ok(self.tcx.mk_mach_int(v)),
            UintType(v) => Ok(self.tcx.mk_mach_uint(v)),
        }
    }

    fn unify_float_variable(&self,
                            vid_is_expected: bool,
                            vid: ty::FloatVid,
                            val: ast::FloatTy)
                            -> RelateResult<'tcx, Ty<'tcx>>
    {
        self.float_unification_table
            .borrow_mut()
            .unify_var_value(vid, Some(ty::FloatVarValue(val)))
            .map_err(|e| float_unification_error(vid_is_expected, e))?;
        Ok(self.tcx.mk_mach_float(val))
    }
}

impl<'infcx, 'tcx> CombineFields<'infcx, 'tcx> {
    pub fn tcx(&self) -> TyCtxt<'tcx> {
        self.infcx.tcx
    }

    pub fn equate<'a>(&'a mut self, a_is_expected: bool) -> Equate<'a, 'infcx, 'tcx> {
        Equate::new(self, a_is_expected)
    }

    pub fn sub<'a>(&'a mut self, a_is_expected: bool) -> Sub<'a, 'infcx, 'tcx> {
        Sub::new(self, a_is_expected)
    }

    pub fn lub<'a>(&'a mut self, a_is_expected: bool) -> Lub<'a, 'infcx, 'tcx> {
        Lub::new(self, a_is_expected)
    }

    pub fn glb<'a>(&'a mut self, a_is_expected: bool) -> Glb<'a, 'infcx, 'tcx> {
        Glb::new(self, a_is_expected)
    }

    /// Here, `dir` is either `EqTo`, `SubtypeOf`, or `SupertypeOf`.
    /// The idea is that we should ensure that the type `a_ty` is equal
    /// to, a subtype of, or a supertype of (respectively) the type
    /// to which `b_vid` is bound.
    ///
    /// Since `b_vid` has not yet been instantiated with a type, we
    /// will first instantiate `b_vid` with a *generalized* version
    /// of `a_ty`. Generalization introduces other inference
    /// variables wherever subtyping could occur.
    pub fn instantiate(&mut self,
                       a_ty: Ty<'tcx>,
                       dir: RelationDir,
                       b_vid: ty::TyVid,
                       a_is_expected: bool)
                       -> RelateResult<'tcx, ()>
    {
        use self::RelationDir::*;

        // Get the actual variable that b_vid has been inferred to
        debug_assert!(self.infcx.type_variables.borrow_mut().probe(b_vid).is_unknown());

        debug!("instantiate(a_ty={:?} dir={:?} b_vid={:?})", a_ty, dir, b_vid);

        // Generalize type of `a_ty` appropriately depending on the
        // direction.  As an example, assume:
        //
        // - `a_ty == &'x ?1`, where `'x` is some free region and `?1` is an
        //   inference variable,
        // - and `dir` == `SubtypeOf`.
        //
        // Then the generalized form `b_ty` would be `&'?2 ?3`, where
        // `'?2` and `?3` are fresh region/type inference
        // variables. (Down below, we will relate `a_ty <: b_ty`,
        // adding constraints like `'x: '?2` and `?1 <: ?3`.)
        let Generalization { ty: b_ty, needs_wf } = self.generalize(a_ty, b_vid, dir)?;
        debug!("instantiate(a_ty={:?}, dir={:?}, b_vid={:?}, generalized b_ty={:?})",
               a_ty, dir, b_vid, b_ty);
        self.infcx.type_variables.borrow_mut().instantiate(b_vid, b_ty);

        if needs_wf {
            self.obligations.push(Obligation::new(self.trace.cause.clone(),
                                                  self.param_env,
                                                  ty::Predicate::WellFormed(b_ty)));
        }

        // Finally, relate `b_ty` to `a_ty`, as described in previous comment.
        //
        // FIXME(#16847): This code is non-ideal because all these subtype
        // relations wind up attributed to the same spans. We need
        // to associate causes/spans with each of the relations in
        // the stack to get this right.
        match dir {
            EqTo => self.equate(a_is_expected).relate(&a_ty, &b_ty),
            SubtypeOf => self.sub(a_is_expected).relate(&a_ty, &b_ty),
            SupertypeOf => self.sub(a_is_expected).relate_with_variance(
                ty::Contravariant, &a_ty, &b_ty),
        }?;

        Ok(())
    }

    /// Attempts to generalize `ty` for the type variable `for_vid`.
    /// This checks for cycle -- that is, whether the type `ty`
    /// references `for_vid`. The `dir` is the "direction" for which we
    /// a performing the generalization (i.e., are we producing a type
    /// that can be used as a supertype etc).
    ///
    /// Preconditions:
    ///
    /// - `for_vid` is a "root vid"
    fn generalize(&self,
                  ty: Ty<'tcx>,
                  for_vid: ty::TyVid,
                  dir: RelationDir)
                  -> RelateResult<'tcx, Generalization<'tcx>>
    {
        debug!("generalize(ty={:?}, for_vid={:?}, dir={:?}", ty, for_vid, dir);
        // Determine the ambient variance within which `ty` appears.
        // The surrounding equation is:
        //
        //     ty [op] ty2
        //
        // where `op` is either `==`, `<:`, or `:>`. This maps quite
        // naturally.
        let ambient_variance = match dir {
            RelationDir::EqTo => ty::Invariant,
            RelationDir::SubtypeOf => ty::Covariant,
            RelationDir::SupertypeOf => ty::Contravariant,
        };

        debug!("generalize: ambient_variance = {:?}", ambient_variance);

        let for_universe = match self.infcx.type_variables.borrow_mut().probe(for_vid) {
            v @ TypeVariableValue::Known { .. } => panic!(
                "instantiating {:?} which has a known value {:?}",
                for_vid,
                v,
            ),
            TypeVariableValue::Unknown { universe } => universe,
        };

        debug!("generalize: for_universe = {:?}", for_universe);

        let mut generalize = Generalizer {
            infcx: self.infcx,
            span: self.trace.cause.span,
            for_vid_sub_root: self.infcx.type_variables.borrow_mut().sub_root_var(for_vid),
            for_universe,
            ambient_variance,
            needs_wf: false,
            root_ty: ty,
            param_env: self.param_env,
        };

        let ty = match generalize.relate(&ty, &ty) {
            Ok(ty) => ty,
            Err(e) => {
                debug!("generalize: failure {:?}", e);
                return Err(e);
            }
        };
        let needs_wf = generalize.needs_wf;
        debug!("generalize: success {{ {:?}, {:?} }}", ty, needs_wf);
        Ok(Generalization { ty, needs_wf })
    }
}

struct Generalizer<'cx, 'tcx> {
    infcx: &'cx InferCtxt<'cx, 'tcx>,

    /// The span, used when creating new type variables and things.
    span: Span,

    /// The vid of the type variable that is in the process of being
    /// instantiated; if we find this within the type we are folding,
    /// that means we would have created a cyclic type.
    for_vid_sub_root: ty::TyVid,

    /// The universe of the type variable that is in the process of
    /// being instantiated. Any fresh variables that we create in this
    /// process should be in that same universe.
    for_universe: ty::UniverseIndex,

    /// Track the variance as we descend into the type.
    ambient_variance: ty::Variance,

    /// See the field `needs_wf` in `Generalization`.
    needs_wf: bool,

    /// The root type that we are generalizing. Used when reporting cycles.
    root_ty: Ty<'tcx>,

    param_env: ty::ParamEnv<'tcx>,
}

/// Result from a generalization operation. This includes
/// not only the generalized type, but also a bool flag
/// indicating whether further WF checks are needed.
struct Generalization<'tcx> {
    ty: Ty<'tcx>,

    /// If true, then the generalized type may not be well-formed,
    /// even if the source type is well-formed, so we should add an
    /// additional check to enforce that it is. This arises in
    /// particular around 'bivariant' type parameters that are only
    /// constrained by a where-clause. As an example, imagine a type:
    ///
    ///     struct Foo<A, B> where A: Iterator<Item = B> {
    ///         data: A
    ///     }
    ///
    /// here, `A` will be covariant, but `B` is
    /// unconstrained. However, whatever it is, for `Foo` to be WF, it
    /// must be equal to `A::Item`. If we have an input `Foo<?A, ?B>`,
    /// then after generalization we will wind up with a type like
    /// `Foo<?C, ?D>`. When we enforce that `Foo<?A, ?B> <: Foo<?C,
    /// ?D>` (or `>:`), we will wind up with the requirement that `?A
    /// <: ?C`, but no particular relationship between `?B` and `?D`
    /// (after all, we do not know the variance of the normalized form
    /// of `A::Item` with respect to `A`). If we do nothing else, this
    /// may mean that `?D` goes unconstrained (as in #41677). So, in
    /// this scenario where we create a new type variable in a
    /// bivariant context, we set the `needs_wf` flag to true. This
    /// will force the calling code to check that `WF(Foo<?C, ?D>)`
    /// holds, which in turn implies that `?C::Item == ?D`. So once
    /// `?C` is constrained, that should suffice to restrict `?D`.
    needs_wf: bool,
}

impl TypeRelation<'tcx> for Generalizer<'_, 'tcx> {
    fn tcx(&self) -> TyCtxt<'tcx> {
        self.infcx.tcx
    }
    fn param_env(&self) -> ty::ParamEnv<'tcx> { self.param_env }

    fn tag(&self) -> &'static str {
        "Generalizer"
    }

    fn a_is_expected(&self) -> bool {
        true
    }

    fn binders<T>(&mut self, a: &ty::Binder<T>, b: &ty::Binder<T>)
                  -> RelateResult<'tcx, ty::Binder<T>>
        where T: Relate<'tcx>
    {
        Ok(ty::Binder::bind(self.relate(a.skip_binder(), b.skip_binder())?))
    }

    fn relate_item_substs(&mut self,
                          item_def_id: DefId,
                          a_subst: SubstsRef<'tcx>,
                          b_subst: SubstsRef<'tcx>)
                          -> RelateResult<'tcx, SubstsRef<'tcx>>
    {
        if self.ambient_variance == ty::Variance::Invariant {
            // Avoid fetching the variance if we are in an invariant
            // context; no need, and it can induce dependency cycles
            // (e.g., #41849).
            relate::relate_substs(self, None, a_subst, b_subst)
        } else {
            let opt_variances = self.tcx().variances_of(item_def_id);
            relate::relate_substs(self, Some(&opt_variances), a_subst, b_subst)
        }
    }

    fn relate_with_variance<T: Relate<'tcx>>(&mut self,
                                             variance: ty::Variance,
                                             a: &T,
                                             b: &T)
                                             -> RelateResult<'tcx, T>
    {
        let old_ambient_variance = self.ambient_variance;
        self.ambient_variance = self.ambient_variance.xform(variance);

        let result = self.relate(a, b);
        self.ambient_variance = old_ambient_variance;
        result
    }

    fn tys(&mut self, t: Ty<'tcx>, t2: Ty<'tcx>) -> RelateResult<'tcx, Ty<'tcx>> {
        assert_eq!(t, t2); // we are abusing TypeRelation here; both LHS and RHS ought to be ==

        debug!("generalize: t={:?}", t);

        // Check to see whether the type we are generalizing references
        // any other type variable related to `vid` via
        // subtyping. This is basically our "occurs check", preventing
        // us from creating infinitely sized types.
        match t.kind {
            ty::Infer(ty::TyVar(vid)) => {
                let mut variables = self.infcx.type_variables.borrow_mut();
                let vid = variables.root_var(vid);
                let sub_vid = variables.sub_root_var(vid);
                if sub_vid == self.for_vid_sub_root {
                    // If sub-roots are equal, then `for_vid` and
                    // `vid` are related via subtyping.
                    Err(TypeError::CyclicTy(self.root_ty))
                } else {
                    match variables.probe(vid) {
                        TypeVariableValue::Known { value: u } => {
                            drop(variables);
                            debug!("generalize: known value {:?}", u);
                            self.relate(&u, &u)
                        }
                        TypeVariableValue::Unknown { universe } => {
                            match self.ambient_variance {
                                // Invariant: no need to make a fresh type variable.
                                ty::Invariant => {
                                    if self.for_universe.can_name(universe) {
                                        return Ok(t);
                                    }
                                }

                                // Bivariant: make a fresh var, but we
                                // may need a WF predicate. See
                                // comment on `needs_wf` field for
                                // more info.
                                ty::Bivariant => self.needs_wf = true,

                                // Co/contravariant: this will be
                                // sufficiently constrained later on.
                                ty::Covariant | ty::Contravariant => (),
                            }

                            let origin = *variables.var_origin(vid);
                            let new_var_id = variables.new_var(self.for_universe, false, origin);
                            let u = self.tcx().mk_ty_var(new_var_id);
                            debug!("generalize: replacing original vid={:?} with new={:?}",
                                   vid, u);
                            Ok(u)
                        }
                    }
                }
            }
            ty::Infer(ty::IntVar(_)) |
            ty::Infer(ty::FloatVar(_)) => {
                // No matter what mode we are in,
                // integer/floating-point types must be equal to be
                // relatable.
                Ok(t)
            }
            _ => {
                relate::super_relate_tys(self, t, t)
            }
        }
    }

    fn regions(&mut self, r: ty::Region<'tcx>, r2: ty::Region<'tcx>)
               -> RelateResult<'tcx, ty::Region<'tcx>> {
        assert_eq!(r, r2); // we are abusing TypeRelation here; both LHS and RHS ought to be ==

        debug!("generalize: regions r={:?}", r);

        match *r {
            // Never make variables for regions bound within the type itself,
            // nor for erased regions.
            ty::ReLateBound(..) |
            ty::ReErased => {
                return Ok(r);
            }

            ty::ReClosureBound(..) => {
                span_bug!(
                    self.span,
                    "encountered unexpected ReClosureBound: {:?}",
                    r,
                );
            }

            ty::RePlaceholder(..) |
            ty::ReVar(..) |
            ty::ReEmpty |
            ty::ReStatic |
            ty::ReScope(..) |
            ty::ReEarlyBound(..) |
            ty::ReFree(..) => {
                // see common code below
            }
        }

        // If we are in an invariant context, we can re-use the region
        // as is, unless it happens to be in some universe that we
        // can't name. (In the case of a region *variable*, we could
        // use it if we promoted it into our universe, but we don't
        // bother.)
        if let ty::Invariant = self.ambient_variance {
            let r_universe = self.infcx.universe_of_region(r);
            if self.for_universe.can_name(r_universe) {
                return Ok(r);
            }
        }

        // FIXME: This is non-ideal because we don't give a
        // very descriptive origin for this region variable.
        Ok(self.infcx.next_region_var_in_universe(MiscVariable(self.span), self.for_universe))
    }

    fn consts(
        &mut self,
        c: &'tcx ty::Const<'tcx>,
        c2: &'tcx ty::Const<'tcx>
    ) -> RelateResult<'tcx, &'tcx ty::Const<'tcx>> {
        assert_eq!(c, c2); // we are abusing TypeRelation here; both LHS and RHS ought to be ==

        match c.val {
            ty::ConstKind::Infer(InferConst::Var(vid)) => {
                let mut variable_table = self.infcx.const_unification_table.borrow_mut();
                let var_value = variable_table.probe_value(vid);
                match var_value.val {
                    ConstVariableValue::Known { value: u } => self.relate(&u, &u),
                    ConstVariableValue::Unknown { universe } => {
                        if self.for_universe.can_name(universe) {
                            Ok(c)
                        } else {
                            let new_var_id = variable_table.new_key(ConstVarValue {
                                origin: var_value.origin,
                                val: ConstVariableValue::Unknown { universe: self.for_universe },
                            });
                            Ok(self.tcx().mk_const_var(new_var_id, c.ty))
                        }
                    }
                }
            }
            _ => relate::super_relate_consts(self, c, c),
        }
    }
}

pub trait RelateResultCompare<'tcx, T> {
    fn compare<F>(&self, t: T, f: F) -> RelateResult<'tcx, T> where
        F: FnOnce() -> TypeError<'tcx>;
}

impl<'tcx, T:Clone + PartialEq> RelateResultCompare<'tcx, T> for RelateResult<'tcx, T> {
    fn compare<F>(&self, t: T, f: F) -> RelateResult<'tcx, T> where
        F: FnOnce() -> TypeError<'tcx>,
    {
        self.clone().and_then(|s| {
            if s == t {
                self.clone()
            } else {
                Err(f())
            }
        })
    }
}

pub fn const_unification_error<'tcx>(
    a_is_expected: bool,
    (a, b): (&'tcx ty::Const<'tcx>, &'tcx ty::Const<'tcx>),
) -> TypeError<'tcx> {
    TypeError::ConstMismatch(ty::relate::expected_found_bool(a_is_expected, &a, &b))
}

fn int_unification_error<'tcx>(a_is_expected: bool, v: (ty::IntVarValue, ty::IntVarValue))
                               -> TypeError<'tcx>
{
    let (a, b) = v;
    TypeError::IntMismatch(ty::relate::expected_found_bool(a_is_expected, &a, &b))
}

fn float_unification_error<'tcx>(a_is_expected: bool,
                                 v: (ty::FloatVarValue, ty::FloatVarValue))
                                 -> TypeError<'tcx>
{
    let (ty::FloatVarValue(a), ty::FloatVarValue(b)) = v;
    TypeError::FloatMismatch(ty::relate::expected_found_bool(a_is_expected, &a, &b))
}
