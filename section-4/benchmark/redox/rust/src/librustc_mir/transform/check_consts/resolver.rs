//! Propagate `Qualif`s between locals and query the results.
//!
//! This contains the dataflow analysis used to track `Qualif`s on complex control-flow graphs.

use rustc::mir::visit::Visitor;
use rustc::mir::{self, BasicBlock, Local, Location};
use rustc_index::bit_set::BitSet;

use std::marker::PhantomData;

use crate::dataflow::{self as old_dataflow, generic as dataflow};
use super::{Item, Qualif};

/// A `Visitor` that propagates qualifs between locals. This defines the transfer function of
/// `FlowSensitiveAnalysis`.
///
/// This transfer does nothing when encountering an indirect assignment. Consumers should rely on
/// the `IndirectlyMutableLocals` dataflow pass to see if a `Local` may have become qualified via
/// an indirect assignment or function call.
struct TransferFunction<'a, 'mir, 'tcx, Q> {
    item: &'a Item<'mir, 'tcx>,
    qualifs_per_local: &'a mut BitSet<Local>,

    _qualif: PhantomData<Q>,
}

impl<Q> TransferFunction<'a, 'mir, 'tcx, Q>
where
    Q: Qualif,
{
    fn new(
        item: &'a Item<'mir, 'tcx>,
        qualifs_per_local: &'a mut BitSet<Local>,
    ) -> Self {
        TransferFunction {
            item,
            qualifs_per_local,
            _qualif: PhantomData,
        }
    }

    fn initialize_state(&mut self) {
        self.qualifs_per_local.clear();

        for arg in self.item.body.args_iter() {
            let arg_ty = self.item.body.local_decls[arg].ty;
            if Q::in_any_value_of_ty(self.item, arg_ty) {
                self.qualifs_per_local.insert(arg);
            }
        }
    }

    fn assign_qualif_direct(&mut self, place: &mir::Place<'tcx>, value: bool) {
        debug_assert!(!place.is_indirect());

        match (value, place.as_ref()) {
            (true, mir::PlaceRef { base: &mir::PlaceBase::Local(local), .. }) => {
                self.qualifs_per_local.insert(local);
            }

            // For now, we do not clear the qualif if a local is overwritten in full by
            // an unqualified rvalue (e.g. `y = 5`). This is to be consistent
            // with aggregates where we overwrite all fields with assignments, which would not
            // get this feature.
            (false, mir::PlaceRef { base: &mir::PlaceBase::Local(_local), projection: &[] }) => {
                // self.qualifs_per_local.remove(*local);
            }

            _ => {}
        }
    }

    fn apply_call_return_effect(
        &mut self,
        _block: BasicBlock,
        func: &mir::Operand<'tcx>,
        args: &[mir::Operand<'tcx>],
        return_place: &mir::Place<'tcx>,
    ) {
        let return_ty = return_place.ty(self.item.body, self.item.tcx).ty;
        let qualif = Q::in_call(
            self.item,
            &|l| self.qualifs_per_local.contains(l),
            func,
            args,
            return_ty,
        );
        if !return_place.is_indirect() {
            self.assign_qualif_direct(return_place, qualif);
        }
    }
}

impl<Q> Visitor<'tcx> for TransferFunction<'_, '_, 'tcx, Q>
where
    Q: Qualif,
{
    fn visit_operand(&mut self, operand: &mir::Operand<'tcx>, location: Location) {
        self.super_operand(operand, location);

        if !Q::IS_CLEARED_ON_MOVE {
            return;
        }

        // If a local with no projections is moved from (e.g. `x` in `y = x`), record that
        // it no longer needs to be dropped.
        if let mir::Operand::Move(place) = operand {
            if let Some(local) = place.as_local() {
                self.qualifs_per_local.remove(local);
            }
        }
    }

    fn visit_assign(
        &mut self,
        place: &mir::Place<'tcx>,
        rvalue: &mir::Rvalue<'tcx>,
        location: Location,
    ) {
        let qualif = Q::in_rvalue(self.item, &|l| self.qualifs_per_local.contains(l), rvalue);
        if !place.is_indirect() {
            self.assign_qualif_direct(place, qualif);
        }

        // We need to assign qualifs to the left-hand side before visiting `rvalue` since
        // qualifs can be cleared on move.
        self.super_assign(place, rvalue, location);
    }

    fn visit_terminator_kind(&mut self, kind: &mir::TerminatorKind<'tcx>, location: Location) {
        // The effect of assignment to the return place in `TerminatorKind::Call` is not applied
        // here; that occurs in `apply_call_return_effect`.

        if let mir::TerminatorKind::DropAndReplace { value, location: dest, .. } = kind {
            let qualif = Q::in_operand(self.item, &|l| self.qualifs_per_local.contains(l), value);
            if !dest.is_indirect() {
                self.assign_qualif_direct(dest, qualif);
            }
        }

        // We need to assign qualifs to the dropped location before visiting the operand that
        // replaces it since qualifs can be cleared on move.
        self.super_terminator_kind(kind, location);
    }
}

/// The dataflow analysis used to propagate qualifs on arbitrary CFGs.
pub(super) struct FlowSensitiveAnalysis<'a, 'mir, 'tcx, Q> {
    item: &'a Item<'mir, 'tcx>,
    _qualif: PhantomData<Q>,
}

impl<'a, 'mir, 'tcx, Q> FlowSensitiveAnalysis<'a, 'mir, 'tcx, Q>
where
    Q: Qualif,
{
    pub(super) fn new(_: Q, item: &'a Item<'mir, 'tcx>) -> Self {
        FlowSensitiveAnalysis {
            item,
            _qualif: PhantomData,
        }
    }

    fn transfer_function(
        &self,
        state: &'a mut BitSet<Local>,
    ) -> TransferFunction<'a, 'mir, 'tcx, Q> {
        TransferFunction::<Q>::new(self.item, state)
    }
}

impl<Q> old_dataflow::BottomValue for FlowSensitiveAnalysis<'_, '_, '_, Q> {
    const BOTTOM_VALUE: bool = false;
}

impl<Q> dataflow::Analysis<'tcx> for FlowSensitiveAnalysis<'_, '_, 'tcx, Q>
where
    Q: Qualif,
{
    type Idx = Local;

    const NAME: &'static str = Q::ANALYSIS_NAME;

    fn bits_per_block(&self, body: &mir::Body<'tcx>) -> usize {
        body.local_decls.len()
    }

    fn initialize_start_block(&self, _body: &mir::Body<'tcx>, state: &mut BitSet<Self::Idx>) {
        self.transfer_function(state).initialize_state();
    }

    fn apply_statement_effect(
        &self,
        state: &mut BitSet<Self::Idx>,
        statement: &mir::Statement<'tcx>,
        location: Location,
    ) {
        self.transfer_function(state).visit_statement(statement, location);
    }

    fn apply_terminator_effect(
        &self,
        state: &mut BitSet<Self::Idx>,
        terminator: &mir::Terminator<'tcx>,
        location: Location,
    ) {
        self.transfer_function(state).visit_terminator(terminator, location);
    }

    fn apply_call_return_effect(
        &self,
        state: &mut BitSet<Self::Idx>,
        block: BasicBlock,
        func: &mir::Operand<'tcx>,
        args: &[mir::Operand<'tcx>],
        return_place: &mir::Place<'tcx>,
    ) {
        self.transfer_function(state).apply_call_return_effect(block, func, args, return_place)
    }
}
