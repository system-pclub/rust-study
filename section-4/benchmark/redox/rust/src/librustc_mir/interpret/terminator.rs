use std::borrow::Cow;

use rustc::{mir, ty};
use rustc::ty::Instance;
use rustc::ty::layout::{self, TyLayout, LayoutOf};
use syntax::source_map::Span;
use rustc_target::spec::abi::Abi;

use super::{
    InterpResult, PointerArithmetic,
    InterpCx, Machine, OpTy, ImmTy, PlaceTy, MPlaceTy, StackPopCleanup, FnVal,
};

impl<'mir, 'tcx, M: Machine<'mir, 'tcx>> InterpCx<'mir, 'tcx, M> {
    #[inline]
    pub fn goto_block(&mut self, target: Option<mir::BasicBlock>) -> InterpResult<'tcx> {
        if let Some(target) = target {
            self.frame_mut().block = Some(target);
            self.frame_mut().stmt = 0;
            Ok(())
        } else {
            throw_ub!(Unreachable)
        }
    }

    pub(super) fn eval_terminator(
        &mut self,
        terminator: &mir::Terminator<'tcx>,
    ) -> InterpResult<'tcx> {
        use rustc::mir::TerminatorKind::*;
        match terminator.kind {
            Return => {
                self.frame().return_place.map(|r| self.dump_place(*r));
                self.pop_stack_frame(/* unwinding */ false)?
            }

            Goto { target } => self.goto_block(Some(target))?,

            SwitchInt {
                ref discr,
                ref values,
                ref targets,
                ..
            } => {
                let discr = self.read_immediate(self.eval_operand(discr, None)?)?;
                trace!("SwitchInt({:?})", *discr);

                // Branch to the `otherwise` case by default, if no match is found.
                let mut target_block = targets[targets.len() - 1];

                for (index, &const_int) in values.iter().enumerate() {
                    // Compare using binary_op, to also support pointer values
                    let res = self.overflowing_binary_op(mir::BinOp::Eq,
                        discr,
                        ImmTy::from_uint(const_int, discr.layout),
                    )?.0;
                    if res.to_bool()? {
                        target_block = targets[index];
                        break;
                    }
                }

                self.goto_block(Some(target_block))?;
            }

            Call {
                ref func,
                ref args,
                ref destination,
                ref cleanup,
                ..
            } => {
                let (dest, ret) = match *destination {
                    Some((ref lv, target)) => (Some(self.eval_place(lv)?), Some(target)),
                    None => (None, None),
                };

                let func = self.eval_operand(func, None)?;
                let (fn_val, abi) = match func.layout.ty.kind {
                    ty::FnPtr(sig) => {
                        let caller_abi = sig.abi();
                        let fn_ptr = self.read_scalar(func)?.not_undef()?;
                        let fn_val = self.memory.get_fn(fn_ptr)?;
                        (fn_val, caller_abi)
                    }
                    ty::FnDef(def_id, substs) => {
                        let sig = func.layout.ty.fn_sig(*self.tcx);
                        (FnVal::Instance(self.resolve(def_id, substs)?), sig.abi())
                    },
                    _ => {
                        bug!("invalid callee of type {:?}", func.layout.ty)
                    }
                };
                let args = self.eval_operands(args)?;
                self.eval_fn_call(
                    fn_val,
                    terminator.source_info.span,
                    abi,
                    &args[..],
                    dest,
                    ret,
                    *cleanup
                )?;
            }

            Drop {
                ref location,
                target,
                unwind,
            } => {
                // FIXME(CTFE): forbid drop in const eval
                let place = self.eval_place(location)?;
                let ty = place.layout.ty;
                trace!("TerminatorKind::drop: {:?}, type {}", location, ty);

                let instance = Instance::resolve_drop_in_place(*self.tcx, ty);
                self.drop_in_place(
                    place,
                    instance,
                    terminator.source_info.span,
                    target,
                    unwind
                )?;
            }

            Assert {
                ref cond,
                expected,
                ref msg,
                target,
                ..
            } => {
                let cond_val = self.read_immediate(self.eval_operand(cond, None)?)?
                    .to_scalar()?.to_bool()?;
                if expected == cond_val {
                    self.goto_block(Some(target))?;
                } else {
                    // Compute error message
                    use rustc::mir::interpret::PanicInfo::*;
                    return Err(match msg {
                        BoundsCheck { ref len, ref index } => {
                            let len = self
                                .read_immediate(self.eval_operand(len, None)?)
                                .expect("can't eval len")
                                .to_scalar()?
                                .to_bits(self.memory.pointer_size())? as u64;
                            let index = self
                                .read_immediate(self.eval_operand(index, None)?)
                                .expect("can't eval index")
                                .to_scalar()?
                                .to_bits(self.memory.pointer_size())? as u64;
                            err_panic!(BoundsCheck { len, index })
                        }
                        Overflow(op) => err_panic!(Overflow(*op)),
                        OverflowNeg => err_panic!(OverflowNeg),
                        DivisionByZero => err_panic!(DivisionByZero),
                        RemainderByZero => err_panic!(RemainderByZero),
                        GeneratorResumedAfterReturn => err_panic!(GeneratorResumedAfterReturn),
                        GeneratorResumedAfterPanic => err_panic!(GeneratorResumedAfterPanic),
                        Panic { .. } => bug!("`Panic` variant cannot occur in MIR"),
                    }
                    .into());
                }
            }


            // When we encounter Resume, we've finished unwinding
            // cleanup for the current stack frame. We pop it in order
            // to continue unwinding the next frame
            Resume => {
                trace!("unwinding: resuming from cleanup");
                // By definition, a Resume terminator means
                // that we're unwinding
                self.pop_stack_frame(/* unwinding */ true)?;
                return Ok(())
            },

            Yield { .. } |
            GeneratorDrop |
            DropAndReplace { .. } |
            Abort => unimplemented!("{:#?}", terminator.kind),
            FalseEdges { .. } => bug!("should have been eliminated by\
                                      `simplify_branches` mir pass"),
            FalseUnwind { .. } => bug!("should have been eliminated by\
                                       `simplify_branches` mir pass"),
            Unreachable => throw_ub!(Unreachable),
        }

        Ok(())
    }

    fn check_argument_compat(
        rust_abi: bool,
        caller: TyLayout<'tcx>,
        callee: TyLayout<'tcx>,
    ) -> bool {
        if caller.ty == callee.ty {
            // No question
            return true;
        }
        if !rust_abi {
            // Don't risk anything
            return false;
        }
        // Compare layout
        match (&caller.abi, &callee.abi) {
            // Different valid ranges are okay (once we enforce validity,
            // that will take care to make it UB to leave the range, just
            // like for transmute).
            (layout::Abi::Scalar(ref caller), layout::Abi::Scalar(ref callee)) =>
                caller.value == callee.value,
            (layout::Abi::ScalarPair(ref caller1, ref caller2),
             layout::Abi::ScalarPair(ref callee1, ref callee2)) =>
                caller1.value == callee1.value && caller2.value == callee2.value,
            // Be conservative
            _ => false
        }
    }

    /// Pass a single argument, checking the types for compatibility.
    fn pass_argument(
        &mut self,
        rust_abi: bool,
        caller_arg: &mut impl Iterator<Item=OpTy<'tcx, M::PointerTag>>,
        callee_arg: PlaceTy<'tcx, M::PointerTag>,
    ) -> InterpResult<'tcx> {
        if rust_abi && callee_arg.layout.is_zst() {
            // Nothing to do.
            trace!("Skipping callee ZST");
            return Ok(());
        }
        let caller_arg = caller_arg.next()
            .ok_or_else(|| err_unsup!(FunctionArgCountMismatch)) ?;
        if rust_abi {
            debug_assert!(!caller_arg.layout.is_zst(), "ZSTs must have been already filtered out");
        }
        // Now, check
        if !Self::check_argument_compat(rust_abi, caller_arg.layout, callee_arg.layout) {
            throw_unsup!(FunctionArgMismatch(caller_arg.layout.ty, callee_arg.layout.ty))
        }
        // We allow some transmutes here
        self.copy_op_transmute(caller_arg, callee_arg)
    }

    /// Call this function -- pushing the stack frame and initializing the arguments.
    fn eval_fn_call(
        &mut self,
        fn_val: FnVal<'tcx, M::ExtraFnVal>,
        span: Span,
        caller_abi: Abi,
        args: &[OpTy<'tcx, M::PointerTag>],
        dest: Option<PlaceTy<'tcx, M::PointerTag>>,
        ret: Option<mir::BasicBlock>,
        unwind: Option<mir::BasicBlock>
    ) -> InterpResult<'tcx> {
        trace!("eval_fn_call: {:#?}", fn_val);

        let instance = match fn_val {
            FnVal::Instance(instance) => instance,
            FnVal::Other(extra) => {
                return M::call_extra_fn(self, extra, args, dest, ret);
            }
        };

        match instance.def {
            ty::InstanceDef::Intrinsic(..) => {
                assert!(caller_abi == Abi::RustIntrinsic || caller_abi == Abi::PlatformIntrinsic);

                let old_stack = self.cur_frame();
                let old_bb = self.frame().block;
                M::call_intrinsic(self, span, instance, args, dest, ret, unwind)?;
                // No stack frame gets pushed, the main loop will just act as if the
                // call completed.
                if ret.is_some() {
                    self.goto_block(ret)?;
                } else {
                    // If this intrinsic call doesn't have a ret block,
                    // then the intrinsic implementation should have
                    // changed the stack frame (otherwise, we'll end
                    // up trying to execute this intrinsic call again)
                    debug_assert!(self.cur_frame() != old_stack || self.frame().block != old_bb);
                }
                if let Some(dest) = dest {
                    self.dump_place(*dest)
                }
                Ok(())
            }
            ty::InstanceDef::VtableShim(..) |
            ty::InstanceDef::ReifyShim(..) |
            ty::InstanceDef::ClosureOnceShim { .. } |
            ty::InstanceDef::FnPtrShim(..) |
            ty::InstanceDef::DropGlue(..) |
            ty::InstanceDef::CloneShim(..) |
            ty::InstanceDef::Item(_) => {
                // ABI check
                {
                    let callee_abi = {
                        let instance_ty = instance.ty(*self.tcx);
                        match instance_ty.kind {
                            ty::FnDef(..) =>
                                instance_ty.fn_sig(*self.tcx).abi(),
                            ty::Closure(..) => Abi::RustCall,
                            ty::Generator(..) => Abi::Rust,
                            _ => bug!("unexpected callee ty: {:?}", instance_ty),
                        }
                    };
                    let normalize_abi = |abi| match abi {
                        Abi::Rust | Abi::RustCall | Abi::RustIntrinsic | Abi::PlatformIntrinsic =>
                            // These are all the same ABI, really.
                            Abi::Rust,
                        abi =>
                            abi,
                    };
                    if normalize_abi(caller_abi) != normalize_abi(callee_abi) {
                        throw_unsup!(FunctionAbiMismatch(caller_abi, callee_abi))
                    }
                }

                // We need MIR for this fn
                let body = match M::find_fn(self, instance, args, dest, ret, unwind)? {
                    Some(body) => body,
                    None => return Ok(()),
                };

                self.push_stack_frame(
                    instance,
                    span,
                    body,
                    dest,
                    StackPopCleanup::Goto { ret, unwind }
                )?;

                // We want to pop this frame again in case there was an error, to put
                // the blame in the right location.  Until the 2018 edition is used in
                // the compiler, we have to do this with an immediately invoked function.
                let res = (||{
                    trace!(
                        "caller ABI: {:?}, args: {:#?}",
                        caller_abi,
                        args.iter()
                            .map(|arg| (arg.layout.ty, format!("{:?}", **arg)))
                            .collect::<Vec<_>>()
                    );
                    trace!(
                        "spread_arg: {:?}, locals: {:#?}",
                        body.spread_arg,
                        body.args_iter()
                            .map(|local|
                                (local, self.layout_of_local(self.frame(), local, None).unwrap().ty)
                            )
                            .collect::<Vec<_>>()
                    );

                    // Figure out how to pass which arguments.
                    // The Rust ABI is special: ZST get skipped.
                    let rust_abi = match caller_abi {
                        Abi::Rust | Abi::RustCall => true,
                        _ => false
                    };
                    // We have two iterators: Where the arguments come from,
                    // and where they go to.

                    // For where they come from: If the ABI is RustCall, we untuple the
                    // last incoming argument.  These two iterators do not have the same type,
                    // so to keep the code paths uniform we accept an allocation
                    // (for RustCall ABI only).
                    let caller_args : Cow<'_, [OpTy<'tcx, M::PointerTag>]> =
                        if caller_abi == Abi::RustCall && !args.is_empty() {
                            // Untuple
                            let (&untuple_arg, args) = args.split_last().unwrap();
                            trace!("eval_fn_call: Will pass last argument by untupling");
                            Cow::from(args.iter().map(|&a| Ok(a))
                                .chain((0..untuple_arg.layout.fields.count()).into_iter()
                                    .map(|i| self.operand_field(untuple_arg, i as u64))
                                )
                                .collect::<InterpResult<'_, Vec<OpTy<'tcx, M::PointerTag>>>>()?)
                        } else {
                            // Plain arg passing
                            Cow::from(args)
                        };
                    // Skip ZSTs
                    let mut caller_iter = caller_args.iter()
                        .filter(|op| !rust_abi || !op.layout.is_zst())
                        .map(|op| *op);

                    // Now we have to spread them out across the callee's locals,
                    // taking into account the `spread_arg`.  If we could write
                    // this is a single iterator (that handles `spread_arg`), then
                    // `pass_argument` would be the loop body. It takes care to
                    // not advance `caller_iter` for ZSTs.
                    let mut locals_iter = body.args_iter();
                    while let Some(local) = locals_iter.next() {
                        let dest = self.eval_place(
                            &mir::Place::from(local)
                        )?;
                        if Some(local) == body.spread_arg {
                            // Must be a tuple
                            for i in 0..dest.layout.fields.count() {
                                let dest = self.place_field(dest, i as u64)?;
                                self.pass_argument(rust_abi, &mut caller_iter, dest)?;
                            }
                        } else {
                            // Normal argument
                            self.pass_argument(rust_abi, &mut caller_iter, dest)?;
                        }
                    }
                    // Now we should have no more caller args
                    if caller_iter.next().is_some() {
                        trace!("Caller has passed too many args");
                        throw_unsup!(FunctionArgCountMismatch)
                    }
                    // Don't forget to check the return type!
                    if let Some(caller_ret) = dest {
                        let callee_ret = self.eval_place(
                            &mir::Place::return_place()
                        )?;
                        if !Self::check_argument_compat(
                            rust_abi,
                            caller_ret.layout,
                            callee_ret.layout,
                        ) {
                            throw_unsup!(
                                FunctionRetMismatch(caller_ret.layout.ty, callee_ret.layout.ty)
                            )
                        }
                    } else {
                        let local = mir::RETURN_PLACE;
                        let callee_layout = self.layout_of_local(self.frame(), local, None)?;
                        if !callee_layout.abi.is_uninhabited() {
                            throw_unsup!(FunctionRetMismatch(
                                self.tcx.types.never, callee_layout.ty
                            ))
                        }
                    }
                    Ok(())
                })();
                match res {
                    Err(err) => {
                        self.stack.pop();
                        Err(err)
                    }
                    Ok(v) => Ok(v)
                }
            }
            // cannot use the shim here, because that will only result in infinite recursion
            ty::InstanceDef::Virtual(_, idx) => {
                let mut args = args.to_vec();
                // We have to implement all "object safe receivers".  Currently we
                // support built-in pointers (&, &mut, Box) as well as unsized-self.  We do
                // not yet support custom self types.
                // Also see librustc_codegen_llvm/abi.rs and librustc_codegen_llvm/mir/block.rs.
                let receiver_place = match args[0].layout.ty.builtin_deref(true) {
                    Some(_) => {
                        // Built-in pointer.
                        self.deref_operand(args[0])?
                    }
                    None => {
                        // Unsized self.
                        args[0].assert_mem_place()
                    }
                };
                // Find and consult vtable
                let vtable = receiver_place.vtable();
                let drop_fn = self.get_vtable_slot(vtable, idx)?;

                // `*mut receiver_place.layout.ty` is almost the layout that we
                // want for args[0]: We have to project to field 0 because we want
                // a thin pointer.
                assert!(receiver_place.layout.is_unsized());
                let receiver_ptr_ty = self.tcx.mk_mut_ptr(receiver_place.layout.ty);
                let this_receiver_ptr = self.layout_of(receiver_ptr_ty)?.field(self, 0)?;
                // Adjust receiver argument.
                args[0] = OpTy::from(ImmTy {
                    layout: this_receiver_ptr,
                    imm: receiver_place.ptr.into()
                });
                trace!("Patched self operand to {:#?}", args[0]);
                // recurse with concrete function
                self.eval_fn_call(drop_fn, span, caller_abi, &args, dest, ret, unwind)
            }
        }
    }

    fn drop_in_place(
        &mut self,
        place: PlaceTy<'tcx, M::PointerTag>,
        instance: ty::Instance<'tcx>,
        span: Span,
        target: mir::BasicBlock,
        unwind: Option<mir::BasicBlock>
    ) -> InterpResult<'tcx> {
        trace!("drop_in_place: {:?},\n  {:?}, {:?}", *place, place.layout.ty, instance);
        // We take the address of the object.  This may well be unaligned, which is fine
        // for us here.  However, unaligned accesses will probably make the actual drop
        // implementation fail -- a problem shared by rustc.
        let place = self.force_allocation(place)?;

        let (instance, place) = match place.layout.ty.kind {
            ty::Dynamic(..) => {
                // Dropping a trait object.
                self.unpack_dyn_trait(place)?
            }
            _ => (instance, place),
        };

        let arg = ImmTy {
            imm: place.to_ref(),
            layout: self.layout_of(self.tcx.mk_mut_ptr(place.layout.ty))?,
        };

        let ty = self.tcx.mk_unit(); // return type is ()
        let dest = MPlaceTy::dangling(self.layout_of(ty)?, self);

        self.eval_fn_call(
            FnVal::Instance(instance),
            span,
            Abi::Rust,
            &[arg.into()],
            Some(dest.into()),
            Some(target),
            unwind
        )
    }
}
