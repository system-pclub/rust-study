// This pass converts move out from array by Subslice and
// ConstIndex{.., from_end: true} to ConstIndex move out(s) from begin
// of array. It allows detect error by mir borrowck and elaborate
// drops for array without additional work.
//
// Example:
//
// let a = [ box 1,box 2, box 3];
// if b {
//  let [_a.., _] = a;
// } else {
//  let [.., _b] = a;
// }
//
//  mir statement _10 = move _2[:-1]; replaced by:
//  StorageLive(_12);
//  _12 = move _2[0 of 3];
//  StorageLive(_13);
//  _13 = move _2[1 of 3];
//  _10 = [move _12, move _13]
//  StorageDead(_12);
//  StorageDead(_13);
//
//  and mir statement _11 = move _2[-1 of 1]; replaced by:
//  _11 = move _2[2 of 3];
//
// FIXME: integrate this transformation to the mir build

use rustc::ty;
use rustc::ty::TyCtxt;
use rustc::mir::*;
use rustc::mir::visit::{Visitor, PlaceContext, NonUseContext};
use rustc_index::vec::{IndexVec};
use crate::transform::{MirPass, MirSource};
use crate::util::patch::MirPatch;

pub struct UniformArrayMoveOut;

impl<'tcx> MirPass<'tcx> for UniformArrayMoveOut {
    fn run_pass(&self, tcx: TyCtxt<'tcx>, src: MirSource<'tcx>, body: &mut Body<'tcx>) {
        let mut patch = MirPatch::new(body);
        let param_env = tcx.param_env(src.def_id());
        {
            let mut visitor = UniformArrayMoveOutVisitor{body, patch: &mut patch, tcx, param_env};
            visitor.visit_body(body);
        }
        patch.apply(body);
    }
}

struct UniformArrayMoveOutVisitor<'a, 'tcx> {
    body: &'a Body<'tcx>,
    patch: &'a mut MirPatch<'tcx>,
    tcx: TyCtxt<'tcx>,
    param_env: ty::ParamEnv<'tcx>,
}

impl<'a, 'tcx> Visitor<'tcx> for UniformArrayMoveOutVisitor<'a, 'tcx> {
    fn visit_assign(&mut self,
                    dst_place: &Place<'tcx>,
                    rvalue: &Rvalue<'tcx>,
                    location: Location) {
        if let Rvalue::Use(Operand::Move(ref src_place)) = rvalue {
            if let &[ref proj_base @ .., elem] = src_place.projection.as_ref() {
                if let ProjectionElem::ConstantIndex{offset: _,
                                                     min_length: _,
                                                     from_end: false} = elem {
                    // no need to transformation
                } else {
                    let place_ty =
                        Place::ty_from(&src_place.base, proj_base, self.body, self.tcx).ty;
                    if let ty::Array(item_ty, const_size) = place_ty.kind {
                        if let Some(size) = const_size.try_eval_usize(self.tcx, self.param_env) {
                            assert!(size <= u32::max_value() as u64,
                                    "uniform array move out doesn't supported
                                     for array bigger then u32");
                            self.uniform(
                                location,
                                dst_place,
                                &src_place.base,
                                &src_place.projection,
                                item_ty,
                                size as u32,
                            );
                        }
                    }

                }
            }
        }
        self.super_assign(dst_place, rvalue, location)
    }
}

