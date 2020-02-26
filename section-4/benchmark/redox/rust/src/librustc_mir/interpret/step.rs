//! This module contains the `InterpCx` methods for executing a single step of the interpreter.
//!
//! The main entry point is the `step` method.

use rustc::mir;
use rustc::ty::layout::LayoutOf;
use rustc::mir::interpret::{InterpResult, Scalar, PointerArithmetic};

use super::{InterpCx, Machine};

/// Classify whether an operator is "left-homogeneous", i.e., the LHS has the
/// same type as the result.
#[inline]
fn binop_left_homogeneous(op: mir::BinOp) -> bool {
    use rustc::mir::BinOp::*;
    match op {
        Add | Sub | Mul | Div | Rem | BitXor | BitAnd | BitOr |
        Offset | Shl | Shr =>
            true,
        Eq | Ne | Lt | Le | Gt | Ge =>
            false,
    }
}
/// Classify whether an operator is "right-homogeneous", i.e., the RHS has the
/// same type as the LHS.
#[inline]
fn binop_right_homogeneous(op: mir::BinOp) -> bool {
    use rustc::mir::BinOp::*;
    match op {
        Add | Sub | Mul | Div | Rem | BitXor | BitAnd | BitOr |
        Eq | Ne | Lt | Le | Gt | Ge =>
            true,
        Offset | Shl | Shr =>
            false,
    }
}

