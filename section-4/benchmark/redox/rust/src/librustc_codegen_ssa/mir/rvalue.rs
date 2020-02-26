use super::{FunctionCx, LocalRef};
use super::operand::{OperandRef, OperandValue};
use super::place::PlaceRef;

use crate::base;
use crate::MemFlags;
use crate::common::{self, RealPredicate, IntPredicate};
use crate::traits::*;

use rustc::ty::{self, Ty, adjustment::{PointerCast}, Instance};
use rustc::ty::cast::{CastTy, IntTy};
use rustc::ty::layout::{self, LayoutOf, HasTyCtxt};
use rustc::mir;
use rustc::middle::lang_items::ExchangeMallocFnLangItem;
use rustc_apfloat::{ieee, Float, Status, Round};
use syntax::symbol::sym;
use syntax::source_map::{DUMMY_SP, Span};

use std::{u128, i128};

impl<'a, 'tcx, Bx: BuilderMethods<'a, 'tcx>> FunctionCx<'a, 'tcx, Bx> {
    pub fn codegen_rvalue(
        &mut self,
        mut bx: Bx,
        dest: PlaceRef<'tcx, Bx::Value>,
        rvalue: &mir::Rvalue<'tcx>
    ) -> Bx {
        debug!("codegen_rvalue(dest.llval={:?}, rvalue={:?})",
               dest.llval, rvalue);

        match *rvalue {
           mir::Rvalue::Use(ref operand) => {
               let cg_operand = self.codegen_operand(&mut bx, operand);
               // FIXME: consider not copying constants through stack. (Fixable by codegen'ing
               // constants into `OperandValue::Ref`; why don’t we do that yet if we don’t?)
               cg_operand.val.store(&mut bx, dest);
               bx
           }

            mir::Rvalue::Cast(mir::CastKind::Pointer(PointerCast::Unsize), ref source, _) => {
                // The destination necessarily contains a fat pointer, so if
                // it's a scalar pair, it's a fat pointer or newtype thereof.
                if bx.cx().is_backend_scalar_pair(dest.layout) {
                    // Into-coerce of a thin pointer to a fat pointer -- just
                    // use the operand path.
                    let (mut bx, temp) = self.codegen_rvalue_operand(bx, rvalue);
                    temp.val.store(&mut bx, dest);
                    return bx;
                }

                // Unsize of a nontrivial struct. I would prefer for
                // this to be eliminated by MIR building, but
                // `CoerceUnsized` can be passed by a where-clause,
                // so the (generic) MIR may not be able to expand it.
                let operand = self.codegen_operand(&mut bx, source);
                match operand.val {
                    OperandValue::Pair(..) |
                    OperandValue::Immediate(_) => {
                        // Unsize from an immediate structure. We don't
                        // really need a temporary alloca here, but
                        // avoiding it would require us to have
                        // `coerce_unsized_into` use `extractvalue` to
                        // index into the struct, and this case isn't
                        // important enough for it.
                        debug!("codegen_rvalue: creating ugly alloca");
                        let scratch = PlaceRef::alloca(&mut bx, operand.layout);
                        scratch.storage_live(&mut bx);
                        operand.val.store(&mut bx, scratch);
                        base::coerce_unsized_into(&mut bx, scratch, dest);
                        scratch.storage_dead(&mut bx);
                    }
                    OperandValue::Ref(llref, None, align) => {
                        let source = PlaceRef::new_sized_aligned(llref, operand.layout, align);
                        base::coerce_unsized_into(&mut bx, source, dest);
                    }
                    OperandValue::Ref(_, Some(_), _) => {
                        bug!("unsized coercion on an unsized rvalue");
                    }
                }
                bx
            }

            mir::Rvalue::Repeat(ref elem, count) => {
                let cg_elem = self.codegen_operand(&mut bx, elem);

                // Do not generate the loop for zero-sized elements or empty arrays.
                if dest.layout.is_zst() {
                    return bx;
                }

                if let OperandValue::Immediate(v) = cg_elem.val {
                    let zero = bx.const_usize(0);
                    let start = dest.project_index(&mut bx, zero).llval;
                    let size = bx.const_usize(dest.layout.size.bytes());

                    // Use llvm.memset.p0i8.* to initialize all zero arrays
                    if bx.cx().const_to_opt_uint(v) == Some(0) {
                        let fill = bx.cx().const_u8(0);
                        bx.memset(start, fill, size, dest.align, MemFlags::empty());
                        return bx;
                    }

                    // Use llvm.memset.p0i8.* to initialize byte arrays
                    let v = base::from_immediate(&mut bx, v);
                    if bx.cx().val_ty(v) == bx.cx().type_i8() {
                        bx.memset(start, v, size, dest.align, MemFlags::empty());
                        return bx;
                    }
                }

                bx.write_operand_repeatedly(cg_elem, count, dest)
            }

            mir::Rvalue::Aggregate(ref kind, ref operands) => {
                let (dest, active_field_index) = match **kind {
                    mir::AggregateKind::Adt(adt_def, variant_index, _, _, active_field_index) => {
                        dest.codegen_set_discr(&mut bx, variant_index);
                        if adt_def.is_enum() {
                            (dest.project_downcast(&mut bx, variant_index), active_field_index)
                        } else {
                            (dest, active_field_index)
                        }
                    }
                    _ => (dest, None)
                };
                for (i, operand) in operands.iter().enumerate() {
                    let op = self.codegen_operand(&mut bx, operand);
                    // Do not generate stores and GEPis for zero-sized fields.
                    if !op.layout.is_zst() {
                        let field_index = active_field_index.unwrap_or(i);
                        let field = dest.project_field(&mut bx, field_index);
                        op.val.store(&mut bx, field);
                    }
                }
                bx
            }

            _ => {
                assert!(self.rvalue_creates_operand(rvalue, DUMMY_SP));
                let (mut bx, temp) = self.codegen_rvalue_operand(bx, rvalue);
                temp.val.store(&mut bx, dest);
                bx
            }
        }
    }

