//! This pass erases all early-bound regions from the types occurring in the MIR.
//! We want to do this once just before codegen, so codegen does not have to take
//! care erasing regions all over the place.
//! N.B., we do _not_ erase regions of statements that are relevant for
//! "types-as-contracts"-validation, namely, `AcquireValid` and `ReleaseValid`.

use rustc::ty::subst::SubstsRef;
use rustc::ty::{self, Ty, TyCtxt};
use rustc::mir::*;
use rustc::mir::visit::{MutVisitor, TyContext};
use crate::transform::{MirPass, MirSource};

struct EraseRegionsVisitor<'tcx> {
    tcx: TyCtxt<'tcx>,
}

impl EraseRegionsVisitor<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        EraseRegionsVisitor {
            tcx,
        }
    }
}

impl MutVisitor<'tcx> for EraseRegionsVisitor<'tcx> {
    fn tcx(&self) -> TyCtxt<'tcx> {
        self.tcx
    }

    fn visit_ty(&mut self, ty: &mut Ty<'tcx>, _: TyContext) {
        *ty = self.tcx.erase_regions(ty);
    }

    fn visit_region(&mut self, region: &mut ty::Region<'tcx>, _: Location) {
        *region = self.tcx.lifetimes.re_erased;
    }

    fn visit_const(&mut self, constant: &mut &'tcx ty::Const<'tcx>, _: Location) {
        *constant = self.tcx.erase_regions(constant);
    }

    fn visit_substs(&mut self, substs: &mut SubstsRef<'tcx>, _: Location) {
        *substs = self.tcx.erase_regions(substs);
    }

    fn process_projection_elem(
        &mut self,
        elem: &PlaceElem<'tcx>,
    ) -> Option<PlaceElem<'tcx>> {
        if let PlaceElem::Field(field, ty) = elem {
            let new_ty = self.tcx.erase_regions(ty);

            if new_ty != *ty {
                return Some(PlaceElem::Field(*field, new_ty));
            }
        }

        None
    }
}

pub struct EraseRegions;

impl<'tcx> MirPass<'tcx> for EraseRegions {
    fn run_pass(&self, tcx: TyCtxt<'tcx>, _: MirSource<'tcx>, body: &mut Body<'tcx>) {
        EraseRegionsVisitor::new(tcx).visit_body(body);
    }
}
