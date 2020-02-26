//! This module contains code to equate the input/output types appearing
//! in the MIR with the expected input/output types from the function
//! signature. This requires a bit of processing, as the expected types
//! are supplied to us before normalization and may contain opaque
//! `impl Trait` instances. In contrast, the input/output types found in
//! the MIR (specifically, in the special local variables for the
//! `RETURN_PLACE` the MIR arguments) are always fully normalized (and
//! contain revealed `impl Trait` values).

use crate::borrow_check::nll::universal_regions::UniversalRegions;
use rustc::infer::LateBoundRegionConversionTime;
use rustc::mir::*;
use rustc::ty::Ty;

use rustc_index::vec::Idx;
use syntax_pos::Span;

use super::{Locations, TypeChecker};

impl<'a, 'tcx> TypeChecker<'a, 'tcx> {
    pub(super) fn equate_inputs_and_outputs(
        &mut self,
        body: &Body<'tcx>,
        universal_regions: &UniversalRegions<'tcx>,
        normalized_inputs_and_output: &[Ty<'tcx>],
    ) {
        let (&normalized_output_ty, normalized_input_tys) =
            normalized_inputs_and_output.split_last().unwrap();

        // If the user explicitly annotated the input types, extract
        // those.
        //
        // e.g., `|x: FxHashMap<_, &'static u32>| ...`
        let user_provided_sig;
        if !self.tcx().is_closure(self.mir_def_id) {
            user_provided_sig = None;
        } else {
            let typeck_tables = self.tcx().typeck_tables_of(self.mir_def_id);
            user_provided_sig = match typeck_tables.user_provided_sigs.get(&self.mir_def_id) {
                None => None,
                Some(user_provided_poly_sig) => {
                    // Instantiate the canonicalized variables from
                    // user-provided signature (e.g., the `_` in the code
                    // above) with fresh variables.
                    let (poly_sig, _) = self.infcx.instantiate_canonical_with_fresh_inference_vars(
                        body.span,
                        &user_provided_poly_sig,
                    );

                    // Replace the bound items in the fn sig with fresh
                    // variables, so that they represent the view from
                    // "inside" the closure.
                    Some(
                        self.infcx
                            .replace_bound_vars_with_fresh_vars(
                                body.span,
                                LateBoundRegionConversionTime::FnCall,
                                &poly_sig,
                            )
                            .0,
                    )
                }
            }
        };

        // Equate expected input tys with those in the MIR.
        for (&normalized_input_ty, argument_index) in normalized_input_tys.iter().zip(0..) {
            // In MIR, argument N is stored in local N+1.
            let local = Local::new(argument_index + 1);

            debug!(
                "equate_inputs_and_outputs: normalized_input_ty = {:?}",
                normalized_input_ty
            );

            let mir_input_ty = body.local_decls[local].ty;
            let mir_input_span = body.local_decls[local].source_info.span;
            self.equate_normalized_input_or_output(
                normalized_input_ty,
                mir_input_ty,
                mir_input_span,
            );
        }

        if let Some(user_provided_sig) = user_provided_sig {
            for (&user_provided_input_ty, argument_index) in
                user_provided_sig.inputs().iter().zip(0..)
            {
                // In MIR, closures begin an implicit `self`, so
                // argument N is stored in local N+2.
                let local = Local::new(argument_index + 2);
                let mir_input_ty = body.local_decls[local].ty;
                let mir_input_span = body.local_decls[local].source_info.span;

                // If the user explicitly annotated the input types, enforce those.
                let user_provided_input_ty =
                    self.normalize(user_provided_input_ty, Locations::All(mir_input_span));
                self.equate_normalized_input_or_output(
                    user_provided_input_ty,
                    mir_input_ty,
                    mir_input_span,
                );
            }
        }

        assert!(
            body.yield_ty.is_some() && universal_regions.yield_ty.is_some()
                || body.yield_ty.is_none() && universal_regions.yield_ty.is_none()
        );
        if let Some(mir_yield_ty) = body.yield_ty {
            let ur_yield_ty = universal_regions.yield_ty.unwrap();
            let yield_span = body.local_decls[RETURN_PLACE].source_info.span;
            self.equate_normalized_input_or_output(ur_yield_ty, mir_yield_ty, yield_span);
        }

        // Return types are a bit more complex. They may contain opaque `impl Trait` types.
        let mir_output_ty = body.local_decls[RETURN_PLACE].ty;
        let output_span = body.local_decls[RETURN_PLACE].source_info.span;
        if let Err(terr) = self.eq_opaque_type_and_type(
            mir_output_ty,
            normalized_output_ty,
            self.mir_def_id,
            Locations::All(output_span),
            ConstraintCategory::BoringNoLocation,
        ) {
            span_mirbug!(
                self,
                Location::START,
                "equate_inputs_and_outputs: `{:?}=={:?}` failed with `{:?}`",
                normalized_output_ty,
                mir_output_ty,
                terr
            );
        };

        // If the user explicitly annotated the output types, enforce those.
        if let Some(user_provided_sig) = user_provided_sig {
            let user_provided_output_ty = user_provided_sig.output();
            let user_provided_output_ty =
                self.normalize(user_provided_output_ty, Locations::All(output_span));
            self.equate_normalized_input_or_output(
                user_provided_output_ty,
                mir_output_ty,
                output_span,
            );
        }
    }

    fn equate_normalized_input_or_output(&mut self, a: Ty<'tcx>, b: Ty<'tcx>, span: Span) {
        debug!("equate_normalized_input_or_output(a={:?}, b={:?})", a, b);

        if let Err(terr) = self.eq_types(
            a,
            b,
            Locations::All(span),
            ConstraintCategory::BoringNoLocation,
        ) {
            span_mirbug!(
                self,
                Location::START,
                "equate_normalized_input_or_output: `{:?}=={:?}` failed with `{:?}`",
                a,
                b,
                terr
            );
        }
    }
}
