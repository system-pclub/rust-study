use crate::borrow_check::location::{LocationIndex, LocationTable};
use crate::dataflow::indexes::MovePathIndex;
use crate::dataflow::move_paths::{LookupResult, MoveData};
use crate::util::liveness::{categorize, DefUse};
use rustc::mir::visit::{MutatingUseContext, PlaceContext, Visitor};
use rustc::mir::{Body, Local, Location, Place};
use rustc::ty::subst::GenericArg;
use rustc::ty::Ty;

use super::TypeChecker;

type VarPointRelations = Vec<(Local, LocationIndex)>;
type MovePathPointRelations = Vec<(MovePathIndex, LocationIndex)>;

struct UseFactsExtractor<'me> {
    var_defined: &'me mut VarPointRelations,
    var_used: &'me mut VarPointRelations,
    location_table: &'me LocationTable,
    var_drop_used: &'me mut Vec<(Local, Location)>,
    move_data: &'me MoveData<'me>,
    path_accessed_at: &'me mut MovePathPointRelations,
}

// A Visitor to walk through the MIR and extract point-wise facts
impl UseFactsExtractor<'_> {
    fn location_to_index(&self, location: Location) -> LocationIndex {
        self.location_table.mid_index(location)
    }

    fn insert_def(&mut self, local: Local, location: Location) {
        debug!("LivenessFactsExtractor::insert_def()");
        self.var_defined.push((local, self.location_to_index(location)));
    }

    fn insert_use(&mut self, local: Local, location: Location) {
        debug!("LivenessFactsExtractor::insert_use()");
        self.var_used.push((local, self.location_to_index(location)));
    }

    fn insert_drop_use(&mut self, local: Local, location: Location) {
        debug!("LivenessFactsExtractor::insert_drop_use()");
        self.var_drop_used.push((local, location));
    }

    fn insert_path_access(&mut self, path: MovePathIndex, location: Location) {
        debug!("LivenessFactsExtractor::insert_path_access({:?}, {:?})", path, location);
        self.path_accessed_at.push((path, self.location_to_index(location)));
    }

    fn place_to_mpi(&self, place: &Place<'_>) -> Option<MovePathIndex> {
        match self.move_data.rev_lookup.find(place.as_ref()) {
            LookupResult::Exact(mpi) => Some(mpi),
            LookupResult::Parent(mmpi) => mmpi,
        }
    }
}

impl Visitor<'tcx> for UseFactsExtractor<'_> {
    fn visit_local(&mut self, &local: &Local, context: PlaceContext, location: Location) {
        match categorize(context) {
            Some(DefUse::Def) => self.insert_def(local, location),
            Some(DefUse::Use) => self.insert_use(local, location),
            Some(DefUse::Drop) => self.insert_drop_use(local, location),
            _ => (),
        }
    }

    fn visit_place(&mut self, place: &Place<'tcx>, context: PlaceContext, location: Location) {
        self.super_place(place, context, location);
        match context {
            PlaceContext::NonMutatingUse(_) => {
                if let Some(mpi) = self.place_to_mpi(place) {
                    self.insert_path_access(mpi, location);
                }
            }

            PlaceContext::MutatingUse(MutatingUseContext::Borrow) => {
                if let Some(mpi) = self.place_to_mpi(place) {
                    self.insert_path_access(mpi, location);
                }
            }
            _ => (),
        }
    }
}

fn add_var_uses_regions(typeck: &mut TypeChecker<'_, 'tcx>, local: Local, ty: Ty<'tcx>) {
    debug!("add_regions(local={:?}, type={:?})", local, ty);
    typeck.tcx().for_each_free_region(&ty, |region| {
        let region_vid = typeck.borrowck_context.universal_regions.to_region_vid(region);
        debug!("add_regions for region {:?}", region_vid);
        if let Some(facts) = typeck.borrowck_context.all_facts {
            facts.var_uses_region.push((local, region_vid));
        }
    });
}

pub(super) fn populate_access_facts(
    typeck: &mut TypeChecker<'_, 'tcx>,
    body: &Body<'tcx>,
    location_table: &LocationTable,
    move_data: &MoveData<'_>,
    drop_used: &mut Vec<(Local, Location)>,
) {
    debug!("populate_var_liveness_facts()");

    if let Some(facts) = typeck.borrowck_context.all_facts.as_mut() {
        UseFactsExtractor {
            var_defined: &mut facts.var_defined,
            var_used: &mut facts.var_used,
            var_drop_used: drop_used,
            path_accessed_at: &mut facts.path_accessed_at,
            location_table,
            move_data,
        }
        .visit_body(body);

        facts.var_drop_used.extend(drop_used.iter().map(|&(local, location)| {
            (local, location_table.mid_index(location))
        }));
    }

    for (local, local_decl) in body.local_decls.iter_enumerated() {
        add_var_uses_regions(typeck, local, local_decl.ty);
    }
}

// For every potentially drop()-touched region `region` in `local`'s type
// (`kind`), emit a Polonius `var_drops_region(local, region)` fact.
pub(super) fn add_var_drops_regions(
    typeck: &mut TypeChecker<'_, 'tcx>,
    local: Local,
    kind: &GenericArg<'tcx>,
) {
    debug!("add_var_drops_region(local={:?}, kind={:?}", local, kind);
    let tcx = typeck.tcx();

    tcx.for_each_free_region(kind, |drop_live_region| {
        let region_vid = typeck.borrowck_context.universal_regions.to_region_vid(drop_live_region);
        if let Some(facts) = typeck.borrowck_context.all_facts.as_mut() {
            facts.var_drops_region.push((local, region_vid));
        };
    });
}
