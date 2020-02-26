use crate::const_eval::const_variant_index;

use rustc::hir;
use rustc::lint;
use rustc::mir::Field;
use rustc::infer::InferCtxt;
use rustc::traits::{ObligationCause, PredicateObligation};
use rustc::ty::{self, Ty, TyCtxt};

use rustc_index::vec::Idx;

use syntax_pos::Span;


use std::cell::Cell;

use super::{FieldPat, Pat, PatCtxt, PatKind};

impl<'a, 'tcx> PatCtxt<'a, 'tcx> {
    /// Converts an evaluated constant to a pattern (if possible).
    /// This means aggregate values (like structs and enums) are converted
    /// to a pattern that matches the value (as if you'd compared via structural equality).
    pub(super) fn const_to_pat(
        &self,
        cv: &'tcx ty::Const<'tcx>,
        id: hir::HirId,
        span: Span,
    ) -> Pat<'tcx> {
        debug!("const_to_pat: cv={:#?} id={:?}", cv, id);
        debug!("const_to_pat: cv.ty={:?} span={:?}", cv.ty, span);

        self.tcx.infer_ctxt().enter(|infcx| {
            let mut convert = ConstToPat::new(self, id, span, infcx);
            convert.to_pat(cv)
        })
    }
}

struct ConstToPat<'a, 'tcx> {
    id: hir::HirId,
    span: Span,
    param_env: ty::ParamEnv<'tcx>,

    // This tracks if we signal some hard error for a given const value, so that
    // we will not subsequently issue an irrelevant lint for the same const
    // value.
    saw_const_match_error: Cell<bool>,

    // inference context used for checking `T: Structural` bounds.
    infcx: InferCtxt<'a, 'tcx>,

    include_lint_checks: bool,
}

impl<'a, 'tcx> ConstToPat<'a, 'tcx> {
    fn new(pat_ctxt: &PatCtxt<'_, 'tcx>,
                      id: hir::HirId,
                      span: Span,
                      infcx: InferCtxt<'a, 'tcx>) -> Self {
        ConstToPat {
            id, span, infcx,
            param_env: pat_ctxt.param_env,
            include_lint_checks: pat_ctxt.include_lint_checks,
            saw_const_match_error: Cell::new(false),
        }
    }

    fn tcx(&self) -> TyCtxt<'tcx> { self.infcx.tcx }

    fn search_for_structural_match_violation(&self,
                                             ty: Ty<'tcx>)
                                             -> Option<ty::NonStructuralMatchTy<'tcx>>
    {
        ty::search_for_structural_match_violation(self.id, self.span, self.tcx(), ty)
    }

    fn type_marked_structural(&self, ty: Ty<'tcx>) -> bool {
        ty::type_marked_structural(self.id, self.span, &self.infcx, ty)
    }

    fn to_pat(&mut self, cv: &'tcx ty::Const<'tcx>) -> Pat<'tcx> {
        // This method is just a wrapper handling a validity check; the heavy lifting is
        // performed by the recursive `recur` method, which is not meant to be
        // invoked except by this method.
        //
        // once indirect_structural_match is a full fledged error, this
        // level of indirection can be eliminated

        let inlined_const_as_pat = self.recur(cv);

        if self.include_lint_checks && !self.saw_const_match_error.get() {
            // If we were able to successfully convert the const to some pat,
            // double-check that all types in the const implement `Structural`.

            let structural = self.search_for_structural_match_violation(cv.ty);
            debug!("search_for_structural_match_violation cv.ty: {:?} returned: {:?}",
                   cv.ty, structural);
            if let Some(non_sm_ty) = structural {
                let adt_def = match non_sm_ty {
                    ty::NonStructuralMatchTy::Adt(adt_def) => adt_def,
                    ty::NonStructuralMatchTy::Param =>
                        bug!("use of constant whose type is a parameter inside a pattern"),
                };
                let path = self.tcx().def_path_str(adt_def.did);
                let msg = format!(
                    "to use a constant of type `{}` in a pattern, \
                     `{}` must be annotated with `#[derive(PartialEq, Eq)]`",
                    path,
                    path,
                );

                // double-check there even *is* a semantic `PartialEq` to dispatch to.
                //
                // (If there isn't, then we can safely issue a hard
                // error, because that's never worked, due to compiler
                // using `PartialEq::eq` in this scenario in the past.)
                //
                // Note: To fix rust-lang/rust#65466, one could lift this check
                // *before* any structural-match checking, and unconditionally error
                // if `PartialEq` is not implemented. However, that breaks stable
                // code at the moment, because types like `for <'a> fn(&'a ())` do
                // not *yet* implement `PartialEq`. So for now we leave this here.
                let ty_is_partial_eq: bool = {
                    let partial_eq_trait_id = self.tcx().lang_items().eq_trait().unwrap();
                    let obligation: PredicateObligation<'_> =
                        self.tcx().predicate_for_trait_def(
                            self.param_env,
                            ObligationCause::misc(self.span, self.id),
                            partial_eq_trait_id,
                            0,
                            cv.ty,
                            &[]);
                    // FIXME: should this call a `predicate_must_hold` variant instead?
                    self.infcx.predicate_may_hold(&obligation)
                };

                if !ty_is_partial_eq {
                    // span_fatal avoids ICE from resolution of non-existent method (rare case).
                    self.tcx().sess.span_fatal(self.span, &msg);
                } else {
                    self.tcx().lint_hir(lint::builtin::INDIRECT_STRUCTURAL_MATCH,
                                        self.id,
                                        self.span,
                                        &msg);
                }
            }
        }

        inlined_const_as_pat
    }

