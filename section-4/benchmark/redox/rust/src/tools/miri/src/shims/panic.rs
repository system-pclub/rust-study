//! Panic runtime for Miri.
//!
//! The core pieces of the runtime are:
//! - An implementation of `__rust_maybe_catch_panic` that pushes the invoked stack frame with
//!   some extra metadata derived from the panic-catching arguments of `__rust_maybe_catch_panic`.
//! - A hack in `libpanic_unwind` that calls the `miri_start_panic` intrinsic instead of the
//!   target-native panic runtime. (This lives in the rustc repo.)
//! - An implementation of `miri_start_panic` that stores its argument (the panic payload), and then
//!   immediately returns, but on the *unwind* edge (not the normal return edge), thus initiating unwinding.
//! - A hook executed each time a frame is popped, such that if the frame pushed by `__rust_maybe_catch_panic`
//!   gets popped *during unwinding*, we take the panic payload and store it according to the extra
//!   metadata we remembered when pushing said frame.

use rustc::mir;
use crate::*;
use super::machine::FrameData;
use rustc_target::spec::PanicStrategy;
use crate::rustc_target::abi::LayoutOf;

/// Holds all of the relevant data for a call to
/// `__rust_maybe_catch_panic`.
///
/// If a panic occurs, we update this data with
/// the information from the panic site.
#[derive(Debug)]
pub struct CatchUnwindData<'tcx> {
    /// The dereferenced `data_ptr` argument passed to `__rust_maybe_catch_panic`.
    pub data_place: MPlaceTy<'tcx, Tag>,
    /// The dereferenced `vtable_ptr` argument passed to `__rust_maybe_catch_panic`.
    pub vtable_place: MPlaceTy<'tcx, Tag>,
    /// The `dest` from the original call to `__rust_maybe_catch_panic`.
    pub dest: PlaceTy<'tcx, Tag>,
}

impl<'mir, 'tcx> EvalContextExt<'mir, 'tcx> for crate::MiriEvalContext<'mir, 'tcx> {}
pub trait EvalContextExt<'mir, 'tcx: 'mir>: crate::MiriEvalContextExt<'mir, 'tcx> {

    /// Handles the special "miri_start_panic" intrinsic, which is called
    /// by libpanic_unwind to delegate the actual unwinding process to Miri.
    #[inline(always)]
    fn handle_miri_start_panic(
        &mut self,
        args: &[OpTy<'tcx, Tag>],
        unwind: Option<mir::BasicBlock>
    ) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();

        trace!("miri_start_panic: {:?}", this.frame().span);

        // Get the raw pointer stored in arg[0] (the panic payload).
        let scalar = this.read_immediate(args[0])?;
        assert!(this.machine.panic_payload.is_none(), "the panic runtime should avoid double-panics");
        this.machine.panic_payload = Some(scalar);

        // Jump to the unwind block to begin unwinding.
        // We don't use `goto_block` as that is just meant for normal returns.
        let next_frame = this.frame_mut();
        next_frame.block = unwind;
        next_frame.stmt = 0;
        return Ok(())
    }

    #[inline(always)]
    fn handle_catch_panic(
        &mut self,
        args: &[OpTy<'tcx, Tag>],
        dest: PlaceTy<'tcx, Tag>,
        ret: mir::BasicBlock,
    ) -> InterpResult<'tcx> {
        let this = self.eval_context_mut();
        let tcx = &{this.tcx.tcx};

        // fn __rust_maybe_catch_panic(
        //     f: fn(*mut u8),
        //     data: *mut u8,
        //     data_ptr: *mut usize,
        //     vtable_ptr: *mut usize,
        // ) -> u32

        // Get all the arguments.
        let f = this.read_scalar(args[0])?.not_undef()?;
        let f_arg = this.read_scalar(args[1])?.not_undef()?;
        let data_place = this.deref_operand(args[2])?;
        let vtable_place = this.deref_operand(args[3])?;

        // Now we make a function call, and pass `f_arg` as first and only argument.
        let f_instance = this.memory.get_fn(f)?.as_instance()?;
        trace!("__rust_maybe_catch_panic: {:?}", f_instance);
        // TODO: consider making this reusable? `InterpCx::step` does something similar
        // for the TLS destructors, and of course `eval_main`.
        let mir = this.load_mir(f_instance.def, None)?;
        let ret_place =
            MPlaceTy::dangling(this.layout_of(tcx.mk_unit())?, this).into();
        this.push_stack_frame(
            f_instance,
            mir.span,
            mir,
            Some(ret_place),
            // Directly return to caller.
            StackPopCleanup::Goto { ret: Some(ret), unwind: None },
        )?;

        let mut args = this.frame().body.args_iter();
        // First argument.
        let arg_local = args
            .next()
            .expect("Argument to __rust_maybe_catch_panic does not take enough arguments.");
        let arg_dest = this.local_place(arg_local)?;
        this.write_scalar(f_arg, arg_dest)?;
        // No more arguments.
        args.next().expect_none("__rust_maybe_catch_panic argument has more arguments than expected");

        // We ourselves will return `0`, eventually (will be overwritten if we catch a panic).
        this.write_null(dest)?;

        // In unwind mode, we tag this frame with some extra data.
        // This lets `handle_stack_pop` (below) know that we should stop unwinding
        // when we pop this frame.
        if this.tcx.tcx.sess.panic_strategy() == PanicStrategy::Unwind {
            this.frame_mut().extra.catch_panic = Some(CatchUnwindData {
                data_place,
                vtable_place,
                dest,
            })
        }

        return Ok(());
    }

    #[inline(always)]
    fn handle_stack_pop(
        &mut self,
        mut extra: FrameData<'tcx>,
        unwinding: bool
    ) -> InterpResult<'tcx, StackPopInfo> {
        let this = self.eval_context_mut();

        trace!("handle_stack_pop(extra = {:?}, unwinding = {})", extra, unwinding);

        // We only care about `catch_panic` if we're unwinding - if we're doing a normal
        // return, then we don't need to do anything special.
        let res = if let (true, Some(unwind_data)) = (unwinding, extra.catch_panic.take()) {
            // We've just popped a frame that was pushed by `__rust_maybe_catch_panic`,
            // and we are unwinding, so we should catch that.
            trace!("unwinding: found catch_panic frame during unwinding: {:?}", this.frame().span);

            // `panic_payload` now holds a `*mut (dyn Any + Send)`,
            // provided by the `miri_start_panic` intrinsic.
            // We want to split this into its consituient parts -
            // the data and vtable pointers - and store them according to
            // `unwind_data`, i.e., we store them where `__rust_maybe_catch_panic`
            // was told to put them.
            let payload = this.machine.panic_payload.take().unwrap();
            let payload = this.ref_to_mplace(payload)?;
            let payload_data_place = payload.ptr;
            let payload_vtable_place = payload.meta.expect("Expected fat pointer");

            this.write_scalar(payload_data_place, unwind_data.data_place.into())?;
            this.write_scalar(payload_vtable_place, unwind_data.vtable_place.into())?;

            // We set the return value of `__rust_maybe_catch_panic` to 1,
            // since there was a panic.
            let dest = unwind_data.dest;
            this.write_scalar(Scalar::from_int(1, dest.layout.size), dest)?;

            StackPopInfo::StopUnwinding
        } else {
            StackPopInfo::Normal
        };
        this.memory.extra.stacked_borrows.borrow_mut().end_call(extra.call_id);
        Ok(res)
    }
}
