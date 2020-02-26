//! Method lookup: the secret sauce of Rust. See the [rustc guide] for more information.
//!
//! [rustc guide]: https://rust-lang.github.io/rustc-guide/method-lookup.html

mod confirm;
pub mod probe;
mod suggest;

pub use self::MethodError::*;
pub use self::CandidateSource::*;
pub use self::suggest::{SelfSource, TraitInfo};

use crate::check::FnCtxt;
use crate::namespace::Namespace;
use errors::{Applicability, DiagnosticBuilder};
use rustc_data_structures::sync::Lrc;
use rustc::hir;
use rustc::hir::def::{CtorOf, DefKind};
use rustc::hir::def_id::DefId;
use rustc::traits;
use rustc::ty::subst::{InternalSubsts, SubstsRef};
use rustc::ty::{self, Ty, ToPredicate, ToPolyTraitRef, TraitRef, TypeFoldable};
use rustc::ty::GenericParamDefKind;
use rustc::ty::subst::Subst;
use rustc::infer::{self, InferOk};
use syntax::ast;
use syntax_pos::Span;

use self::probe::{IsSuggestion, ProbeScope};

pub fn provide(providers: &mut ty::query::Providers<'_>) {
    suggest::provide(providers);
    probe::provide(providers);
}

#[derive(Clone, Copy, Debug)]
pub struct MethodCallee<'tcx> {
    /// Impl method ID, for inherent methods, or trait method ID, otherwise.
    pub def_id: DefId,
    pub substs: SubstsRef<'tcx>,

    /// Instantiated method signature, i.e., it has been
    /// substituted, normalized, and has had late-bound
    /// lifetimes replaced with inference variables.
    pub sig: ty::FnSig<'tcx>,
}

pub enum MethodError<'tcx> {
    // Did not find an applicable method, but we did find various near-misses that may work.
    NoMatch(NoMatchData<'tcx>),

    // Multiple methods might apply.
    Ambiguity(Vec<CandidateSource>),

    // Found an applicable method, but it is not visible. The third argument contains a list of
    // not-in-scope traits which may work.
    PrivateMatch(DefKind, DefId, Vec<DefId>),

    // Found a `Self: Sized` bound where `Self` is a trait object, also the caller may have
    // forgotten to import a trait.
    IllegalSizedBound(Vec<DefId>, bool),

    // Found a match, but the return type is wrong
    BadReturnType,
}

// Contains a list of static methods that may apply, a list of unsatisfied trait predicates which
// could lead to matches if satisfied, and a list of not-in-scope traits which may work.
pub struct NoMatchData<'tcx> {
    pub static_candidates: Vec<CandidateSource>,
    pub unsatisfied_predicates: Vec<TraitRef<'tcx>>,
    pub out_of_scope_traits: Vec<DefId>,
    pub lev_candidate: Option<ty::AssocItem>,
    pub mode: probe::Mode,
}

impl<'tcx> NoMatchData<'tcx> {
    pub fn new(static_candidates: Vec<CandidateSource>,
               unsatisfied_predicates: Vec<TraitRef<'tcx>>,
               out_of_scope_traits: Vec<DefId>,
               lev_candidate: Option<ty::AssocItem>,
               mode: probe::Mode)
               -> Self {
        NoMatchData {
            static_candidates,
            unsatisfied_predicates,
            out_of_scope_traits,
            lev_candidate,
            mode,
        }
    }
}