    pub fn codegen_rvalue_unsized(
        &mut self,
        mut bx: Bx,
        indirect_dest: PlaceRef<'tcx, Bx::Value>,
        rvalue: &mir::Rvalue<'tcx>,
    ) -> Bx {
        debug!("codegen_rvalue_unsized(indirect_dest.llval={:?}, rvalue={:?})",
               indirect_dest.llval, rvalue);

        match *rvalue {
            mir::Rvalue::Use(ref operand) => {
                let cg_operand = self.codegen_operand(&mut bx, operand);
                cg_operand.val.store_unsized(&mut bx, indirect_dest);
                bx
            }

            _ => bug!("unsized assignment other than `Rvalue::Use`"),
        }
    }

    pub fn codegen_rvalue_operand(
        &mut self,
        mut bx: Bx,
        rvalue: &mir::Rvalue<'tcx>
    ) -> (Bx, OperandRef<'tcx, Bx::Value>) {
        assert!(
            self.rvalue_creates_operand(rvalue, DUMMY_SP),
            "cannot codegen {:?} to operand",
            rvalue,
        );

        match *rvalue {
            mir::Rvalue::Cast(ref kind, ref source, mir_cast_ty) => {
                let operand = self.codegen_operand(&mut bx, source);
                debug!("cast operand is {:?}", operand);
                let cast = bx.cx().layout_of(self.monomorphize(&mir_cast_ty));

                let val = match *kind {
                    mir::CastKind::Pointer(PointerCast::ReifyFnPointer) => {
                        match operand.layout.ty.kind {
                            ty::FnDef(def_id, substs) => {
                                if bx.cx().tcx().has_attr(def_id, sym::rustc_args_required_const) {
                                    bug!("reifying a fn ptr that requires const arguments");
                                }
                                OperandValue::Immediate(
                                    bx.get_fn_addr(
                                        ty::Instance::resolve_for_fn_ptr(
                                            bx.tcx(),
                                            ty::ParamEnv::reveal_all(),
                                            def_id,
                                            substs
                                        ).unwrap()
                                    )
                                )
                            }
                            _ => {
                                bug!("{} cannot be reified to a fn ptr", operand.layout.ty)
                            }
                        }
                    }
                    mir::CastKind::Pointer(PointerCast::ClosureFnPointer(_)) => {
                        match operand.layout.ty.kind {
                            ty::Closure(def_id, substs) => {
                                let instance = Instance::resolve_closure(
                                    bx.cx().tcx(),
                                    def_id,
                                    substs,
                                    ty::ClosureKind::FnOnce);
                                OperandValue::Immediate(bx.cx().get_fn_addr(instance))
                            }
                            _ => {
                                bug!("{} cannot be cast to a fn ptr", operand.layout.ty)
                            }
                        }
                    }
                    mir::CastKind::Pointer(PointerCast::UnsafeFnPointer) => {
                        // This is a no-op at the LLVM level.
                        operand.val
                    }
                    mir::CastKind::Pointer(PointerCast::Unsize) => {
                        assert!(bx.cx().is_backend_scalar_pair(cast));
                        match operand.val {
                            OperandValue::Pair(lldata, llextra) => {
                                // unsize from a fat pointer -- this is a
                                // "trait-object-to-supertrait" coercion, for
                                // example, `&'a fmt::Debug + Send => &'a fmt::Debug`.

                                // HACK(eddyb) have to bitcast pointers
                                // until LLVM removes pointee types.
                                let lldata = bx.pointercast(lldata,
                                    bx.cx().scalar_pair_element_backend_type(cast, 0, true));
                                OperandValue::Pair(lldata, llextra)
                            }
                            OperandValue::Immediate(lldata) => {
                                // "standard" unsize
                                let (lldata, llextra) = base::unsize_thin_ptr(&mut bx, lldata,
                                    operand.layout.ty, cast.ty);
                                OperandValue::Pair(lldata, llextra)
                            }
                            OperandValue::Ref(..) => {
                                bug!("by-ref operand {:?} in `codegen_rvalue_operand`",
                                     operand);
                            }
                        }
                    }
                    mir::CastKind::Pointer(PointerCast::MutToConstPointer) |
                    mir::CastKind::Misc if bx.cx().is_backend_scalar_pair(operand.layout) => {
                        if let OperandValue::Pair(data_ptr, meta) = operand.val {
                            if bx.cx().is_backend_scalar_pair(cast) {
                                let data_cast = bx.pointercast(data_ptr,
                                    bx.cx().scalar_pair_element_backend_type(cast, 0, true));
                                OperandValue::Pair(data_cast, meta)
                            } else { // cast to thin-ptr
                                // Cast of fat-ptr to thin-ptr is an extraction of data-ptr and
                                // pointer-cast of that pointer to desired pointer type.
                                let llcast_ty = bx.cx().immediate_backend_type(cast);
                                let llval = bx.pointercast(data_ptr, llcast_ty);
                                OperandValue::Immediate(llval)
                            }
                        } else {
                            bug!("unexpected non-pair operand");
                        }
                    }
                    mir::CastKind::Pointer(PointerCast::MutToConstPointer) |
                    mir::CastKind::Pointer(PointerCast::ArrayToPointer) |
                    mir::CastKind::Misc => {
                        assert!(bx.cx().is_backend_immediate(cast));
                        let ll_t_out = bx.cx().immediate_backend_type(cast);
                        if operand.layout.abi.is_uninhabited() {
                            let val = OperandValue::Immediate(bx.cx().const_undef(ll_t_out));
                            return (bx, OperandRef {
                                val,
                                layout: cast,
                            });
                        }
                        let r_t_in = CastTy::from_ty(operand.layout.ty)
                            .expect("bad input type for cast");
                        let r_t_out = CastTy::from_ty(cast.ty).expect("bad output type for cast");
                        let ll_t_in = bx.cx().immediate_backend_type(operand.layout);
                        match operand.layout.variants {
                            layout::Variants::Single { index } => {
                                if let Some(discr) =
                                    operand.layout.ty.discriminant_for_variant(bx.tcx(), index)
                                {
                                    let discr_val = bx.cx().const_uint_big(ll_t_out, discr.val);
                                    return (bx, OperandRef {
                                        val: OperandValue::Immediate(discr_val),
                                        layout: cast,
                                    });
                                }
                            }
                            layout::Variants::Multiple { .. } => {},
                        }
                        let llval = operand.immediate();

                        let mut signed = false;
                        if let layout::Abi::Scalar(ref scalar) = operand.layout.abi {
                            if let layout::Int(_, s) = scalar.value {
                                // We use `i1` for bytes that are always `0` or `1`,
                                // e.g., `#[repr(i8)] enum E { A, B }`, but we can't
                                // let LLVM interpret the `i1` as signed, because
                                // then `i1 1` (i.e., E::B) is effectively `i8 -1`.
                                signed = !scalar.is_bool() && s;

                                let er = scalar.valid_range_exclusive(bx.cx());
                                if er.end != er.start &&
                                   scalar.valid_range.end() > scalar.valid_range.start() {
                                    // We want `table[e as usize]` to not
                                    // have bound checks, and this is the most
                                    // convenient place to put the `assume`.
                                    let ll_t_in_const =
                                        bx.cx().const_uint_big(ll_t_in, *scalar.valid_range.end());
                                    let cmp = bx.icmp(
                                        IntPredicate::IntULE,
                                        llval,
                                        ll_t_in_const
                                    );
                                    bx.assume(cmp);
                                }
                            }
                        }

                        let newval = match (r_t_in, r_t_out) {
                            (CastTy::Int(_), CastTy::Int(_)) => {
                                bx.intcast(llval, ll_t_out, signed)
                            }
                            (CastTy::Float, CastTy::Float) => {
                                let srcsz = bx.cx().float_width(ll_t_in);
                                let dstsz = bx.cx().float_width(ll_t_out);
                                if dstsz > srcsz {
                                    bx.fpext(llval, ll_t_out)
                                } else if srcsz > dstsz {
                                    bx.fptrunc(llval, ll_t_out)
                                } else {
                                    llval
                                }
                            }
                            (CastTy::Ptr(_), CastTy::Ptr(_)) |
                            (CastTy::FnPtr, CastTy::Ptr(_)) |
                            (CastTy::RPtr(_), CastTy::Ptr(_)) =>
                                bx.pointercast(llval, ll_t_out),
                            (CastTy::Ptr(_), CastTy::Int(_)) |
                            (CastTy::FnPtr, CastTy::Int(_)) =>
                                bx.ptrtoint(llval, ll_t_out),
                            (CastTy::Int(_), CastTy::Ptr(_)) => {
                                let usize_llval = bx.intcast(llval, bx.cx().type_isize(), signed);
                                bx.inttoptr(usize_llval, ll_t_out)
                            }
                            (CastTy::Int(_), CastTy::Float) =>
                                cast_int_to_float(&mut bx, signed, llval, ll_t_in, ll_t_out),
                            (CastTy::Float, CastTy::Int(IntTy::I)) =>
                                cast_float_to_int(&mut bx, true, llval, ll_t_in, ll_t_out),
                            (CastTy::Float, CastTy::Int(_)) =>
                                cast_float_to_int(&mut bx, false, llval, ll_t_in, ll_t_out),
                            _ => bug!("unsupported cast: {:?} to {:?}", operand.layout.ty, cast.ty)
                        };
                        OperandValue::Immediate(newval)
                    }
                };
                (bx, OperandRef {
                    val,
                    layout: cast
                })
            }

            mir::Rvalue::Ref(_, bk, ref place) => {
                let cg_place = self.codegen_place(&mut bx, &place.as_ref());

                let ty = cg_place.layout.ty;

                // Note: places are indirect, so storing the `llval` into the
                // destination effectively creates a reference.
                let val = if !bx.cx().type_has_metadata(ty) {
                    OperandValue::Immediate(cg_place.llval)
                } else {
                    OperandValue::Pair(cg_place.llval, cg_place.llextra.unwrap())
                };
                (bx, OperandRef {
                    val,
                    layout: self.cx.layout_of(self.cx.tcx().mk_ref(
                        self.cx.tcx().lifetimes.re_erased,
                        ty::TypeAndMut { ty, mutbl: bk.to_mutbl_lossy() }
                    )),
                })
            }

            mir::Rvalue::Len(ref place) => {
                let size = self.evaluate_array_len(&mut bx, place);
                let operand = OperandRef {
                    val: OperandValue::Immediate(size),
                    layout: bx.cx().layout_of(bx.tcx().types.usize),
                };
                (bx, operand)
            }

            mir::Rvalue::BinaryOp(op, ref lhs, ref rhs) => {
                let lhs = self.codegen_operand(&mut bx, lhs);
                let rhs = self.codegen_operand(&mut bx, rhs);
                let llresult = match (lhs.val, rhs.val) {
                    (OperandValue::Pair(lhs_addr, lhs_extra),
                     OperandValue::Pair(rhs_addr, rhs_extra)) => {
                        self.codegen_fat_ptr_binop(&mut bx, op,
                                                 lhs_addr, lhs_extra,
                                                 rhs_addr, rhs_extra,
                                                 lhs.layout.ty)
                    }

                    (OperandValue::Immediate(lhs_val),
                     OperandValue::Immediate(rhs_val)) => {
                        self.codegen_scalar_binop(&mut bx, op, lhs_val, rhs_val, lhs.layout.ty)
                    }

                    _ => bug!()
                };
                let operand = OperandRef {
                    val: OperandValue::Immediate(llresult),
                    layout: bx.cx().layout_of(
                        op.ty(bx.tcx(), lhs.layout.ty, rhs.layout.ty)),
                };
                (bx, operand)
            }
            mir::Rvalue::CheckedBinaryOp(op, ref lhs, ref rhs) => {
                let lhs = self.codegen_operand(&mut bx, lhs);
                let rhs = self.codegen_operand(&mut bx, rhs);
                let result = self.codegen_scalar_checked_binop(&mut bx, op,
                                                             lhs.immediate(), rhs.immediate(),
                                                             lhs.layout.ty);
                let val_ty = op.ty(bx.tcx(), lhs.layout.ty, rhs.layout.ty);
                let operand_ty = bx.tcx().intern_tup(&[val_ty, bx.tcx().types.bool]);
                let operand = OperandRef {
                    val: result,
                    layout: bx.cx().layout_of(operand_ty)
                };

                (bx, operand)
            }

            mir::Rvalue::UnaryOp(op, ref operand) => {
                let operand = self.codegen_operand(&mut bx, operand);
                let lloperand = operand.immediate();
                let is_float = operand.layout.ty.is_floating_point();
                let llval = match op {
                    mir::UnOp::Not => bx.not(lloperand),
                    mir::UnOp::Neg => if is_float {
                        bx.fneg(lloperand)
                    } else {
                        bx.neg(lloperand)
                    }
                };
                (bx, OperandRef {
                    val: OperandValue::Immediate(llval),
                    layout: operand.layout,
                })
            }

            mir::Rvalue::Discriminant(ref place) => {
                let discr_ty = rvalue.ty(&*self.mir, bx.tcx());
                let discr =  self.codegen_place(&mut bx, &place.as_ref())
                    .codegen_get_discr(&mut bx, discr_ty);
                (bx, OperandRef {
                    val: OperandValue::Immediate(discr),
                    layout: self.cx.layout_of(discr_ty)
                })
            }

            mir::Rvalue::NullaryOp(mir::NullOp::SizeOf, ty) => {
                assert!(bx.cx().type_is_sized(ty));
                let val = bx.cx().const_usize(bx.cx().layout_of(ty).size.bytes());
                let tcx = self.cx.tcx();
                (bx, OperandRef {
                    val: OperandValue::Immediate(val),
                    layout: self.cx.layout_of(tcx.types.usize),
                })
            }

            mir::Rvalue::NullaryOp(mir::NullOp::Box, content_ty) => {
                let content_ty = self.monomorphize(&content_ty);
                let content_layout = bx.cx().layout_of(content_ty);
                let llsize = bx.cx().const_usize(content_layout.size.bytes());
                let llalign = bx.cx().const_usize(content_layout.align.abi.bytes());
                let box_layout = bx.cx().layout_of(bx.tcx().mk_box(content_ty));
                let llty_ptr = bx.cx().backend_type(box_layout);

                // Allocate space:
                let def_id = match bx.tcx().lang_items().require(ExchangeMallocFnLangItem) {
                    Ok(id) => id,
                    Err(s) => {
                        bx.cx().sess().fatal(&format!("allocation of `{}` {}", box_layout.ty, s));
                    }
                };
                let instance = ty::Instance::mono(bx.tcx(), def_id);
                let r = bx.cx().get_fn_addr(instance);
                let call = bx.call(r, &[llsize, llalign], None);
                let val = bx.pointercast(call, llty_ptr);

                let operand = OperandRef {
                    val: OperandValue::Immediate(val),
                    layout: box_layout,
                };
                (bx, operand)
            }
            mir::Rvalue::Use(ref operand) => {
                let operand = self.codegen_operand(&mut bx, operand);
                (bx, operand)
            }
            mir::Rvalue::Repeat(..) |
            mir::Rvalue::Aggregate(..) => {
                // According to `rvalue_creates_operand`, only ZST
                // aggregate rvalues are allowed to be operands.
                let ty = rvalue.ty(self.mir, self.cx.tcx());
                let operand = OperandRef::new_zst(
                    &mut bx,
                    self.cx.layout_of(self.monomorphize(&ty)),
                );
                (bx, operand)
            }
        }
    }

