use rustc::hir;
use rustc::traits;
use rustc::ty::ToPredicate;
use rustc::ty::subst::Subst;
use rustc::infer::InferOk;
use rustc::hir::def_id::LOCAL_CRATE;
use syntax_pos::DUMMY_SP;

use super::*;

pub struct BlanketImplFinder<'a, 'tcx> {
    pub cx: &'a core::DocContext<'tcx>,
}

impl<'a, 'tcx> BlanketImplFinder<'a, 'tcx> {
    pub fn new(cx: &'a core::DocContext<'tcx>) -> Self {
        BlanketImplFinder { cx }
    }

    // FIXME(eddyb) figure out a better way to pass information about
    // parametrization of `ty` than `param_env_def_id`.
    pub fn get_blanket_impls(
        &self,
        ty: Ty<'tcx>,
        param_env_def_id: DefId,
    ) -> Vec<Item> {
        let param_env = self.cx.tcx.param_env(param_env_def_id);

        debug!("get_blanket_impls({:?})", ty);
        let mut impls = Vec::new();
        for &trait_def_id in self.cx.tcx.all_traits(LOCAL_CRATE).iter() {
            if !self.cx.renderinfo.borrow().access_levels.is_public(trait_def_id) ||
               self.cx.generated_synthetics
                      .borrow_mut()
                      .get(&(ty, trait_def_id))
                      .is_some() {
                continue
            }
            self.cx.tcx.for_each_relevant_impl(trait_def_id, ty, |impl_def_id| {
                debug!("get_blanket_impls: Considering impl for trait '{:?}' {:?}",
                        trait_def_id, impl_def_id);
                let trait_ref = self.cx.tcx.impl_trait_ref(impl_def_id).unwrap();
                let may_apply = self.cx.tcx.infer_ctxt().enter(|infcx| {
                    match trait_ref.self_ty().kind {
                        ty::Param(_) => {},
                        _ => return false,
                    }

                    let substs = infcx.fresh_substs_for_item(DUMMY_SP, param_env_def_id);
                    let ty = ty.subst(infcx.tcx, substs);
                    let param_env = param_env.subst(infcx.tcx, substs);

                    let impl_substs = infcx.fresh_substs_for_item(DUMMY_SP, impl_def_id);
                    let trait_ref = trait_ref.subst(infcx.tcx, impl_substs);

                    // Require the type the impl is implemented on to match
                    // our type, and ignore the impl if there was a mismatch.
                    let cause = traits::ObligationCause::dummy();
                    let eq_result = infcx.at(&cause, param_env)
                                         .eq(trait_ref.self_ty(), ty);
                    if let Ok(InferOk { value: (), obligations }) = eq_result {
                        // FIXME(eddyb) ignoring `obligations` might cause false positives.
                        drop(obligations);

                        debug!(
                            "invoking predicate_may_hold: param_env={:?}, trait_ref={:?}, ty={:?}",
                             param_env, trait_ref, ty
                        );
                        match infcx.evaluate_obligation(
                            &traits::Obligation::new(
                                cause,
                                param_env,
                                trait_ref.to_predicate(),
                            ),
                        ) {
                            Ok(eval_result) => eval_result.may_apply(),
                            Err(traits::OverflowError) => true, // overflow doesn't mean yes *or* no
                        }
                    } else {
                        false
                    }
                });
                debug!("get_blanket_impls: found applicable impl: {}\
                        for trait_ref={:?}, ty={:?}",
                        may_apply, trait_ref, ty);
                if !may_apply {
                    return;
                }

                self.cx.generated_synthetics.borrow_mut()
                                            .insert((ty, trait_def_id));
                let provided_trait_methods =
                    self.cx.tcx.provided_trait_methods(trait_def_id)
                                .into_iter()
                                .map(|meth| meth.ident.to_string())
                                .collect();

                impls.push(Item {
                    source: self.cx.tcx.def_span(impl_def_id).clean(self.cx),
                    name: None,
                    attrs: Default::default(),
                    visibility: Inherited,
                    def_id: self.cx.next_def_id(impl_def_id.krate),
                    stability: None,
                    deprecation: None,
                    inner: ImplItem(Impl {
                        unsafety: hir::Unsafety::Normal,
                        generics: (
                            self.cx.tcx.generics_of(impl_def_id),
                            self.cx.tcx.explicit_predicates_of(impl_def_id),
                        ).clean(self.cx),
                        provided_trait_methods,
                        // FIXME(eddyb) compute both `trait_` and `for_` from
                        // the post-inference `trait_ref`, as it's more accurate.
                        trait_: Some(trait_ref.clean(self.cx).get_trait_type().unwrap()),
                        for_: ty.clean(self.cx),
                        items: self.cx.tcx.associated_items(impl_def_id)
                                        .collect::<Vec<_>>()
                                        .clean(self.cx),
                        polarity: None,
                        synthetic: false,
                        blanket_impl: Some(trait_ref.self_ty().clean(self.cx)),
                    }),
                });
            });
        }
        impls
    }
}
