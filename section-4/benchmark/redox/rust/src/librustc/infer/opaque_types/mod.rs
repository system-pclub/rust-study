use crate::hir;
use crate::hir::def_id::DefId;
use crate::hir::Node;
use crate::infer::outlives::free_region_map::FreeRegionRelations;
use crate::infer::{self, InferCtxt, InferOk, TypeVariableOrigin, TypeVariableOriginKind};
use crate::middle::region;
use crate::traits::{self, PredicateObligation};
use crate::ty::fold::{BottomUpFolder, TypeFoldable, TypeFolder, TypeVisitor};
use crate::ty::subst::{InternalSubsts, GenericArg, SubstsRef, GenericArgKind};
use crate::ty::{self, GenericParamDefKind, Ty, TyCtxt};
use crate::util::nodemap::DefIdMap;
use errors::DiagnosticBuilder;
use rustc::session::config::nightly_options;
use rustc_data_structures::fx::FxHashMap;
use rustc_data_structures::sync::Lrc;
use syntax_pos::Span;

use rustc_error_codes::*;

pub type OpaqueTypeMap<'tcx> = DefIdMap<OpaqueTypeDecl<'tcx>>;

/// Information about the opaque types whose values we
/// are inferring in this function (these are the `impl Trait` that
/// appear in the return type).
#[derive(Copy, Clone, Debug)]
pub struct OpaqueTypeDecl<'tcx> {

    /// The opaque type (`ty::Opaque`) for this declaration.
    pub opaque_type: Ty<'tcx>,

    /// The substitutions that we apply to the opaque type that this
    /// `impl Trait` desugars to. e.g., if:
    ///
    ///     fn foo<'a, 'b, T>() -> impl Trait<'a>
    ///
    /// winds up desugared to:
    ///
    ///     type Foo<'x, X> = impl Trait<'x>
    ///     fn foo<'a, 'b, T>() -> Foo<'a, T>
    ///
    /// then `substs` would be `['a, T]`.
    pub substs: SubstsRef<'tcx>,

    /// The span of this particular definition of the opaque type.  So
    /// for example:
    ///
    /// ```
    /// type Foo = impl Baz;
    /// fn bar() -> Foo {
    ///             ^^^ This is the span we are looking for!
    /// ```
    ///
    /// In cases where the fn returns `(impl Trait, impl Trait)` or
    /// other such combinations, the result is currently
    /// over-approximated, but better than nothing.
    pub definition_span: Span,

    /// The type variable that represents the value of the opaque type
    /// that we require. In other words, after we compile this function,
    /// we will be created a constraint like:
    ///
    ///     Foo<'a, T> = ?C
    ///
    /// where `?C` is the value of this type variable. =) It may
    /// naturally refer to the type and lifetime parameters in scope
    /// in this function, though ultimately it should only reference
    /// those that are arguments to `Foo` in the constraint above. (In
    /// other words, `?C` should not include `'b`, even though it's a
    /// lifetime parameter on `foo`.)
    pub concrete_ty: Ty<'tcx>,

    /// Returns `true` if the `impl Trait` bounds include region bounds.
    /// For example, this would be true for:
    ///
    ///     fn foo<'a, 'b, 'c>() -> impl Trait<'c> + 'a + 'b
    ///
    /// but false for:
    ///
    ///     fn foo<'c>() -> impl Trait<'c>
    ///
    /// unless `Trait` was declared like:
    ///
    ///     trait Trait<'c>: 'c
    ///
    /// in which case it would be true.
    ///
    /// This is used during regionck to decide whether we need to
    /// impose any additional constraints to ensure that region
    /// variables in `concrete_ty` wind up being constrained to
    /// something from `substs` (or, at minimum, things that outlive
    /// the fn body). (Ultimately, writeback is responsible for this
    /// check.)
    pub has_required_region_bounds: bool,

    /// The origin of the opaque type.
    pub origin: hir::OpaqueTyOrigin,
}