impl<'a, 'tcx> UniformArrayMoveOutVisitor<'a, 'tcx> {
    fn uniform(&mut self,
               location: Location,
               dst_place: &Place<'tcx>,
               base: &PlaceBase<'tcx>,
               proj: &[PlaceElem<'tcx>],
               item_ty: &'tcx ty::TyS<'tcx>,
               size: u32) {
        if let [proj_base @ .., elem] = proj {
            match elem {
                // uniforms statements like_10 = move _2[:-1];
                ProjectionElem::Subslice{from, to} => {
                    self.patch.make_nop(location);
                    let temps : Vec<_> = (*from..(size-*to)).map(|i| {
                        let temp =
                            self.patch.new_temp(item_ty, self.body.source_info(location).span);
                        self.patch.add_statement(location, StatementKind::StorageLive(temp));

                        let mut projection = proj_base.to_vec();
                        projection.push(ProjectionElem::ConstantIndex {
                            offset: i,
                            min_length: size,
                            from_end: false,
                        });
                        self.patch.add_assign(
                            location,
                            Place::from(temp),
                            Rvalue::Use(Operand::Move(Place {
                                base: base.clone(),
                                projection: self.tcx.intern_place_elems(&projection),
                            })),
                        );
                        temp
                    }).collect();
                    self.patch.add_assign(
                        location,
                        dst_place.clone(),
                        Rvalue::Aggregate(
                            box AggregateKind::Array(item_ty),
                            temps.iter().map(
                                |x| Operand::Move(Place::from(*x))
                            ).collect()
                        )
                    );
                    for temp in temps {
                        self.patch.add_statement(location, StatementKind::StorageDead(temp));
                    }
                }
                // uniforms statements like _11 = move _2[-1 of 1];
                ProjectionElem::ConstantIndex{offset, min_length: _, from_end: true} => {
                    self.patch.make_nop(location);

                    let mut projection = proj_base.to_vec();
                    projection.push(ProjectionElem::ConstantIndex {
                        offset: size - offset,
                        min_length: size,
                        from_end: false,
                    });
                    self.patch.add_assign(
                        location,
                        dst_place.clone(),
                        Rvalue::Use(Operand::Move(Place {
                            base: base.clone(),
                            projection: self.tcx.intern_place_elems(&projection),
                        })),
                    );
                }
                _ => {}
            }
        }
    }
}

// Restore Subslice move out after analysis
// Example:
//
//  next statements:
//   StorageLive(_12);
//   _12 = move _2[0 of 3];
//   StorageLive(_13);
//   _13 = move _2[1 of 3];
//   _10 = [move _12, move _13]
//   StorageDead(_12);
//   StorageDead(_13);
//
// replaced by _10 = move _2[:-1];

pub struct RestoreSubsliceArrayMoveOut<'tcx> {
    tcx: TyCtxt<'tcx>
}

impl<'tcx> MirPass<'tcx> for RestoreSubsliceArrayMoveOut<'tcx> {
    fn run_pass(&self, tcx: TyCtxt<'tcx>, src: MirSource<'tcx>, body: &mut Body<'tcx>) {
        let mut patch = MirPatch::new(body);
        let param_env = tcx.param_env(src.def_id());
        {
            let mut visitor = RestoreDataCollector {
                locals_use: IndexVec::from_elem(LocalUse::new(), &body.local_decls),
                candidates: vec![],
            };
            visitor.visit_body(body);

            for candidate in &visitor.candidates {
                let statement = &body[candidate.block].statements[candidate.statement_index];
                if let StatementKind::Assign(box(ref dst_place, ref rval)) = statement.kind {
                    if let Rvalue::Aggregate(box AggregateKind::Array(_), ref items) = *rval {
                        let items : Vec<_> = items.iter().map(|item| {
                            if let Operand::Move(place) = item {
                                if let Some(local) = place.as_local() {
                                    let local_use = &visitor.locals_use[local];
                                    let opt_index_and_place =
                                        Self::try_get_item_source(local_use, body);
                                    // each local should be used twice:
                                    //  in assign and in aggregate statements
                                    if local_use.use_count == 2 && opt_index_and_place.is_some() {
                                        let (index, src_place) = opt_index_and_place.unwrap();
                                        return Some((local_use, index, src_place));
                                    }
                                }
                            }
                            None
                        }).collect();

                        let opt_src_place = items.first().and_then(|x| *x).map(|x| x.2);
                        let opt_size = opt_src_place.and_then(|src_place| {
                            let src_ty =
                                Place::ty_from(src_place.base, src_place.projection, body, tcx).ty;
                            if let ty::Array(_, ref size_o) = src_ty.kind {
                                size_o.try_eval_usize(tcx, param_env)
                            } else {
                                None
                            }
                        });
                        let restore_subslice = RestoreSubsliceArrayMoveOut { tcx };
                        restore_subslice
                            .check_and_patch(*candidate, &items, opt_size, &mut patch, dst_place);
                    }
                }
            }
        }
        patch.apply(body);
    }
}