    fn evaluate_array_len(
        &mut self,
        bx: &mut Bx,
        place: &mir::Place<'tcx>,
    ) -> Bx::Value {
        // ZST are passed as operands and require special handling
        // because codegen_place() panics if Local is operand.
        if let Some(index) = place.as_local() {
            if let LocalRef::Operand(Some(op)) = self.locals[index] {
                if let ty::Array(_, n) = op.layout.ty.kind {
                    let n = n.eval_usize(bx.cx().tcx(), ty::ParamEnv::reveal_all());
                    return bx.cx().const_usize(n);
                }
            }
        }
        // use common size calculation for non zero-sized types
        let cg_value = self.codegen_place(bx, &place.as_ref());
        cg_value.len(bx.cx())
    }

    pub fn codegen_scalar_binop(
        &mut self,
        bx: &mut Bx,
        op: mir::BinOp,
        lhs: Bx::Value,
        rhs: Bx::Value,
        input_ty: Ty<'tcx>,
    ) -> Bx::Value {
        let is_float = input_ty.is_floating_point();
        let is_signed = input_ty.is_signed();
        match op {
            mir::BinOp::Add => if is_float {
                bx.fadd(lhs, rhs)
            } else {
                bx.add(lhs, rhs)
            },
            mir::BinOp::Sub => if is_float {
                bx.fsub(lhs, rhs)
            } else {
                bx.sub(lhs, rhs)
            },
            mir::BinOp::Mul => if is_float {
                bx.fmul(lhs, rhs)
            } else {
                bx.mul(lhs, rhs)
            },
            mir::BinOp::Div => if is_float {
                bx.fdiv(lhs, rhs)
            } else if is_signed {
                bx.sdiv(lhs, rhs)
            } else {
                bx.udiv(lhs, rhs)
            },
            mir::BinOp::Rem => if is_float {
                bx.frem(lhs, rhs)
            } else if is_signed {
                bx.srem(lhs, rhs)
            } else {
                bx.urem(lhs, rhs)
            },
            mir::BinOp::BitOr => bx.or(lhs, rhs),
            mir::BinOp::BitAnd => bx.and(lhs, rhs),
            mir::BinOp::BitXor => bx.xor(lhs, rhs),
            mir::BinOp::Offset => bx.inbounds_gep(lhs, &[rhs]),
            mir::BinOp::Shl => common::build_unchecked_lshift(bx, lhs, rhs),
            mir::BinOp::Shr => common::build_unchecked_rshift(bx, input_ty, lhs, rhs),
            mir::BinOp::Ne | mir::BinOp::Lt | mir::BinOp::Gt |
            mir::BinOp::Eq | mir::BinOp::Le | mir::BinOp::Ge => if is_float {
                bx.fcmp(
                    base::bin_op_to_fcmp_predicate(op.to_hir_binop()),
                    lhs, rhs
                )
            } else {
                bx.icmp(
                    base::bin_op_to_icmp_predicate(op.to_hir_binop(), is_signed),
                    lhs, rhs
                )
            }
        }
    }