// A pared down enum describing just the places from which a method
// candidate can arise. Used for error reporting only.
#[derive(Copy, Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CandidateSource {
    ImplSource(DefId),
    TraitSource(DefId /* trait id */),
}

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    /// Determines whether the type `self_ty` supports a method name `method_name` or not.
    pub fn method_exists(&self,
                         method_name: ast::Ident,
                         self_ty: Ty<'tcx>,
                         call_expr_id: hir::HirId,
                         allow_private: bool)
                         -> bool {
        let mode = probe::Mode::MethodCall;
        match self.probe_for_name(method_name.span, mode, method_name,
                                  IsSuggestion(false), self_ty, call_expr_id,
                                  ProbeScope::TraitsInScope) {
            Ok(..) => true,
            Err(NoMatch(..)) => false,
            Err(Ambiguity(..)) => true,
            Err(PrivateMatch(..)) => allow_private,
            Err(IllegalSizedBound(..)) => true,
            Err(BadReturnType) => {
                bug!("no return type expectations but got BadReturnType")
            }

        }
    }

    /// Adds a suggestion to call the given method to the provided diagnostic.
    crate fn suggest_method_call(
        &self,
        err: &mut DiagnosticBuilder<'a>,
        msg: &str,
        method_name: ast::Ident,
        self_ty: Ty<'tcx>,
        call_expr_id: hir::HirId,
    ) {
        let has_params = self
            .probe_for_name(
                method_name.span,
                probe::Mode::MethodCall,
                method_name,
                IsSuggestion(false),
                self_ty,
                call_expr_id,
                ProbeScope::TraitsInScope,
            )
            .and_then(|pick| {
                let sig = self.tcx.fn_sig(pick.item.def_id);
                Ok(sig.inputs().skip_binder().len() > 1)
            });

        let (suggestion, applicability) = if has_params.unwrap_or_default() {
            (
                format!("{}(...)", method_name),
                Applicability::HasPlaceholders,
            )
        } else {
            (format!("{}()", method_name), Applicability::MaybeIncorrect)
        };

        err.span_suggestion(method_name.span, msg, suggestion, applicability);
    }

    /// Performs method lookup. If lookup is successful, it will return the callee
    /// and store an appropriate adjustment for the self-expr. In some cases it may
    /// report an error (e.g., invoking the `drop` method).
    ///
    /// # Arguments
    ///
    /// Given a method call like `foo.bar::<T1,...Tn>(...)`:
    ///
    /// * `fcx`:                   the surrounding `FnCtxt` (!)
    /// * `span`:                  the span for the method call
    /// * `method_name`:           the name of the method being called (`bar`)
    /// * `self_ty`:               the (unadjusted) type of the self expression (`foo`)
    /// * `supplied_method_types`: the explicit method type parameters, if any (`T1..Tn`)
    /// * `self_expr`:             the self expression (`foo`)
    pub fn lookup_method(
        &self,
        self_ty: Ty<'tcx>,
        segment: &hir::PathSegment,
        span: Span,
        call_expr: &'tcx hir::Expr,
        self_expr: &'tcx hir::Expr,
    ) -> Result<MethodCallee<'tcx>, MethodError<'tcx>> {
        debug!("lookup(method_name={}, self_ty={:?}, call_expr={:?}, self_expr={:?})",
               segment.ident,
               self_ty,
               call_expr,
               self_expr);

        let pick = self.lookup_probe(
            span,
            segment.ident,
            self_ty,
            call_expr,
            ProbeScope::TraitsInScope
        )?;

        for import_id in &pick.import_ids {
            let import_def_id = self.tcx.hir().local_def_id(*import_id);
            debug!("used_trait_import: {:?}", import_def_id);
            Lrc::get_mut(&mut self.tables.borrow_mut().used_trait_imports)
                .unwrap().insert(import_def_id);
        }

        self.tcx.check_stability(pick.item.def_id, Some(call_expr.hir_id), span);

        let result = self.confirm_method(
            span,
            self_expr,
            call_expr,
            self_ty,
            pick.clone(),
            segment,
        );

        if result.illegal_sized_bound {
            let mut needs_mut = false;
            if let ty::Ref(region, t_type, mutability) = self_ty.kind {
                let trait_type = self.tcx.mk_ref(region, ty::TypeAndMut {
                    ty: t_type,
                    mutbl: mutability.invert(),
                });
                // We probe again to see if there might be a borrow mutability discrepancy.
                match self.lookup_probe(
                    span,
                    segment.ident,
                    trait_type,
                    call_expr,
                    ProbeScope::TraitsInScope
                ) {
                    Ok(ref new_pick) if *new_pick != pick => {
                        needs_mut = true;
                    }
                    _ => {}
                }
            }

            // We probe again, taking all traits into account (not only those in scope).
            let candidates = match self.lookup_probe(
                span,
                segment.ident,
                self_ty,
                call_expr,
                ProbeScope::AllTraits,
            ) {
                // If we find a different result the caller probably forgot to import a trait.
                Ok(ref new_pick) if *new_pick != pick => vec![new_pick.item.container.id()],
                Err(Ambiguity(ref sources)) => sources.iter().filter_map(|source| {
                    match *source {
                        // Note: this cannot come from an inherent impl,
                        // because the first probing succeeded.
                        ImplSource(def) => self.tcx.trait_id_of_impl(def),
                        TraitSource(_) => None,
                    }
                }).collect(),
                _ => Vec::new(),
            };

            return Err(IllegalSizedBound(candidates, needs_mut));
        }

        Ok(result.callee)
    }

    pub fn lookup_probe(
        &self,
        span: Span,
        method_name: ast::Ident,
        self_ty: Ty<'tcx>,
        call_expr: &'tcx hir::Expr,
        scope: ProbeScope,
    ) -> probe::PickResult<'tcx> {
        let mode = probe::Mode::MethodCall;
        let self_ty = self.resolve_vars_if_possible(&self_ty);
        self.probe_for_name(span, mode, method_name, IsSuggestion(false),
                            self_ty, call_expr.hir_id, scope)
    }

    /// `lookup_method_in_trait` is used for overloaded operators.
    /// It does a very narrow slice of what the normal probe/confirm path does.
    /// In particular, it doesn't really do any probing: it simply constructs
    /// an obligation for a particular trait with the given self type and checks
    /// whether that trait is implemented.
    //
    // FIXME(#18741): it seems likely that we can consolidate some of this
    // code with the other method-lookup code. In particular, the second half
    // of this method is basically the same as confirmation.
    pub fn lookup_method_in_trait(&self,
                                  span: Span,
                                  m_name: ast::Ident,
                                  trait_def_id: DefId,
                                  self_ty: Ty<'tcx>,
                                  opt_input_types: Option<&[Ty<'tcx>]>)
                                  -> Option<InferOk<'tcx, MethodCallee<'tcx>>> {
        debug!("lookup_in_trait_adjusted(self_ty={:?}, \
                m_name={}, trait_def_id={:?})",
               self_ty,
               m_name,
               trait_def_id);

        // Construct a trait-reference `self_ty : Trait<input_tys>`
        let substs = InternalSubsts::for_item(self.tcx, trait_def_id, |param, _| {
            match param.kind {
                GenericParamDefKind::Lifetime | GenericParamDefKind::Const => {}
                GenericParamDefKind::Type { .. } => {
                    if param.index == 0 {
                        return self_ty.into();
                    } else if let Some(ref input_types) = opt_input_types {
                        return input_types[param.index as usize - 1].into();
                    }
                }
            }
            self.var_for_def(span, param)
        });

        let trait_ref = ty::TraitRef::new(trait_def_id, substs);

        // Construct an obligation
        let poly_trait_ref = trait_ref.to_poly_trait_ref();
        let obligation =
            traits::Obligation::misc(span,
                                     self.body_id,
                                     self.param_env,
                                     poly_trait_ref.to_predicate());

        // Now we want to know if this can be matched
        if !self.predicate_may_hold(&obligation) {
            debug!("--> Cannot match obligation");
            return None; // Cannot be matched, no such method resolution is possible.
        }

        // Trait must have a method named `m_name` and it should not have
        // type parameters or early-bound regions.
        let tcx = self.tcx;
        let method_item = match self.associated_item(trait_def_id, m_name, Namespace::Value) {
            Some(method_item) => method_item,
            None => {
                tcx.sess.delay_span_bug(span,
                    "operator trait does not have corresponding operator method");
                return None;
            }
        };
        let def_id = method_item.def_id;
        let generics = tcx.generics_of(def_id);
        assert_eq!(generics.params.len(), 0);

        debug!("lookup_in_trait_adjusted: method_item={:?}", method_item);
        let mut obligations = vec![];

        // Instantiate late-bound regions and substitute the trait
        // parameters into the method type to get the actual method type.
        //
        // N.B., instantiate late-bound regions first so that
        // `instantiate_type_scheme` can normalize associated types that
        // may reference those regions.
        let fn_sig = tcx.fn_sig(def_id);
        let fn_sig = self.replace_bound_vars_with_fresh_vars(
            span,
            infer::FnCall,
            &fn_sig
        ).0;
        let fn_sig = fn_sig.subst(self.tcx, substs);
        let fn_sig = match self.normalize_associated_types_in_as_infer_ok(span, &fn_sig) {
            InferOk { value, obligations: o } => {
                obligations.extend(o);
                value
            }
        };

        // Register obligations for the parameters. This will include the
        // `Self` parameter, which in turn has a bound of the main trait,
        // so this also effectively registers `obligation` as well.  (We
        // used to register `obligation` explicitly, but that resulted in
        // double error messages being reported.)
        //
        // Note that as the method comes from a trait, it should not have
        // any late-bound regions appearing in its bounds.
        let bounds = self.tcx.predicates_of(def_id).instantiate(self.tcx, substs);
        let bounds = match self.normalize_associated_types_in_as_infer_ok(span, &bounds) {
            InferOk { value, obligations: o } => {
                obligations.extend(o);
                value
            }
        };
        assert!(!bounds.has_escaping_bound_vars());

        let cause = traits::ObligationCause::misc(span, self.body_id);
        obligations.extend(traits::predicates_for_generics(cause.clone(),
                                                           self.param_env,
                                                           &bounds));

        // Also add an obligation for the method type being well-formed.
        let method_ty = tcx.mk_fn_ptr(ty::Binder::bind(fn_sig));
        debug!("lookup_in_trait_adjusted: matched method method_ty={:?} obligation={:?}",
               method_ty,
               obligation);
        obligations.push(traits::Obligation::new(cause,
                                                 self.param_env,
                                                 ty::Predicate::WellFormed(method_ty)));

        let callee = MethodCallee {
            def_id,
            substs: trait_ref.substs,
            sig: fn_sig,
        };

        debug!("callee = {:?}", callee);

        Some(InferOk {
            obligations,
            value: callee
        })
    }

    pub fn resolve_ufcs(
        &self,
        span: Span,
        method_name: ast::Ident,
        self_ty: Ty<'tcx>,
        expr_id: hir::HirId
    ) -> Result<(DefKind, DefId), MethodError<'tcx>> {
        debug!(
            "resolve_ufcs: method_name={:?} self_ty={:?} expr_id={:?}",
            method_name, self_ty, expr_id,
        );

        let tcx = self.tcx;

        // Check if we have an enum variant.
        if let ty::Adt(adt_def, _) = self_ty.kind {
            if adt_def.is_enum() {
                let variant_def = adt_def.variants.iter().find(|vd| {
                    tcx.hygienic_eq(method_name, vd.ident, adt_def.did)
                });
                if let Some(variant_def) = variant_def {
                    // Braced variants generate unusable names in value namespace (reserved for
                    // possible future use), so variants resolved as associated items may refer to
                    // them as well. It's ok to use the variant's id as a ctor id since an
                    // error will be reported on any use of such resolution anyway.
                    let ctor_def_id = variant_def.ctor_def_id.unwrap_or(variant_def.def_id);
                    tcx.check_stability(ctor_def_id, Some(expr_id), span);
                    return Ok((
                        DefKind::Ctor(CtorOf::Variant, variant_def.ctor_kind),
                        ctor_def_id,
                    ));
                }
            }
        }

        let pick = self.probe_for_name(span, probe::Mode::Path, method_name, IsSuggestion(false),
                                       self_ty, expr_id, ProbeScope::TraitsInScope)?;
        debug!("resolve_ufcs: pick={:?}", pick);
        for import_id in pick.import_ids {
            let import_def_id = tcx.hir().local_def_id(import_id);
            debug!("resolve_ufcs: used_trait_import: {:?}", import_def_id);
            Lrc::get_mut(&mut self.tables.borrow_mut().used_trait_imports)
                .unwrap().insert(import_def_id);
        }

        let def_kind = pick.item.def_kind();
        debug!("resolve_ufcs: def_kind={:?}, def_id={:?}", def_kind, pick.item.def_id);
        tcx.check_stability(pick.item.def_id, Some(expr_id), span);
        Ok((def_kind, pick.item.def_id))
    }

    /// Finds item with name `item_name` defined in impl/trait `def_id`
    /// and return it, or `None`, if no such item was defined there.
    pub fn associated_item(&self, def_id: DefId, item_name: ast::Ident, ns: Namespace)
                           -> Option<ty::AssocItem> {
        self.tcx.associated_items(def_id).find(|item| {
            Namespace::from(item.kind) == ns &&
            self.tcx.hygienic_eq(item_name, item.ident, def_id)
        })
    }
}