impl<'a, 'tcx> InferCtxt<'a, 'tcx> {
    /// Replaces all opaque types in `value` with fresh inference variables
    /// and creates appropriate obligations. For example, given the input:
    ///
    ///     impl Iterator<Item = impl Debug>
    ///
    /// this method would create two type variables, `?0` and `?1`. It would
    /// return the type `?0` but also the obligations:
    ///
    ///     ?0: Iterator<Item = ?1>
    ///     ?1: Debug
    ///
    /// Moreover, it returns a `OpaqueTypeMap` that would map `?0` to
    /// info about the `impl Iterator<..>` type and `?1` to info about
    /// the `impl Debug` type.
    ///
    /// # Parameters
    ///
    /// - `parent_def_id` -- the `DefId` of the function in which the opaque type
    ///   is defined
    /// - `body_id` -- the body-id with which the resulting obligations should
    ///   be associated
    /// - `param_env` -- the in-scope parameter environment to be used for
    ///   obligations
    /// - `value` -- the value within which we are instantiating opaque types
    /// - `value_span` -- the span where the value came from, used in error reporting
    pub fn instantiate_opaque_types<T: TypeFoldable<'tcx>>(
        &self,
        parent_def_id: DefId,
        body_id: hir::HirId,
        param_env: ty::ParamEnv<'tcx>,
        value: &T,
        value_span: Span,
    ) -> InferOk<'tcx, (T, OpaqueTypeMap<'tcx>)> {
        debug!(
            "instantiate_opaque_types(value={:?}, parent_def_id={:?}, body_id={:?}, \
             param_env={:?}, value_span={:?})",
            value, parent_def_id, body_id, param_env, value_span,
        );
        let mut instantiator = Instantiator {
            infcx: self,
            parent_def_id,
            body_id,
            param_env,
            value_span,
            opaque_types: Default::default(),
            obligations: vec![],
        };
        let value = instantiator.instantiate_opaque_types_in_map(value);
        InferOk { value: (value, instantiator.opaque_types), obligations: instantiator.obligations }
    }

    /// Given the map `opaque_types` containing the opaque
    /// `impl Trait` types whose underlying, hidden types are being
    /// inferred, this method adds constraints to the regions
    /// appearing in those underlying hidden types to ensure that they
    /// at least do not refer to random scopes within the current
    /// function. These constraints are not (quite) sufficient to
    /// guarantee that the regions are actually legal values; that
    /// final condition is imposed after region inference is done.
    ///
    /// # The Problem
    ///
    /// Let's work through an example to explain how it works. Assume
    /// the current function is as follows:
    ///
    /// ```text
    /// fn foo<'a, 'b>(..) -> (impl Bar<'a>, impl Bar<'b>)
    /// ```
    ///
    /// Here, we have two `impl Trait` types whose values are being
    /// inferred (the `impl Bar<'a>` and the `impl
    /// Bar<'b>`). Conceptually, this is sugar for a setup where we
    /// define underlying opaque types (`Foo1`, `Foo2`) and then, in
    /// the return type of `foo`, we *reference* those definitions:
    ///
    /// ```text
    /// type Foo1<'x> = impl Bar<'x>;
    /// type Foo2<'x> = impl Bar<'x>;
    /// fn foo<'a, 'b>(..) -> (Foo1<'a>, Foo2<'b>) { .. }
    ///                    //  ^^^^ ^^
    ///                    //  |    |
    ///                    //  |    substs
    ///                    //  def_id
    /// ```
    ///
    /// As indicating in the comments above, each of those references
    /// is (in the compiler) basically a substitution (`substs`)
    /// applied to the type of a suitable `def_id` (which identifies
    /// `Foo1` or `Foo2`).
    ///
    /// Now, at this point in compilation, what we have done is to
    /// replace each of the references (`Foo1<'a>`, `Foo2<'b>`) with
    /// fresh inference variables C1 and C2. We wish to use the values
    /// of these variables to infer the underlying types of `Foo1` and
    /// `Foo2`. That is, this gives rise to higher-order (pattern) unification
    /// constraints like:
    ///
    /// ```text
    /// for<'a> (Foo1<'a> = C1)
    /// for<'b> (Foo1<'b> = C2)
    /// ```
    ///
    /// For these equation to be satisfiable, the types `C1` and `C2`
    /// can only refer to a limited set of regions. For example, `C1`
    /// can only refer to `'static` and `'a`, and `C2` can only refer
    /// to `'static` and `'b`. The job of this function is to impose that
    /// constraint.
    ///
    /// Up to this point, C1 and C2 are basically just random type
    /// inference variables, and hence they may contain arbitrary
    /// regions. In fact, it is fairly likely that they do! Consider
    /// this possible definition of `foo`:
    ///
    /// ```text
    /// fn foo<'a, 'b>(x: &'a i32, y: &'b i32) -> (impl Bar<'a>, impl Bar<'b>) {
    ///         (&*x, &*y)
    ///     }
    /// ```
    ///
    /// Here, the values for the concrete types of the two impl
    /// traits will include inference variables:
    ///
    /// ```text
    /// &'0 i32
    /// &'1 i32
    /// ```
    ///
    /// Ordinarily, the subtyping rules would ensure that these are
    /// sufficiently large. But since `impl Bar<'a>` isn't a specific
    /// type per se, we don't get such constraints by default. This
    /// is where this function comes into play. It adds extra
    /// constraints to ensure that all the regions which appear in the
    /// inferred type are regions that could validly appear.
    ///
    /// This is actually a bit of a tricky constraint in general. We
    /// want to say that each variable (e.g., `'0`) can only take on
    /// values that were supplied as arguments to the opaque type
    /// (e.g., `'a` for `Foo1<'a>`) or `'static`, which is always in
    /// scope. We don't have a constraint quite of this kind in the current
    /// region checker.
    ///
    /// # The Solution
    ///
    /// We generally prefer to make `<=` constraints, since they
    /// integrate best into the region solver. To do that, we find the
    /// "minimum" of all the arguments that appear in the substs: that
    /// is, some region which is less than all the others. In the case
    /// of `Foo1<'a>`, that would be `'a` (it's the only choice, after
    /// all). Then we apply that as a least bound to the variables
    /// (e.g., `'a <= '0`).
    ///
    /// In some cases, there is no minimum. Consider this example:
    ///
    /// ```text
    /// fn baz<'a, 'b>() -> impl Trait<'a, 'b> { ... }
    /// ```
    ///
    /// Here we would report a more complex "in constraint", like `'r
    /// in ['a, 'b, 'static]` (where `'r` is some regon appearing in
    /// the hidden type).
    ///
    /// # Constrain regions, not the hidden concrete type
    ///
    /// Note that generating constraints on each region `Rc` is *not*
    /// the same as generating an outlives constraint on `Tc` iself.
    /// For example, if we had a function like this:
    ///
    /// ```rust
    /// fn foo<'a, T>(x: &'a u32, y: T) -> impl Foo<'a> {
    ///   (x, y)
    /// }
    ///
    /// // Equivalent to:
    /// type FooReturn<'a, T> = impl Foo<'a>;
    /// fn foo<'a, T>(..) -> FooReturn<'a, T> { .. }
    /// ```
    ///
    /// then the hidden type `Tc` would be `(&'0 u32, T)` (where `'0`
    /// is an inference variable). If we generated a constraint that
    /// `Tc: 'a`, then this would incorrectly require that `T: 'a` --
    /// but this is not necessary, because the opaque type we
    /// create will be allowed to reference `T`. So we only generate a
    /// constraint that `'0: 'a`.
    ///
    /// # The `free_region_relations` parameter
    ///
    /// The `free_region_relations` argument is used to find the
    /// "minimum" of the regions supplied to a given opaque type.
    /// It must be a relation that can answer whether `'a <= 'b`,
    /// where `'a` and `'b` are regions that appear in the "substs"
    /// for the opaque type references (the `<'a>` in `Foo1<'a>`).
    ///
    /// Note that we do not impose the constraints based on the
    /// generic regions from the `Foo1` definition (e.g., `'x`). This
    /// is because the constraints we are imposing here is basically
    /// the concern of the one generating the constraining type C1,
    /// which is the current function. It also means that we can
    /// take "implied bounds" into account in some cases:
    ///
    /// ```text
    /// trait SomeTrait<'a, 'b> { }
    /// fn foo<'a, 'b>(_: &'a &'b u32) -> impl SomeTrait<'a, 'b> { .. }
    /// ```
    ///
    /// Here, the fact that `'b: 'a` is known only because of the
    /// implied bounds from the `&'a &'b u32` parameter, and is not
    /// "inherent" to the opaque type definition.
    ///
    /// # Parameters
    ///
    /// - `opaque_types` -- the map produced by `instantiate_opaque_types`
    /// - `free_region_relations` -- something that can be used to relate
    ///   the free regions (`'a`) that appear in the impl trait.
    pub fn constrain_opaque_types<FRR: FreeRegionRelations<'tcx>>(
        &self,
        opaque_types: &OpaqueTypeMap<'tcx>,
        free_region_relations: &FRR,
    ) {
        debug!("constrain_opaque_types()");

        for (&def_id, opaque_defn) in opaque_types {
            self.constrain_opaque_type(def_id, opaque_defn, free_region_relations);
        }
    }

    /// See `constrain_opaque_types` for documentation.
    pub fn constrain_opaque_type<FRR: FreeRegionRelations<'tcx>>(
        &self,
        def_id: DefId,
        opaque_defn: &OpaqueTypeDecl<'tcx>,
        free_region_relations: &FRR,
    ) {
        debug!("constrain_opaque_type()");
        debug!("constrain_opaque_type: def_id={:?}", def_id);
        debug!("constrain_opaque_type: opaque_defn={:#?}", opaque_defn);

        let tcx = self.tcx;

        let concrete_ty = self.resolve_vars_if_possible(&opaque_defn.concrete_ty);

        debug!("constrain_opaque_type: concrete_ty={:?}", concrete_ty);

        let opaque_type_generics = tcx.generics_of(def_id);

        let span = tcx.def_span(def_id);

        // If there are required region bounds, we can use them.
        if opaque_defn.has_required_region_bounds {
            let predicates_of = tcx.predicates_of(def_id);
            debug!("constrain_opaque_type: predicates: {:#?}", predicates_of,);
            let bounds = predicates_of.instantiate(tcx, opaque_defn.substs);
            debug!("constrain_opaque_type: bounds={:#?}", bounds);
            let opaque_type = tcx.mk_opaque(def_id, opaque_defn.substs);

            let required_region_bounds = tcx.required_region_bounds(opaque_type, bounds.predicates);
            debug_assert!(!required_region_bounds.is_empty());

            for required_region in required_region_bounds {
                concrete_ty.visit_with(&mut ConstrainOpaqueTypeRegionVisitor {
                    tcx: self.tcx,
                    op: |r| self.sub_regions(infer::CallReturn(span), required_region, r),
                });
            }
            return;
        }

        // There were no `required_region_bounds`,
        // so we have to search for a `least_region`.
        // Go through all the regions used as arguments to the
        // opaque type. These are the parameters to the opaque
        // type; so in our example above, `substs` would contain
        // `['a]` for the first impl trait and `'b` for the
        // second.
        let mut least_region = None;
        for param in &opaque_type_generics.params {
            match param.kind {
                GenericParamDefKind::Lifetime => {}
                _ => continue,
            }

            // Get the value supplied for this region from the substs.
            let subst_arg = opaque_defn.substs.region_at(param.index as usize);

            // Compute the least upper bound of it with the other regions.
            debug!("constrain_opaque_types: least_region={:?}", least_region);
            debug!("constrain_opaque_types: subst_arg={:?}", subst_arg);
            match least_region {
                None => least_region = Some(subst_arg),
                Some(lr) => {
                    if free_region_relations.sub_free_regions(lr, subst_arg) {
                        // keep the current least region
                    } else if free_region_relations.sub_free_regions(subst_arg, lr) {
                        // switch to `subst_arg`
                        least_region = Some(subst_arg);
                    } else {
                        // There are two regions (`lr` and
                        // `subst_arg`) which are not relatable. We
                        // can't find a best choice. Therefore,
                        // instead of creating a single bound like
                        // `'r: 'a` (which is our preferred choice),
                        // we will create a "in bound" like `'r in
                        // ['a, 'b, 'c]`, where `'a..'c` are the
                        // regions that appear in the impl trait.
                        return self.generate_member_constraint(
                            concrete_ty,
                            opaque_type_generics,
                            opaque_defn,
                            def_id,
                            lr,
                            subst_arg,
                        );
                    }
                }
            }
        }

        let least_region = least_region.unwrap_or(tcx.lifetimes.re_static);
        debug!("constrain_opaque_types: least_region={:?}", least_region);

        concrete_ty.visit_with(&mut ConstrainOpaqueTypeRegionVisitor {
            tcx: self.tcx,
            op: |r| self.sub_regions(infer::CallReturn(span), least_region, r),
        });
    }

    /// As a fallback, we sometimes generate an "in constraint". For
    /// a case like `impl Foo<'a, 'b>`, where `'a` and `'b` cannot be
    /// related, we would generate a constraint `'r in ['a, 'b,
    /// 'static]` for each region `'r` that appears in the hidden type
    /// (i.e., it must be equal to `'a`, `'b`, or `'static`).
    ///
    /// `conflict1` and `conflict2` are the two region bounds that we
    /// detected which were unrelated. They are used for diagnostics.
    fn generate_member_constraint(
        &self,
        concrete_ty: Ty<'tcx>,
        opaque_type_generics: &ty::Generics,
        opaque_defn: &OpaqueTypeDecl<'tcx>,
        opaque_type_def_id: DefId,
        conflict1: ty::Region<'tcx>,
        conflict2: ty::Region<'tcx>,
    ) {
        // For now, enforce a feature gate outside of async functions.
        if self.member_constraint_feature_gate(
            opaque_defn,
            opaque_type_def_id,
            conflict1,
            conflict2,
        ) {
            return;
        }

        // Create the set of choice regions: each region in the hidden
        // type can be equal to any of the region parameters of the
        // opaque type definition.
        let choice_regions: Lrc<Vec<ty::Region<'tcx>>> = Lrc::new(
            opaque_type_generics
                .params
                .iter()
                .filter(|param| match param.kind {
                    GenericParamDefKind::Lifetime => true,
                    GenericParamDefKind::Type { .. } | GenericParamDefKind::Const => false,
                })
                .map(|param| opaque_defn.substs.region_at(param.index as usize))
                .chain(std::iter::once(self.tcx.lifetimes.re_static))
                .collect(),
        );

        concrete_ty.visit_with(&mut ConstrainOpaqueTypeRegionVisitor {
            tcx: self.tcx,
            op: |r| self.member_constraint(
                opaque_type_def_id,
                opaque_defn.definition_span,
                concrete_ty,
                r,
                &choice_regions,
            ),
        });
    }

    /// Member constraints are presently feature-gated except for
    /// async-await. We expect to lift this once we've had a bit more
    /// time.
    fn member_constraint_feature_gate(
        &self,
        opaque_defn: &OpaqueTypeDecl<'tcx>,
        opaque_type_def_id: DefId,
        conflict1: ty::Region<'tcx>,
        conflict2: ty::Region<'tcx>,
    ) -> bool {
        // If we have `#![feature(member_constraints)]`, no problems.
        if self.tcx.features().member_constraints {
            return false;
        }

        let span = self.tcx.def_span(opaque_type_def_id);

        // Without a feature-gate, we only generate member-constraints for async-await.
        let context_name = match opaque_defn.origin {
            // No feature-gate required for `async fn`.
            hir::OpaqueTyOrigin::AsyncFn => return false,

            // Otherwise, generate the label we'll use in the error message.
            hir::OpaqueTyOrigin::TypeAlias => "impl Trait",
            hir::OpaqueTyOrigin::FnReturn => "impl Trait",
        };
        let msg = format!("ambiguous lifetime bound in `{}`", context_name);
        let mut err = self.tcx.sess.struct_span_err(span, &msg);

        let conflict1_name = conflict1.to_string();
        let conflict2_name = conflict2.to_string();
        let label_owned;
        let label = match (&*conflict1_name, &*conflict2_name) {
            ("'_", "'_") => "the elided lifetimes here do not outlive one another",
            _ => {
                label_owned = format!(
                    "neither `{}` nor `{}` outlives the other",
                    conflict1_name, conflict2_name,
                );
                &label_owned
            }
        };
        err.span_label(span, label);

        if nightly_options::is_nightly_build() {
            help!(err,
                  "add #![feature(member_constraints)] to the crate attributes \
                   to enable");
        }

        err.emit();
        true
    }

    /// Given the fully resolved, instantiated type for an opaque
    /// type, i.e., the value of an inference variable like C1 or C2
    /// (*), computes the "definition type" for an opaque type
    /// definition -- that is, the inferred value of `Foo1<'x>` or
    /// `Foo2<'x>` that we would conceptually use in its definition:
    ///
    ///     type Foo1<'x> = impl Bar<'x> = AAA; <-- this type AAA
    ///     type Foo2<'x> = impl Bar<'x> = BBB; <-- or this type BBB
    ///     fn foo<'a, 'b>(..) -> (Foo1<'a>, Foo2<'b>) { .. }
    ///
    /// Note that these values are defined in terms of a distinct set of
    /// generic parameters (`'x` instead of `'a`) from C1 or C2. The main
    /// purpose of this function is to do that translation.
    ///
    /// (*) C1 and C2 were introduced in the comments on
    /// `constrain_opaque_types`. Read that comment for more context.
    ///
    /// # Parameters
    ///
    /// - `def_id`, the `impl Trait` type
    /// - `opaque_defn`, the opaque definition created in `instantiate_opaque_types`
    /// - `instantiated_ty`, the inferred type C1 -- fully resolved, lifted version of
    ///   `opaque_defn.concrete_ty`
    pub fn infer_opaque_definition_from_instantiation(
        &self,
        def_id: DefId,
        opaque_defn: &OpaqueTypeDecl<'tcx>,
        instantiated_ty: Ty<'tcx>,
        span: Span,
    ) -> Ty<'tcx> {
        debug!(
            "infer_opaque_definition_from_instantiation(def_id={:?}, instantiated_ty={:?})",
            def_id, instantiated_ty
        );

        // Use substs to build up a reverse map from regions to their
        // identity mappings. This is necessary because of `impl
        // Trait` lifetimes are computed by replacing existing
        // lifetimes with 'static and remapping only those used in the
        // `impl Trait` return type, resulting in the parameters
        // shifting.
        let id_substs = InternalSubsts::identity_for_item(self.tcx, def_id);
        let map: FxHashMap<GenericArg<'tcx>, GenericArg<'tcx>> = opaque_defn
            .substs
            .iter()
            .enumerate()
            .map(|(index, subst)| (*subst, id_substs[index]))
            .collect();

        // Convert the type from the function into a type valid outside
        // the function, by replacing invalid regions with 'static,
        // after producing an error for each of them.
        let definition_ty = instantiated_ty.fold_with(&mut ReverseMapper::new(
            self.tcx,
            self.is_tainted_by_errors(),
            def_id,
            map,
            instantiated_ty,
            span,
        ));
        debug!("infer_opaque_definition_from_instantiation: definition_ty={:?}", definition_ty);

        definition_ty
    }
}