    pub fn codegen_fat_ptr_binop(
        &mut self,
        bx: &mut Bx,
        op: mir::BinOp,
        lhs_addr: Bx::Value,
        lhs_extra: Bx::Value,
        rhs_addr: Bx::Value,
        rhs_extra: Bx::Value,
        _input_ty: Ty<'tcx>,
    ) -> Bx::Value {
        match op {
            mir::BinOp::Eq => {
                let lhs = bx.icmp(IntPredicate::IntEQ, lhs_addr, rhs_addr);
                let rhs = bx.icmp(IntPredicate::IntEQ, lhs_extra, rhs_extra);
                bx.and(lhs, rhs)
            }
            mir::BinOp::Ne => {
                let lhs = bx.icmp(IntPredicate::IntNE, lhs_addr, rhs_addr);
                let rhs = bx.icmp(IntPredicate::IntNE, lhs_extra, rhs_extra);
                bx.or(lhs, rhs)
            }
            mir::BinOp::Le | mir::BinOp::Lt |
            mir::BinOp::Ge | mir::BinOp::Gt => {
                // a OP b ~ a.0 STRICT(OP) b.0 | (a.0 == b.0 && a.1 OP a.1)
                let (op, strict_op) = match op {
                    mir::BinOp::Lt => (IntPredicate::IntULT, IntPredicate::IntULT),
                    mir::BinOp::Le => (IntPredicate::IntULE, IntPredicate::IntULT),
                    mir::BinOp::Gt => (IntPredicate::IntUGT, IntPredicate::IntUGT),
                    mir::BinOp::Ge => (IntPredicate::IntUGE, IntPredicate::IntUGT),
                    _ => bug!(),
                };
                let lhs = bx.icmp(strict_op, lhs_addr, rhs_addr);
                let and_lhs = bx.icmp(IntPredicate::IntEQ, lhs_addr, rhs_addr);
                let and_rhs = bx.icmp(op, lhs_extra, rhs_extra);
                let rhs = bx.and(and_lhs, and_rhs);
                bx.or(lhs, rhs)
            }
            _ => {
                bug!("unexpected fat ptr binop");
            }
        }
    }