impl<'mir, 'tcx, M: Machine<'mir, 'tcx>> InterpCx<'mir, 'tcx, M> {
    pub fn run(&mut self) -> InterpResult<'tcx> {
        while self.step()? {}
        Ok(())
    }

    /// Returns `true` as long as there are more things to do.
    ///
    /// This is used by [priroda](https://github.com/oli-obk/priroda)
    pub fn step(&mut self) -> InterpResult<'tcx, bool> {
        if self.stack.is_empty() {
            return Ok(false);
        }

        let block = match self.frame().block {
            Some(block) => block,
            None => {
                // We are unwinding and this fn has no cleanup code.
                // Just go on unwinding.
                trace!("unwinding: skipping frame");
                self.pop_stack_frame(/* unwinding */ true)?;
                return Ok(true)
            }
        };
        let stmt_id = self.frame().stmt;
        let body = self.body();
        let basic_block = &body.basic_blocks()[block];

        let old_frames = self.cur_frame();

        if let Some(stmt) = basic_block.statements.get(stmt_id) {
            assert_eq!(old_frames, self.cur_frame());
            self.statement(stmt)?;
            return Ok(true);
        }

        M::before_terminator(self)?;

        let terminator = basic_block.terminator();
        assert_eq!(old_frames, self.cur_frame());
        self.terminator(terminator)?;
        Ok(true)
    }

    fn statement(&mut self, stmt: &mir::Statement<'tcx>) -> InterpResult<'tcx> {
        info!("{:?}", stmt);

        use rustc::mir::StatementKind::*;

        // Some statements (e.g., box) push new stack frames.
        // We have to record the stack frame number *before* executing the statement.
        let frame_idx = self.cur_frame();
        self.tcx.span = stmt.source_info.span;
        self.memory.tcx.span = stmt.source_info.span;

        match stmt.kind {
            Assign(box(ref place, ref rvalue)) => self.eval_rvalue_into_place(rvalue, place)?,

            SetDiscriminant {
                ref place,
                variant_index,
            } => {
                let dest = self.eval_place(place)?;
                self.write_discriminant_index(variant_index, dest)?;
            }

            // Mark locals as alive
            StorageLive(local) => {
                let old_val = self.storage_live(local)?;
                self.deallocate_local(old_val)?;
            }

            // Mark locals as dead
            StorageDead(local) => {
                let old_val = self.storage_dead(local);
                self.deallocate_local(old_val)?;
            }

            // No dynamic semantics attached to `FakeRead`; MIR
            // interpreter is solely intended for borrowck'ed code.
            FakeRead(..) => {}

            // Stacked Borrows.
            Retag(kind, ref place) => {
                let dest = self.eval_place(place)?;
                M::retag(self, kind, dest)?;
            }

            // Statements we do not track.
            AscribeUserType(..) => {}

            // Defined to do nothing. These are added by optimization passes, to avoid changing the
            // size of MIR constantly.
            Nop => {}

            InlineAsm { .. } => throw_unsup_format!("inline assembly is not supported"),
        }

        self.stack[frame_idx].stmt += 1;
        Ok(())
    }

    /// Evaluate an assignment statement.
    ///
    /// There is no separate `eval_rvalue` function. Instead, the code for handling each rvalue
    /// type writes its results directly into the memory specified by the place.
    pub fn eval_rvalue_into_place(
        &mut self,
        rvalue: &mir::Rvalue<'tcx>,
        place: &mir::Place<'tcx>,
    ) -> InterpResult<'tcx> {
        let dest = self.eval_place(place)?;

        use rustc::mir::Rvalue::*;
        match *rvalue {
            Use(ref operand) => {
                // Avoid recomputing the layout
                let op = self.eval_operand(operand, Some(dest.layout))?;
                self.copy_op(op, dest)?;
            }

            BinaryOp(bin_op, ref left, ref right) => {
                let layout = if binop_left_homogeneous(bin_op) { Some(dest.layout) } else { None };
                let left = self.read_immediate(self.eval_operand(left, layout)?)?;
                let layout = if binop_right_homogeneous(bin_op) { Some(left.layout) } else { None };
                let right = self.read_immediate(self.eval_operand(right, layout)?)?;
                self.binop_ignore_overflow(
                    bin_op,
                    left,
                    right,
                    dest,
                )?;
            }

            CheckedBinaryOp(bin_op, ref left, ref right) => {
                // Due to the extra boolean in the result, we can never reuse the `dest.layout`.
                let left = self.read_immediate(self.eval_operand(left, None)?)?;
                let layout = if binop_right_homogeneous(bin_op) { Some(left.layout) } else { None };
                let right = self.read_immediate(self.eval_operand(right, layout)?)?;
                self.binop_with_overflow(
                    bin_op,
                    left,
                    right,
                    dest,
                )?;
            }

            UnaryOp(un_op, ref operand) => {
                // The operand always has the same type as the result.
                let val = self.read_immediate(self.eval_operand(operand, Some(dest.layout))?)?;
                let val = self.unary_op(un_op, val)?;
                assert_eq!(val.layout, dest.layout, "layout mismatch for result of {:?}", un_op);
                self.write_immediate(*val, dest)?;
            }

            Aggregate(ref kind, ref operands) => {
                let (dest, active_field_index) = match **kind {
                    mir::AggregateKind::Adt(adt_def, variant_index, _, _, active_field_index) => {
                        self.write_discriminant_index(variant_index, dest)?;
                        if adt_def.is_enum() {
                            (self.place_downcast(dest, variant_index)?, active_field_index)
                        } else {
                            (dest, active_field_index)
                        }
                    }
                    _ => (dest, None)
                };

                for (i, operand) in operands.iter().enumerate() {
                    let op = self.eval_operand(operand, None)?;
                    // Ignore zero-sized fields.
                    if !op.layout.is_zst() {
                        let field_index = active_field_index.unwrap_or(i);
                        let field_dest = self.place_field(dest, field_index as u64)?;
                        self.copy_op(op, field_dest)?;
                    }
                }
            }

            Repeat(ref operand, _) => {
                let op = self.eval_operand(operand, None)?;
                let dest = self.force_allocation(dest)?;
                let length = dest.len(self)?;

                if let Some(first_ptr) = self.check_mplace_access(dest, None)? {
                    // Write the first.
                    let first = self.mplace_field(dest, 0)?;
                    self.copy_op(op, first.into())?;

                    if length > 1 {
                        let elem_size = first.layout.size;
                        // Copy the rest. This is performance-sensitive code
                        // for big static/const arrays!
                        let rest_ptr = first_ptr.offset(elem_size, self)?;
                        self.memory.copy_repeatedly(
                            first_ptr, rest_ptr, elem_size, length - 1, /*nonoverlapping:*/true
                        )?;
                    }
                }
            }

            Len(ref place) => {
                // FIXME(CTFE): don't allow computing the length of arrays in const eval
                let src = self.eval_place(place)?;
                let mplace = self.force_allocation(src)?;
                let len = mplace.len(self)?;
                let size = self.pointer_size();
                self.write_scalar(
                    Scalar::from_uint(len, size),
                    dest,
                )?;
            }

            Ref(_, _, ref place) => {
                let src = self.eval_place(place)?;
                let place = self.force_allocation(src)?;
                if place.layout.size.bytes() > 0 {
                    // definitely not a ZST
                    assert!(place.ptr.is_ptr(), "non-ZST places should be normalized to `Pointer`");
                }
                self.write_immediate(place.to_ref(), dest)?;
            }

            NullaryOp(mir::NullOp::Box, _) => {
                M::box_alloc(self, dest)?;
            }

            NullaryOp(mir::NullOp::SizeOf, ty) => {
                let ty = self.subst_from_frame_and_normalize_erasing_regions(ty);
                let layout = self.layout_of(ty)?;
                assert!(!layout.is_unsized(),
                        "SizeOf nullary MIR operator called for unsized type");
                let size = self.pointer_size();
                self.write_scalar(
                    Scalar::from_uint(layout.size.bytes(), size),
                    dest,
                )?;
            }

            Cast(kind, ref operand, _) => {
                let src = self.eval_operand(operand, None)?;
                self.cast(src, kind, dest)?;
            }

            Discriminant(ref place) => {
                let op = self.eval_place_to_op(place, None)?;
                let discr_val = self.read_discriminant(op)?.0;
                let size = dest.layout.size;
                self.write_scalar(Scalar::from_uint(discr_val, size), dest)?;
            }
        }

        self.dump_place(*dest);

        Ok(())
    }

    fn terminator(&mut self, terminator: &mir::Terminator<'tcx>) -> InterpResult<'tcx> {
        info!("{:?}", terminator.kind);
        self.tcx.span = terminator.source_info.span;
        self.memory.tcx.span = terminator.source_info.span;

        let old_stack = self.cur_frame();
        let old_bb = self.frame().block;

        self.eval_terminator(terminator)?;
        if !self.stack.is_empty() {
            // This should change *something*
            debug_assert!(self.cur_frame() != old_stack || self.frame().block != old_bb);
            info!("// {:?}", self.frame().block);
        }
        Ok(())
    }
}