pub fn unexpected_hidden_region_diagnostic(
    tcx: TyCtxt<'tcx>,
    region_scope_tree: Option<&region::ScopeTree>,
    opaque_type_def_id: DefId,
    hidden_ty: Ty<'tcx>,
    hidden_region: ty::Region<'tcx>,
) -> DiagnosticBuilder<'tcx> {
    let span = tcx.def_span(opaque_type_def_id);
    let mut err = struct_span_err!(
        tcx.sess,
        span,
        E0700,
        "hidden type for `impl Trait` captures lifetime that does not appear in bounds",
    );

    // Explain the region we are capturing.
    if let ty::ReEarlyBound(_) | ty::ReFree(_) | ty::ReStatic | ty::ReEmpty = hidden_region {
        // Assuming regionck succeeded (*), we ought to always be
        // capturing *some* region from the fn header, and hence it
        // ought to be free. So under normal circumstances, we will go
        // down this path which gives a decent human readable
        // explanation.
        //
        // (*) if not, the `tainted_by_errors` flag would be set to
        // true in any case, so we wouldn't be here at all.
        tcx.note_and_explain_free_region(
            &mut err,
            &format!("hidden type `{}` captures ", hidden_ty),
            hidden_region,
            "",
        );
    } else {
        // Ugh. This is a painful case: the hidden region is not one
        // that we can easily summarize or explain. This can happen
        // in a case like
        // `src/test/ui/multiple-lifetimes/ordinary-bounds-unsuited.rs`:
        //
        // ```
        // fn upper_bounds<'a, 'b>(a: Ordinary<'a>, b: Ordinary<'b>) -> impl Trait<'a, 'b> {
        //   if condition() { a } else { b }
        // }
        // ```
        //
        // Here the captured lifetime is the intersection of `'a` and
        // `'b`, which we can't quite express.

        if let Some(region_scope_tree) = region_scope_tree {
            // If the `region_scope_tree` is available, this is being
            // invoked from the "region inferencer error". We can at
            // least report a really cryptic error for now.
            tcx.note_and_explain_region(
                region_scope_tree,
                &mut err,
                &format!("hidden type `{}` captures ", hidden_ty),
                hidden_region,
                "",
            );
        } else {
            // If the `region_scope_tree` is *unavailable*, this is
            // being invoked by the code that comes *after* region
            // inferencing. This is a bug, as the region inferencer
            // ought to have noticed the failed constraint and invoked
            // error reporting, which in turn should have prevented us
            // from getting trying to infer the hidden type
            // completely.
            tcx.sess.delay_span_bug(
                span,
                &format!(
                    "hidden type captures unexpected lifetime `{:?}` \
                     but no region inference failure",
                    hidden_region,
                ),
            );
        }
    }

    err
}