    pub fn codegen_scalar_checked_binop(
        &mut self,
        bx: &mut Bx,
        op: mir::BinOp,
        lhs: Bx::Value,
        rhs: Bx::Value,
        input_ty: Ty<'tcx>
    ) -> OperandValue<Bx::Value> {
        // This case can currently arise only from functions marked
        // with #[rustc_inherit_overflow_checks] and inlined from
        // another crate (mostly core::num generic/#[inline] fns),
        // while the current crate doesn't use overflow checks.
        if !bx.cx().check_overflow() {
            let val = self.codegen_scalar_binop(bx, op, lhs, rhs, input_ty);
            return OperandValue::Pair(val, bx.cx().const_bool(false));
        }

        let (val, of) = match op {
            // These are checked using intrinsics
            mir::BinOp::Add | mir::BinOp::Sub | mir::BinOp::Mul => {
                let oop = match op {
                    mir::BinOp::Add => OverflowOp::Add,
                    mir::BinOp::Sub => OverflowOp::Sub,
                    mir::BinOp::Mul => OverflowOp::Mul,
                    _ => unreachable!()
                };
                bx.checked_binop(oop, input_ty, lhs, rhs)
            }
            mir::BinOp::Shl | mir::BinOp::Shr => {
                let lhs_llty = bx.cx().val_ty(lhs);
                let rhs_llty = bx.cx().val_ty(rhs);
                let invert_mask = common::shift_mask_val(bx, lhs_llty, rhs_llty, true);
                let outer_bits = bx.and(rhs, invert_mask);

                let of = bx.icmp(IntPredicate::IntNE, outer_bits, bx.cx().const_null(rhs_llty));
                let val = self.codegen_scalar_binop(bx, op, lhs, rhs, input_ty);

                (val, of)
            }
            _ => {
                bug!("Operator `{:?}` is not a checkable operator", op)
            }
        };

        OperandValue::Pair(val, of)
    }
}