    // Recursive helper for `to_pat`; invoke that (instead of calling this directly).
    fn recur(&self, cv: &'tcx ty::Const<'tcx>) -> Pat<'tcx> {
        let id = self.id;
        let span = self.span;
        let tcx = self.tcx();
        let param_env = self.param_env;

        let adt_subpattern = |i, variant_opt| {
            let field = Field::new(i);
            let val = crate::const_eval::const_field(
                tcx, param_env, variant_opt, field, cv
            );
            self.recur(val)
        };
        let adt_subpatterns = |n, variant_opt| {
            (0..n).map(|i| {
                let field = Field::new(i);
                FieldPat {
                    field,
                    pattern: adt_subpattern(i, variant_opt),
                }
            }).collect::<Vec<_>>()
        };


        let kind = match cv.ty.kind {
            ty::Float(_) => {
                tcx.lint_hir(
                    ::rustc::lint::builtin::ILLEGAL_FLOATING_POINT_LITERAL_PATTERN,
                    id,
                    span,
                    "floating-point types cannot be used in patterns",
                );
                PatKind::Constant {
                    value: cv,
                }
            }
            ty::Adt(adt_def, _) if adt_def.is_union() => {
                // Matching on union fields is unsafe, we can't hide it in constants
                self.saw_const_match_error.set(true);
                tcx.sess.span_err(span, "cannot use unions in constant patterns");
                PatKind::Wild
            }
            // keep old code until future-compat upgraded to errors.
            ty::Adt(adt_def, _) if !self.type_marked_structural(cv.ty) => {
                debug!("adt_def {:?} has !type_marked_structural for cv.ty: {:?}",
                       adt_def, cv.ty);
                let path = tcx.def_path_str(adt_def.did);
                let msg = format!(
                    "to use a constant of type `{}` in a pattern, \
                     `{}` must be annotated with `#[derive(PartialEq, Eq)]`",
                    path,
                    path,
                );
                self.saw_const_match_error.set(true);
                tcx.sess.span_err(span, &msg);
                PatKind::Wild
            }
            // keep old code until future-compat upgraded to errors.
            ty::Ref(_, adt_ty @ ty::TyS { kind: ty::Adt(_, _), .. }, _)
                if !self.type_marked_structural(adt_ty) =>
            {
                let adt_def = if let ty::Adt(adt_def, _) = adt_ty.kind {
                    adt_def
                } else {
                    unreachable!()
                };

                debug!("adt_def {:?} has !type_marked_structural for adt_ty: {:?}",
                       adt_def, adt_ty);

                // HACK(estebank): Side-step ICE #53708, but anything other than erroring here
                // would be wrong. Returnging `PatKind::Wild` is not technically correct.
                let path = tcx.def_path_str(adt_def.did);
                let msg = format!(
                    "to use a constant of type `{}` in a pattern, \
                     `{}` must be annotated with `#[derive(PartialEq, Eq)]`",
                    path,
                    path,
                );
                self.saw_const_match_error.set(true);
                tcx.sess.span_err(span, &msg);
                PatKind::Wild
            }
            ty::Adt(adt_def, substs) if adt_def.is_enum() => {
                let variant_index = const_variant_index(tcx, self.param_env, cv);
                let subpatterns = adt_subpatterns(
                    adt_def.variants[variant_index].fields.len(),
                    Some(variant_index),
                );
                PatKind::Variant {
                    adt_def,
                    substs,
                    variant_index,
                    subpatterns,
                }
            }
            ty::Adt(adt_def, _) => {
                let struct_var = adt_def.non_enum_variant();
                PatKind::Leaf {
                    subpatterns: adt_subpatterns(struct_var.fields.len(), None),
                }
            }
            ty::Tuple(fields) => {
                PatKind::Leaf {
                    subpatterns: adt_subpatterns(fields.len(), None),
                }
            }
            ty::Array(_, n) => {
                PatKind::Array {
                    prefix: (0..n.eval_usize(tcx, self.param_env))
                        .map(|i| adt_subpattern(i as usize, None))
                        .collect(),
                    slice: None,
                    suffix: Vec::new(),
                }
            }
            _ => {
                PatKind::Constant {
                    value: cv,
                }
            }
        };

        Pat {
            span,
            ty: cv.ty,
            kind: Box::new(kind),
        }
    }
}