impl RestoreSubsliceArrayMoveOut<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        RestoreSubsliceArrayMoveOut { tcx }
    }

    // Checks that source has size, all locals are inited from same source place and
    // indices is an integer interval. If all checks pass do the replacent.
    // items are Vec<Option<LocalUse, index in source array, source place for init local>>
    fn check_and_patch(&self,
                       candidate: Location,
                       items: &[Option<(&LocalUse, u32, PlaceRef<'_, 'tcx>)>],
                       opt_size: Option<u64>,
                       patch: &mut MirPatch<'tcx>,
                       dst_place: &Place<'tcx>) {
        let opt_src_place = items.first().and_then(|x| *x).map(|x| x.2);

        if opt_size.is_some() && items.iter().all(
            |l| l.is_some() && l.unwrap().2 == opt_src_place.unwrap()) {
            let src_place = opt_src_place.unwrap();

            let indices: Vec<_> = items.iter().map(|x| x.unwrap().1).collect();
            for i in 1..indices.len() {
                if indices[i - 1] + 1 != indices[i] {
                    return;
                }
            }

            let min = *indices.first().unwrap();
            let max = *indices.last().unwrap();

            for item in items {
                let locals_use = item.unwrap().0;
                patch.make_nop(locals_use.alive.unwrap());
                patch.make_nop(locals_use.dead.unwrap());
                patch.make_nop(locals_use.first_use.unwrap());
            }
            patch.make_nop(candidate);
            let size = opt_size.unwrap() as u32;

            let mut projection = src_place.projection.to_vec();
            projection.push(ProjectionElem::Subslice { from: min, to: size - max - 1 });
            patch.add_assign(
                candidate,
                dst_place.clone(),
                Rvalue::Use(Operand::Move(Place {
                    base: src_place.base.clone(),
                    projection: self.tcx.intern_place_elems(&projection),
                })),
            );
        }
    }

    fn try_get_item_source<'a>(local_use: &LocalUse,
                               body: &'a Body<'tcx>) -> Option<(u32, PlaceRef<'a, 'tcx>)> {
        if let Some(location) = local_use.first_use {
            let block = &body[location.block];
            if block.statements.len() > location.statement_index {
                let statement = &block.statements[location.statement_index];
                if let StatementKind::Assign(
                    box(place, Rvalue::Use(Operand::Move(src_place)))
                ) = &statement.kind {
                    if let (Some(_), PlaceRef {
                        base: _,
                        projection: &[.., ProjectionElem::ConstantIndex {
                            offset, min_length: _, from_end: false
                        }],
                    }) = (place.as_local(), src_place.as_ref()) {
                        if let StatementKind::Assign(
                            box(_, Rvalue::Use(Operand::Move(place)))
                        ) = &statement.kind {
                            if let PlaceRef {
                                base,
                                projection: &[ref proj_base @ .., _],
                            } = place.as_ref() {
                                return Some((offset, PlaceRef {
                                    base,
                                    projection: proj_base,
                                }))
                            }
                        }
                    }
                }
            }
        }
        None
    }
}

#[derive(Copy, Clone, Debug)]
struct LocalUse {
    alive: Option<Location>,
    dead: Option<Location>,
    use_count: u32,
    first_use: Option<Location>,
}

impl LocalUse {
    pub fn new() -> Self {
        LocalUse{alive: None, dead: None, use_count: 0, first_use: None}
    }
}

struct RestoreDataCollector {
    locals_use: IndexVec<Local, LocalUse>,
    candidates: Vec<Location>,
}

impl<'tcx> Visitor<'tcx> for RestoreDataCollector {
    fn visit_assign(&mut self,
                    place: &Place<'tcx>,
                    rvalue: &Rvalue<'tcx>,
                    location: Location) {
        if let Rvalue::Aggregate(box AggregateKind::Array(_), _) = *rvalue {
            self.candidates.push(location);
        }
        self.super_assign(place, rvalue, location)
    }

    fn visit_local(&mut self,
                   local: &Local,
                   context: PlaceContext,
                   location: Location) {
        let local_use = &mut self.locals_use[*local];
        match context {
            PlaceContext::NonUse(NonUseContext::StorageLive) => local_use.alive = Some(location),
            PlaceContext::NonUse(NonUseContext::StorageDead) => local_use.dead = Some(location),
            _ => {
                local_use.use_count += 1;
                if local_use.first_use.is_none() {
                    local_use.first_use = Some(location);
                }
            }
        }
    }
}