impl<'a, 'tcx, Bx: BuilderMethods<'a, 'tcx>> FunctionCx<'a, 'tcx, Bx> {
    pub fn rvalue_creates_operand(&self, rvalue: &mir::Rvalue<'tcx>, span: Span) -> bool {
        match *rvalue {
            mir::Rvalue::Ref(..) |
            mir::Rvalue::Len(..) |
            mir::Rvalue::Cast(..) | // (*)
            mir::Rvalue::BinaryOp(..) |
            mir::Rvalue::CheckedBinaryOp(..) |
            mir::Rvalue::UnaryOp(..) |
            mir::Rvalue::Discriminant(..) |
            mir::Rvalue::NullaryOp(..) |
            mir::Rvalue::Use(..) => // (*)
                true,
            mir::Rvalue::Repeat(..) |
            mir::Rvalue::Aggregate(..) => {
                let ty = rvalue.ty(self.mir, self.cx.tcx());
                let ty = self.monomorphize(&ty);
                self.cx.spanned_layout_of(ty, span).is_zst()
            }
        }

        // (*) this is only true if the type is suitable
    }
}

fn cast_int_to_float<'a, 'tcx, Bx: BuilderMethods<'a, 'tcx>>(
    bx: &mut Bx,
    signed: bool,
    x: Bx::Value,
    int_ty: Bx::Type,
    float_ty: Bx::Type
) -> Bx::Value {
    // Most integer types, even i128, fit into [-f32::MAX, f32::MAX] after rounding.
    // It's only u128 -> f32 that can cause overflows (i.e., should yield infinity).
    // LLVM's uitofp produces undef in those cases, so we manually check for that case.
    let is_u128_to_f32 = !signed &&
        bx.cx().int_width(int_ty) == 128 &&
        bx.cx().float_width(float_ty) == 32;
    if is_u128_to_f32 {
        // All inputs greater or equal to (f32::MAX + 0.5 ULP) are rounded to infinity,
        // and for everything else LLVM's uitofp works just fine.
        use rustc_apfloat::ieee::Single;
        const MAX_F32_PLUS_HALF_ULP: u128 = ((1 << (Single::PRECISION + 1)) - 1)
                                            << (Single::MAX_EXP - Single::PRECISION as i16);
        let max = bx.cx().const_uint_big(int_ty, MAX_F32_PLUS_HALF_ULP);
        let overflow = bx.icmp(IntPredicate::IntUGE, x, max);
        let infinity_bits = bx.cx().const_u32(ieee::Single::INFINITY.to_bits() as u32);
        let infinity = bx.bitcast(infinity_bits, float_ty);
        let fp = bx.uitofp(x, float_ty);
        bx.select(overflow, infinity, fp)
    } else {
        if signed {
            bx.sitofp(x, float_ty)
        } else {
            bx.uitofp(x, float_ty)
        }
    }
}