// Visitor that requires that (almost) all regions in the type visited outlive
// `least_region`. We cannot use `push_outlives_components` because regions in
// closure signatures are not included in their outlives components. We need to
// ensure all regions outlive the given bound so that we don't end up with,
// say, `ReScope` appearing in a return type and causing ICEs when other
// functions end up with region constraints involving regions from other
// functions.
//
// We also cannot use `for_each_free_region` because for closures it includes
// the regions parameters from the enclosing item.
//
// We ignore any type parameters because impl trait values are assumed to
// capture all the in-scope type parameters.
struct ConstrainOpaqueTypeRegionVisitor<'tcx, OP>
where
    OP: FnMut(ty::Region<'tcx>),
{
    tcx: TyCtxt<'tcx>,
    op: OP,
}

impl<'tcx, OP> TypeVisitor<'tcx> for ConstrainOpaqueTypeRegionVisitor<'tcx, OP>
where
    OP: FnMut(ty::Region<'tcx>),
{
    fn visit_binder<T: TypeFoldable<'tcx>>(&mut self, t: &ty::Binder<T>) -> bool {
        t.skip_binder().visit_with(self);
        false // keep visiting
    }

    fn visit_region(&mut self, r: ty::Region<'tcx>) -> bool {
        match *r {
            // ignore bound regions, keep visiting
            ty::ReLateBound(_, _) => false,
            _ => {
                (self.op)(r);
                false
            }
        }
    }

    fn visit_ty(&mut self, ty: Ty<'tcx>) -> bool {
        // We're only interested in types involving regions
        if !ty.flags.intersects(ty::TypeFlags::HAS_FREE_REGIONS) {
            return false; // keep visiting
        }

        match ty.kind {
            ty::Closure(def_id, ref substs) => {
                // Skip lifetime parameters of the enclosing item(s)

                for upvar_ty in substs.as_closure().upvar_tys(def_id, self.tcx) {
                    upvar_ty.visit_with(self);
                }

                substs.as_closure().sig_ty(def_id, self.tcx).visit_with(self);
            }

            ty::Generator(def_id, ref substs, _) => {
                // Skip lifetime parameters of the enclosing item(s)
                // Also skip the witness type, because that has no free regions.

                for upvar_ty in substs.as_generator().upvar_tys(def_id, self.tcx) {
                    upvar_ty.visit_with(self);
                }

                substs.as_generator().return_ty(def_id, self.tcx).visit_with(self);
                substs.as_generator().yield_ty(def_id, self.tcx).visit_with(self);
            }
            _ => {
                ty.super_visit_with(self);
            }
        }

        false
    }
}

struct ReverseMapper<'tcx> {
    tcx: TyCtxt<'tcx>,

    /// If errors have already been reported in this fn, we suppress
    /// our own errors because they are sometimes derivative.
    tainted_by_errors: bool,

    opaque_type_def_id: DefId,
    map: FxHashMap<GenericArg<'tcx>, GenericArg<'tcx>>,
    map_missing_regions_to_empty: bool,

    /// initially `Some`, set to `None` once error has been reported
    hidden_ty: Option<Ty<'tcx>>,

    /// Span of function being checked.
    span: Span,
}