fn cast_float_to_int<'a, 'tcx, Bx: BuilderMethods<'a, 'tcx>>(
    bx: &mut Bx,
    signed: bool,
    x: Bx::Value,
    float_ty: Bx::Type,
    int_ty: Bx::Type
) -> Bx::Value {
    let fptosui_result = if signed {
        bx.fptosi(x, int_ty)
    } else {
        bx.fptoui(x, int_ty)
    };

    if !bx.cx().sess().opts.debugging_opts.saturating_float_casts {
        return fptosui_result;
    }

    let int_width = bx.cx().int_width(int_ty);
    let float_width = bx.cx().float_width(float_ty);
    // LLVM's fpto[su]i returns undef when the input x is infinite, NaN, or does not fit into the
    // destination integer type after rounding towards zero. This `undef` value can cause UB in
    // safe code (see issue #10184), so we implement a saturating conversion on top of it:
    // Semantically, the mathematical value of the input is rounded towards zero to the next
    // mathematical integer, and then the result is clamped into the range of the destination
    // integer type. Positive and negative infinity are mapped to the maximum and minimum value of
    // the destination integer type. NaN is mapped to 0.
    //
    // Define f_min and f_max as the largest and smallest (finite) floats that are exactly equal to
    // a value representable in int_ty.
    // They are exactly equal to int_ty::{MIN,MAX} if float_ty has enough significand bits.
    // Otherwise, int_ty::MAX must be rounded towards zero, as it is one less than a power of two.
    // int_ty::MIN, however, is either zero or a negative power of two and is thus exactly
    // representable. Note that this only works if float_ty's exponent range is sufficiently large.
    // f16 or 256 bit integers would break this property. Right now the smallest float type is f32
    // with exponents ranging up to 127, which is barely enough for i128::MIN = -2^127.
    // On the other hand, f_max works even if int_ty::MAX is greater than float_ty::MAX. Because
    // we're rounding towards zero, we just get float_ty::MAX (which is always an integer).
    // This already happens today with u128::MAX = 2^128 - 1 > f32::MAX.
    let int_max = |signed: bool, int_width: u64| -> u128 {
        let shift_amount = 128 - int_width;
        if signed {
            i128::MAX as u128 >> shift_amount
        } else {
            u128::MAX >> shift_amount
        }
    };
    let int_min = |signed: bool, int_width: u64| -> i128 {
        if signed {
            i128::MIN >> (128 - int_width)
        } else {
            0
        }
    };

    let compute_clamp_bounds_single =
    |signed: bool, int_width: u64| -> (u128, u128) {
        let rounded_min = ieee::Single::from_i128_r(int_min(signed, int_width), Round::TowardZero);
        assert_eq!(rounded_min.status, Status::OK);
        let rounded_max = ieee::Single::from_u128_r(int_max(signed, int_width), Round::TowardZero);
        assert!(rounded_max.value.is_finite());
        (rounded_min.value.to_bits(), rounded_max.value.to_bits())
    };
    let compute_clamp_bounds_double =
    |signed: bool, int_width: u64| -> (u128, u128) {
        let rounded_min = ieee::Double::from_i128_r(int_min(signed, int_width), Round::TowardZero);
        assert_eq!(rounded_min.status, Status::OK);
        let rounded_max = ieee::Double::from_u128_r(int_max(signed, int_width), Round::TowardZero);
        assert!(rounded_max.value.is_finite());
        (rounded_min.value.to_bits(), rounded_max.value.to_bits())
    };

    let mut float_bits_to_llval = |bits| {
        let bits_llval = match float_width  {
            32 => bx.cx().const_u32(bits as u32),
            64 => bx.cx().const_u64(bits as u64),
            n => bug!("unsupported float width {}", n),
        };
        bx.bitcast(bits_llval, float_ty)
    };
    let (f_min, f_max) = match float_width {
        32 => compute_clamp_bounds_single(signed, int_width),
        64 => compute_clamp_bounds_double(signed, int_width),
        n => bug!("unsupported float width {}", n),
    };
    let f_min = float_bits_to_llval(f_min);
    let f_max = float_bits_to_llval(f_max);
    // To implement saturation, we perform the following steps:
    //
    // 1. Cast x to an integer with fpto[su]i. This may result in undef.
    // 2. Compare x to f_min and f_max, and use the comparison results to select:
    //  a) int_ty::MIN if x < f_min or x is NaN
    //  b) int_ty::MAX if x > f_max
    //  c) the result of fpto[su]i otherwise
    // 3. If x is NaN, return 0.0, otherwise return the result of step 2.
    //
    // This avoids resulting undef because values in range [f_min, f_max] by definition fit into the
    // destination type. It creates an undef temporary, but *producing* undef is not UB. Our use of
    // undef does not introduce any non-determinism either.
    // More importantly, the above procedure correctly implements saturating conversion.
    // Proof (sketch):
    // If x is NaN, 0 is returned by definition.
    // Otherwise, x is finite or infinite and thus can be compared with f_min and f_max.
    // This yields three cases to consider:
    // (1) if x in [f_min, f_max], the result of fpto[su]i is returned, which agrees with
    //     saturating conversion for inputs in that range.
    // (2) if x > f_max, then x is larger than int_ty::MAX. This holds even if f_max is rounded
    //     (i.e., if f_max < int_ty::MAX) because in those cases, nextUp(f_max) is already larger
    //     than int_ty::MAX. Because x is larger than int_ty::MAX, the return value of int_ty::MAX
    //     is correct.
    // (3) if x < f_min, then x is smaller than int_ty::MIN. As shown earlier, f_min exactly equals
    //     int_ty::MIN and therefore the return value of int_ty::MIN is correct.
    // QED.

    // Step 1 was already performed above.

    // Step 2: We use two comparisons and two selects, with %s1 being the result:
    //     %less_or_nan = fcmp ult %x, %f_min
    //     %greater = fcmp olt %x, %f_max
    //     %s0 = select %less_or_nan, int_ty::MIN, %fptosi_result
    //     %s1 = select %greater, int_ty::MAX, %s0
    // Note that %less_or_nan uses an *unordered* comparison. This comparison is true if the
    // operands are not comparable (i.e., if x is NaN). The unordered comparison ensures that s1
    // becomes int_ty::MIN if x is NaN.
    // Performance note: Unordered comparison can be lowered to a "flipped" comparison and a
    // negation, and the negation can be merged into the select. Therefore, it not necessarily any
    // more expensive than a ordered ("normal") comparison. Whether these optimizations will be
    // performed is ultimately up to the backend, but at least x86 does perform them.
    let less_or_nan = bx.fcmp(RealPredicate::RealULT, x, f_min);
    let greater = bx.fcmp(RealPredicate::RealOGT, x, f_max);
    let int_max = bx.cx().const_uint_big(int_ty, int_max(signed, int_width));
    let int_min = bx.cx().const_uint_big(int_ty, int_min(signed, int_width) as u128);
    let s0 = bx.select(less_or_nan, int_min, fptosui_result);
    let s1 = bx.select(greater, int_max, s0);

    // Step 3: NaN replacement.
    // For unsigned types, the above step already yielded int_ty::MIN == 0 if x is NaN.
    // Therefore we only need to execute this step for signed integer types.
    if signed {
        // LLVM has no isNaN predicate, so we use (x == x) instead
        let zero = bx.cx().const_uint(int_ty, 0);
        let cmp = bx.fcmp(RealPredicate::RealOEQ, x, x);
        bx.select(cmp, s1, zero)
    } else {
        s1
    }
}