impl ReverseMapper<'tcx> {
    fn new(
        tcx: TyCtxt<'tcx>,
        tainted_by_errors: bool,
        opaque_type_def_id: DefId,
        map: FxHashMap<GenericArg<'tcx>, GenericArg<'tcx>>,
        hidden_ty: Ty<'tcx>,
        span: Span,
    ) -> Self {
        Self {
            tcx,
            tainted_by_errors,
            opaque_type_def_id,
            map,
            map_missing_regions_to_empty: false,
            hidden_ty: Some(hidden_ty),
            span,
        }
    }

    fn fold_kind_mapping_missing_regions_to_empty(
        &mut self,
        kind: GenericArg<'tcx>,
    ) -> GenericArg<'tcx> {
        assert!(!self.map_missing_regions_to_empty);
        self.map_missing_regions_to_empty = true;
        let kind = kind.fold_with(self);
        self.map_missing_regions_to_empty = false;
        kind
    }

    fn fold_kind_normally(&mut self, kind: GenericArg<'tcx>) -> GenericArg<'tcx> {
        assert!(!self.map_missing_regions_to_empty);
        kind.fold_with(self)
    }
}

impl TypeFolder<'tcx> for ReverseMapper<'tcx> {
    fn tcx(&self) -> TyCtxt<'tcx> {
        self.tcx
    }

    fn fold_region(&mut self, r: ty::Region<'tcx>) -> ty::Region<'tcx> {
        match r {
            // ignore bound regions that appear in the type (e.g., this
            // would ignore `'r` in a type like `for<'r> fn(&'r u32)`.
            ty::ReLateBound(..) |

            // ignore `'static`, as that can appear anywhere
            ty::ReStatic => return r,

            _ => { }
        }

        let generics = self.tcx().generics_of(self.opaque_type_def_id);
        match self.map.get(&r.into()).map(|k| k.unpack()) {
            Some(GenericArgKind::Lifetime(r1)) => r1,
            Some(u) => panic!("region mapped to unexpected kind: {:?}", u),
            None if generics.parent.is_some() => {
                if !self.map_missing_regions_to_empty && !self.tainted_by_errors {
                    if let Some(hidden_ty) = self.hidden_ty.take() {
                        unexpected_hidden_region_diagnostic(
                            self.tcx,
                            None,
                            self.opaque_type_def_id,
                            hidden_ty,
                            r,
                        ).emit();
                    }
                }
                self.tcx.lifetimes.re_empty
            }
            None => {
                self.tcx.sess
                    .struct_span_err(
                        self.span,
                        "non-defining opaque type use in defining scope"
                    )
                    .span_label(
                        self.span,
                        format!("lifetime `{}` is part of concrete type but not used in \
                                 parameter list of the `impl Trait` type alias", r),
                    )
                    .emit();

                self.tcx().mk_region(ty::ReStatic)
            },
        }
    }

    fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
        match ty.kind {
            ty::Closure(def_id, substs) => {
                // I am a horrible monster and I pray for death. When
                // we encounter a closure here, it is always a closure
                // from within the function that we are currently
                // type-checking -- one that is now being encapsulated
                // in an opaque type. Ideally, we would
                // go through the types/lifetimes that it references
                // and treat them just like we would any other type,
                // which means we would error out if we find any
                // reference to a type/region that is not in the
                // "reverse map".
                //
                // **However,** in the case of closures, there is a
                // somewhat subtle (read: hacky) consideration. The
                // problem is that our closure types currently include
                // all the lifetime parameters declared on the
                // enclosing function, even if they are unused by the
                // closure itself. We can't readily filter them out,
                // so here we replace those values with `'empty`. This
                // can't really make a difference to the rest of the
                // compiler; those regions are ignored for the
                // outlives relation, and hence don't affect trait
                // selection or auto traits, and they are erased
                // during codegen.

                let generics = self.tcx.generics_of(def_id);
                let substs =
                    self.tcx.mk_substs(substs.iter().enumerate().map(|(index, &kind)| {
                        if index < generics.parent_count {
                            // Accommodate missing regions in the parent kinds...
                            self.fold_kind_mapping_missing_regions_to_empty(kind)
                        } else {
                            // ...but not elsewhere.
                            self.fold_kind_normally(kind)
                        }
                    }));

                self.tcx.mk_closure(def_id, substs)
            }

            ty::Generator(def_id, substs, movability) => {
                let generics = self.tcx.generics_of(def_id);
                let substs =
                    self.tcx.mk_substs(substs.iter().enumerate().map(|(index, &kind)| {
                        if index < generics.parent_count {
                            // Accommodate missing regions in the parent kinds...
                            self.fold_kind_mapping_missing_regions_to_empty(kind)
                        } else {
                            // ...but not elsewhere.
                            self.fold_kind_normally(kind)
                        }
                    }));

                self.tcx.mk_generator(def_id, substs, movability)
            }

            ty::Param(..) => {
                // Look it up in the substitution list.
                match self.map.get(&ty.into()).map(|k| k.unpack()) {
                    // Found it in the substitution list; replace with the parameter from the
                    // opaque type.
                    Some(GenericArgKind::Type(t1)) => t1,
                    Some(u) => panic!("type mapped to unexpected kind: {:?}", u),
                    None => {
                        self.tcx.sess
                            .struct_span_err(
                                self.span,
                                &format!("type parameter `{}` is part of concrete type but not \
                                          used in parameter list for the `impl Trait` type alias",
                                         ty),
                            )
                            .emit();

                        self.tcx().types.err
                    }
                }
            }

            _ => ty.super_fold_with(self),
        }
    }

    fn fold_const(&mut self, ct: &'tcx ty::Const<'tcx>) -> &'tcx ty::Const<'tcx> {
        trace!("checking const {:?}", ct);
        // Find a const parameter
        match ct.val {
            ty::ConstKind::Param(..) => {
                // Look it up in the substitution list.
                match self.map.get(&ct.into()).map(|k| k.unpack()) {
                    // Found it in the substitution list, replace with the parameter from the
                    // opaque type.
                    Some(GenericArgKind::Const(c1)) => c1,
                    Some(u) => panic!("const mapped to unexpected kind: {:?}", u),
                    None => {
                        self.tcx.sess
                            .struct_span_err(
                                self.span,
                                &format!("const parameter `{}` is part of concrete type but not \
                                          used in parameter list for the `impl Trait` type alias",
                                         ct)
                            )
                            .emit();

                        self.tcx().consts.err
                    }
                }
            }

            _ => ct,
        }
    }
}

struct Instantiator<'a, 'tcx> {
    infcx: &'a InferCtxt<'a, 'tcx>,
    parent_def_id: DefId,
    body_id: hir::HirId,
    param_env: ty::ParamEnv<'tcx>,
    value_span: Span,
    opaque_types: OpaqueTypeMap<'tcx>,
    obligations: Vec<PredicateObligation<'tcx>>,
}

impl<'a, 'tcx> Instantiator<'a, 'tcx> {
    fn instantiate_opaque_types_in_map<T: TypeFoldable<'tcx>>(&mut self, value: &T) -> T {
        debug!("instantiate_opaque_types_in_map(value={:?})", value);
        let tcx = self.infcx.tcx;
        value.fold_with(&mut BottomUpFolder {
            tcx,
            ty_op: |ty| {
                if ty.references_error() {
                    return tcx.types.err;
                } else if let ty::Opaque(def_id, substs) = ty.kind {
                    // Check that this is `impl Trait` type is
                    // declared by `parent_def_id` -- i.e., one whose
                    // value we are inferring.  At present, this is
                    // always true during the first phase of
                    // type-check, but not always true later on during
                    // NLL. Once we support named opaque types more fully,
                    // this same scenario will be able to arise during all phases.
                    //
                    // Here is an example using type alias `impl Trait`
                    // that indicates the distinction we are checking for:
                    //
                    // ```rust
                    // mod a {
                    //   pub type Foo = impl Iterator;
                    //   pub fn make_foo() -> Foo { .. }
                    // }
                    //
                    // mod b {
                    //   fn foo() -> a::Foo { a::make_foo() }
                    // }
                    // ```
                    //
                    // Here, the return type of `foo` references a
                    // `Opaque` indeed, but not one whose value is
                    // presently being inferred. You can get into a
                    // similar situation with closure return types
                    // today:
                    //
                    // ```rust
                    // fn foo() -> impl Iterator { .. }
                    // fn bar() {
                    //     let x = || foo(); // returns the Opaque assoc with `foo`
                    // }
                    // ```
                    if let Some(opaque_hir_id) = tcx.hir().as_local_hir_id(def_id) {
                        let parent_def_id = self.parent_def_id;
                        let def_scope_default = || {
                            let opaque_parent_hir_id = tcx.hir().get_parent_item(opaque_hir_id);
                            parent_def_id == tcx.hir()
                                                .local_def_id(opaque_parent_hir_id)
                        };
                        let (in_definition_scope, origin) = match tcx.hir().find(opaque_hir_id) {
                            Some(Node::Item(item)) => match item.kind {
                                // Anonymous `impl Trait`
                                hir::ItemKind::OpaqueTy(hir::OpaqueTy {
                                    impl_trait_fn: Some(parent),
                                    origin,
                                    ..
                                }) => (parent == self.parent_def_id, origin),
                                // Named `type Foo = impl Bar;`
                                hir::ItemKind::OpaqueTy(hir::OpaqueTy {
                                    impl_trait_fn: None,
                                    origin,
                                    ..
                                }) => (
                                    may_define_opaque_type(
                                        tcx,
                                        self.parent_def_id,
                                        opaque_hir_id,
                                    ),
                                    origin,
                                ),
                                _ => {
                                    (def_scope_default(), hir::OpaqueTyOrigin::TypeAlias)
                                }
                            },
                            Some(Node::ImplItem(item)) => match item.kind {
                                hir::ImplItemKind::OpaqueTy(_) => (
                                    may_define_opaque_type(
                                        tcx,
                                        self.parent_def_id,
                                        opaque_hir_id,
                                    ),
                                    hir::OpaqueTyOrigin::TypeAlias,
                                ),
                                _ => {
                                    (def_scope_default(), hir::OpaqueTyOrigin::TypeAlias)
                                }
                            },
                            _ => bug!(
                                "expected (impl) item, found {}",
                                tcx.hir().node_to_string(opaque_hir_id),
                            ),
                        };
                        if in_definition_scope {
                            return self.fold_opaque_ty(ty, def_id, substs, origin);
                        }

                        debug!(
                            "instantiate_opaque_types_in_map: \
                             encountered opaque outside its definition scope \
                             def_id={:?}",
                            def_id,
                        );
                    }
                }

                ty
            },
            lt_op: |lt| lt,
            ct_op: |ct| ct,
        })
    }

    fn fold_opaque_ty(
        &mut self,
        ty: Ty<'tcx>,
        def_id: DefId,
        substs: SubstsRef<'tcx>,
        origin: hir::OpaqueTyOrigin,
    ) -> Ty<'tcx> {
        let infcx = self.infcx;
        let tcx = infcx.tcx;

        debug!("instantiate_opaque_types: Opaque(def_id={:?}, substs={:?})", def_id, substs);

        // Use the same type variable if the exact same opaque type appears more
        // than once in the return type (e.g., if it's passed to a type alias).
        if let Some(opaque_defn) = self.opaque_types.get(&def_id) {
            debug!("instantiate_opaque_types: returning concrete ty {:?}", opaque_defn.concrete_ty);
            return opaque_defn.concrete_ty;
        }
        let span = tcx.def_span(def_id);
        debug!("fold_opaque_ty {:?} {:?}", self.value_span, span);
        let ty_var = infcx
            .next_ty_var(TypeVariableOrigin { kind: TypeVariableOriginKind::TypeInference, span });

        let predicates_of = tcx.predicates_of(def_id);
        debug!("instantiate_opaque_types: predicates={:#?}", predicates_of,);
        let bounds = predicates_of.instantiate(tcx, substs);

        let param_env = tcx.param_env(def_id);
        let InferOk { value: bounds, obligations } =
            infcx.partially_normalize_associated_types_in(span, self.body_id, param_env, &bounds);
        self.obligations.extend(obligations);

        debug!("instantiate_opaque_types: bounds={:?}", bounds);

        let required_region_bounds = tcx.required_region_bounds(ty, bounds.predicates.clone());
        debug!("instantiate_opaque_types: required_region_bounds={:?}", required_region_bounds);

        // Make sure that we are in fact defining the *entire* type
        // (e.g., `type Foo<T: Bound> = impl Bar;` needs to be
        // defined by a function like `fn foo<T: Bound>() -> Foo<T>`).
        debug!("instantiate_opaque_types: param_env={:#?}", self.param_env,);
        debug!("instantiate_opaque_types: generics={:#?}", tcx.generics_of(def_id),);

        // Ideally, we'd get the span where *this specific `ty` came
        // from*, but right now we just use the span from the overall
        // value being folded. In simple cases like `-> impl Foo`,
        // these are the same span, but not in cases like `-> (impl
        // Foo, impl Bar)`.
        let definition_span = self.value_span;

        self.opaque_types.insert(
            def_id,
            OpaqueTypeDecl {
                opaque_type: ty,
                substs,
                definition_span,
                concrete_ty: ty_var,
                has_required_region_bounds: !required_region_bounds.is_empty(),
                origin,
            },
        );
        debug!("instantiate_opaque_types: ty_var={:?}", ty_var);

        for predicate in &bounds.predicates {
            if let ty::Predicate::Projection(projection) = &predicate {
                if projection.skip_binder().ty.references_error() {
                    // No point on adding these obligations since there's a type error involved.
                    return ty_var;
                }
            }
        }

        self.obligations.reserve(bounds.predicates.len());
        for predicate in bounds.predicates {
            // Change the predicate to refer to the type variable,
            // which will be the concrete type instead of the opaque type.
            // This also instantiates nested instances of `impl Trait`.
            let predicate = self.instantiate_opaque_types_in_map(&predicate);

            let cause = traits::ObligationCause::new(span, self.body_id, traits::SizedReturnType);

            // Require that the predicate holds for the concrete type.
            debug!("instantiate_opaque_types: predicate={:?}", predicate);
            self.obligations.push(traits::Obligation::new(cause, self.param_env, predicate));
        }

        ty_var
    }
}

/// Returns `true` if `opaque_hir_id` is a sibling or a child of a sibling of `def_id`.
///
/// Example:
/// ```rust
/// pub mod foo {
///     pub mod bar {
///         pub trait Bar { .. }
///
///         pub type Baz = impl Bar;
///
///         fn f1() -> Baz { .. }
///     }
///
///     fn f2() -> bar::Baz { .. }
/// }
/// ```
///
/// Here, `def_id` is the `DefId` of the defining use of the opaque type (e.g., `f1` or `f2`),
/// and `opaque_hir_id` is the `HirId` of the definition of the opaque type `Baz`.
/// For the above example, this function returns `true` for `f1` and `false` for `f2`.
pub fn may_define_opaque_type(
    tcx: TyCtxt<'_>,
    def_id: DefId,
    opaque_hir_id: hir::HirId,
) -> bool {
    let mut hir_id = tcx.hir().as_local_hir_id(def_id).unwrap();

    // Named opaque types can be defined by any siblings or children of siblings.
    let scope = tcx.hir().get_defining_scope(opaque_hir_id);
    // We walk up the node tree until we hit the root or the scope of the opaque type.
    while hir_id != scope && hir_id != hir::CRATE_HIR_ID {
        hir_id = tcx.hir().get_parent_item(hir_id);
    }
    // Syntactically, we are allowed to define the concrete type if:
    let res = hir_id == scope;
    trace!(
        "may_define_opaque_type(def={:?}, opaque_node={:?}) = {}",
        tcx.hir().get(hir_id),
        tcx.hir().get(opaque_hir_id),
        res
    );
    res
}
