//! See docs in build/expr/mod.rs

use crate::build::expr::category::{Category, RvalueFunc};
use crate::build::{BlockAnd, BlockAndExtension, BlockFrame, Builder};
use crate::hair::*;
use rustc::mir::*;
use rustc::ty::{self, CanonicalUserTypeAnnotation};
use rustc_data_structures::fx::FxHashMap;
use syntax_pos::symbol::sym;

use rustc_target::spec::abi::Abi;

impl<'a, 'tcx> Builder<'a, 'tcx> {
    /// Compile `expr`, storing the result into `destination`, which
    /// is assumed to be uninitialized.
    pub fn into_expr(
        &mut self,
        destination: &Place<'tcx>,
        mut block: BasicBlock,
        expr: Expr<'tcx>,
    ) -> BlockAnd<()> {
        debug!(
            "into_expr(destination={:?}, block={:?}, expr={:?})",
            destination, block, expr
        );

        // since we frequently have to reference `self` from within a
        // closure, where `self` would be shadowed, it's easier to
        // just use the name `this` uniformly
        let this = self;
        let expr_span = expr.span;
        let source_info = this.source_info(expr_span);

        let expr_is_block_or_scope = match expr.kind {
            ExprKind::Block { .. } => true,
            ExprKind::Scope { .. } => true,
            _ => false,
        };

        if !expr_is_block_or_scope {
            this.block_context.push(BlockFrame::SubExpr);
        }

        let block_and = match expr.kind {
            ExprKind::Scope {
                region_scope,
                lint_level,
                value,
            } => {
                let region_scope = (region_scope, source_info);
                this.in_scope(region_scope, lint_level, |this| {
                    this.into(destination, block, value)
                })
            }
            ExprKind::Block { body: ast_block } => {
                this.ast_block(destination, block, ast_block, source_info)
            }
            ExprKind::Match { scrutinee, arms } => {
                this.match_expr(destination, expr_span, block, scrutinee, arms)
            }
            ExprKind::NeverToAny { source } => {
                let source = this.hir.mirror(source);
                let is_call = match source.kind {
                    ExprKind::Call { .. } => true,
                    _ => false,
                };

                unpack!(block = this.as_local_rvalue(block, source));

                // This is an optimization. If the expression was a call then we already have an
                // unreachable block. Don't bother to terminate it and create a new one.
                if is_call {
                    block.unit()
                } else {
                    this.cfg
                        .terminate(block, source_info, TerminatorKind::Unreachable);
                    let end_block = this.cfg.start_new_block();
                    end_block.unit()
                }
            }
            ExprKind::LogicalOp { op, lhs, rhs } => {
                // And:
                //
                // [block: If(lhs)] -true-> [else_block: If(rhs)] -true-> [true_block]
                //        |                          | (false)
                //        +----------false-----------+------------------> [false_block]
                //
                // Or:
                //
                // [block: If(lhs)] -false-> [else_block: If(rhs)] -true-> [true_block]
                //        | (true)                   | (false)
                //  [true_block]               [false_block]

                let (true_block, false_block, mut else_block, join_block) = (
                    this.cfg.start_new_block(),
                    this.cfg.start_new_block(),
                    this.cfg.start_new_block(),
                    this.cfg.start_new_block(),
                );

                let lhs = unpack!(block = this.as_local_operand(block, lhs));
                let blocks = match op {
                    LogicalOp::And => (else_block, false_block),
                    LogicalOp::Or => (true_block, else_block),
                };
                let term = TerminatorKind::if_(this.hir.tcx(), lhs, blocks.0, blocks.1);
                this.cfg.terminate(block, source_info, term);

                let rhs = unpack!(else_block = this.as_local_operand(else_block, rhs));
                let term = TerminatorKind::if_(this.hir.tcx(), rhs, true_block, false_block);
                this.cfg.terminate(else_block, source_info, term);

                this.cfg.push_assign_constant(
                    true_block,
                    source_info,
                    destination,
                    Constant {
                        span: expr_span,
                        user_ty: None,
                        literal: this.hir.true_literal(),
                    },
                );

                this.cfg.push_assign_constant(
                    false_block,
                    source_info,
                    destination,
                    Constant {
                        span: expr_span,
                        user_ty: None,
                        literal: this.hir.false_literal(),
                    },
                );

                this.cfg.terminate(
                    true_block,
                    source_info,
                    TerminatorKind::Goto { target: join_block },
                );
                this.cfg.terminate(
                    false_block,
                    source_info,
                    TerminatorKind::Goto { target: join_block },
                );

                join_block.unit()
            }
            ExprKind::Loop { body } => {
                // [block]
                //    |
                //   [loop_block] -> [body_block] -/eval. body/-> [body_block_end]
                //    |        ^                                         |
                // false link  |                                         |
                //    |        +-----------------------------------------+
                //    +-> [diverge_cleanup]
                // The false link is required to make sure borrowck considers unwinds through the
                // body, even when the exact code in the body cannot unwind

                let loop_block = this.cfg.start_new_block();
                let exit_block = this.cfg.start_new_block();

                // start the loop
                this.cfg.terminate(
                    block,
                    source_info,
                    TerminatorKind::Goto { target: loop_block },
                );

                this.in_breakable_scope(
                    Some(loop_block),
                    exit_block,
                    destination.clone(),
                    move |this| {
                        // conduct the test, if necessary
                        let body_block = this.cfg.start_new_block();
                        let diverge_cleanup = this.diverge_cleanup();
                        this.cfg.terminate(
                            loop_block,
                            source_info,
                            TerminatorKind::FalseUnwind {
                                real_target: body_block,
                                unwind: Some(diverge_cleanup),
                            },
                        );

                        // The “return” value of the loop body must always be an unit. We therefore
                        // introduce a unit temporary as the destination for the loop body.
                        let tmp = this.get_unit_temp();
                        // Execute the body, branching back to the test.
                        let body_block_end = unpack!(this.into(&tmp, body_block, body));
                        this.cfg.terminate(
                            body_block_end,
                            source_info,
                            TerminatorKind::Goto { target: loop_block },
                        );
                    },
                );
                exit_block.unit()
            }
            ExprKind::Call { ty, fun, args, from_hir_call } => {
                let intrinsic = match ty.kind {
                    ty::FnDef(def_id, _) => {
                        let f = ty.fn_sig(this.hir.tcx());
                        if f.abi() == Abi::RustIntrinsic || f.abi() == Abi::PlatformIntrinsic {
                            Some(this.hir.tcx().item_name(def_id))
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let fun = unpack!(block = this.as_local_operand(block, fun));
                if let Some(sym::move_val_init) = intrinsic {
                    // `move_val_init` has "magic" semantics - the second argument is
                    // always evaluated "directly" into the first one.

                    let mut args = args.into_iter();
                    let ptr = args.next().expect("0 arguments to `move_val_init`");
                    let val = args.next().expect("1 argument to `move_val_init`");
                    assert!(args.next().is_none(), ">2 arguments to `move_val_init`");

                    let ptr = this.hir.mirror(ptr);
                    let ptr_ty = ptr.ty;
                    // Create an *internal* temp for the pointer, so that unsafety
                    // checking won't complain about the raw pointer assignment.
                    let ptr_temp = this.local_decls.push(LocalDecl {
                        mutability: Mutability::Mut,
                        ty: ptr_ty,
                        user_ty: UserTypeProjections::none(),
                        name: None,
                        source_info,
                        visibility_scope: source_info.scope,
                        internal: true,
                        local_info: LocalInfo::Other,
                        is_block_tail: None,
                    });
                    let ptr_temp = Place::from(ptr_temp);
                    let block = unpack!(this.into(&ptr_temp, block, ptr));
                    this.into(&this.hir.tcx().mk_place_deref(ptr_temp), block, val)
                } else {
                    let args: Vec<_> = args
                        .into_iter()
                        .map(|arg| unpack!(block = this.as_local_operand(block, arg)))
                        .collect();

                    let success = this.cfg.start_new_block();
                    let cleanup = this.diverge_cleanup();

                    this.record_operands_moved(&args);

                    this.cfg.terminate(
                        block,
                        source_info,
                        TerminatorKind::Call {
                            func: fun,
                            args,
                            cleanup: Some(cleanup),
                            // FIXME(varkor): replace this with an uninhabitedness-based check.
                            // This requires getting access to the current module to call
                            // `tcx.is_ty_uninhabited_from`, which is currently tricky to do.
                            destination: if expr.ty.is_never() {
                                None
                            } else {
                                Some((destination.clone(), success))
                            },
                            from_hir_call,
                        },
                    );
                    success.unit()
                }
            }
            ExprKind::Use { source } => {
                this.into(destination, block, source)
            }
            ExprKind::Borrow { arg, borrow_kind } => {
                // We don't do this in `as_rvalue` because we use `as_place`
                // for borrow expressions, so we cannot create an `RValue` that
                // remains valid across user code. `as_rvalue` is usually called
                // by this method anyway, so this shouldn't cause too many
                // unnecessary temporaries.
                let arg_place = match borrow_kind {
                    BorrowKind::Shared => unpack!(block = this.as_read_only_place(block, arg)),
                    _ => unpack!(block = this.as_place(block, arg)),
                };
                let borrow = Rvalue::Ref(
                    this.hir.tcx().lifetimes.re_erased,
                    borrow_kind,
                    arg_place,
                );
                this.cfg.push_assign(block, source_info, destination, borrow);
                block.unit()
            }
            ExprKind::Adt {
                adt_def,
                variant_index,
                substs,
                user_ty,
                fields,
                base,
            } => {
                // See the notes for `ExprKind::Array` in `as_rvalue` and for
                // `ExprKind::Borrow` above.
                let is_union = adt_def.is_union();
                let active_field_index = if is_union {
                    Some(fields[0].name.index())
                } else {
                    None
                };

                let scope =  this.local_scope();

                // first process the set of fields that were provided
                // (evaluating them in order given by user)
                let fields_map: FxHashMap<_, _> = fields
                    .into_iter()
                    .map(|f| {
                        (
                            f.name,
                            unpack!(block = this.as_operand(block, scope, f.expr)),
                        )
                    }).collect();

                let field_names = this.hir.all_fields(adt_def, variant_index);

                let fields = if let Some(FruInfo { base, field_types }) = base {
                    let base = unpack!(block = this.as_place(block, base));

                    // MIR does not natively support FRU, so for each
                    // base-supplied field, generate an operand that
                    // reads it from the base.
                    field_names
                        .into_iter()
                        .zip(field_types.into_iter())
                        .map(|(n, ty)| match fields_map.get(&n) {
                            Some(v) => v.clone(),
                            None => this.consume_by_copy_or_move(
                                this.hir.tcx().mk_place_field(base.clone(), n, ty),
                            ),
                        }).collect()
                } else {
                    field_names
                        .iter()
                        .filter_map(|n| fields_map.get(n).cloned())
                        .collect()
                };

                let inferred_ty = expr.ty;
                let user_ty = user_ty.map(|ty| {
                    this.canonical_user_type_annotations.push(CanonicalUserTypeAnnotation {
                        span: source_info.span,
                        user_ty: ty,
                        inferred_ty,
                    })
                });
                let adt = box AggregateKind::Adt(
                    adt_def,
                    variant_index,
                    substs,
                    user_ty,
                    active_field_index,
                );
                this.cfg.push_assign(
                    block,
                    source_info,
                    destination,
                    Rvalue::Aggregate(adt, fields)
                );
                block.unit()
            }


            // These cases don't actually need a destination
            ExprKind::Assign { .. }
            | ExprKind::AssignOp { .. }
            | ExprKind::Continue { .. }
            | ExprKind::Break { .. }
            | ExprKind::InlineAsm { .. }
            | ExprKind::Return { .. } => {
                unpack!(block = this.stmt_expr(block, expr, None));
                this.cfg.push_assign_unit(block, source_info, destination);
                block.unit()
            }

            // Avoid creating a temporary
            ExprKind::VarRef { .. } |
            ExprKind::SelfRef |
            ExprKind::PlaceTypeAscription { .. } |
            ExprKind::ValueTypeAscription { .. } => {
                debug_assert!(Category::of(&expr.kind) == Some(Category::Place));

                let place = unpack!(block = this.as_place(block, expr));
                let rvalue = Rvalue::Use(this.consume_by_copy_or_move(place));
                this.cfg
                    .push_assign(block, source_info, destination, rvalue);
                block.unit()
            }
            ExprKind::Index { .. } | ExprKind::Deref { .. } | ExprKind::Field { .. } => {
                debug_assert!(Category::of(&expr.kind) == Some(Category::Place));

                // Create a "fake" temporary variable so that we check that the
                // value is Sized. Usually, this is caught in type checking, but
                // in the case of box expr there is no such check.
                if !destination.projection.is_empty() {
                    this.local_decls
                        .push(LocalDecl::new_temp(expr.ty, expr.span));
                }

                debug_assert!(Category::of(&expr.kind) == Some(Category::Place));

                let place = unpack!(block = this.as_place(block, expr));
                let rvalue = Rvalue::Use(this.consume_by_copy_or_move(place));
                this.cfg
                    .push_assign(block, source_info, destination, rvalue);
                block.unit()
            }

            // these are the cases that are more naturally handled by some other mode
            ExprKind::Unary { .. }
            | ExprKind::Binary { .. }
            | ExprKind::Box { .. }
            | ExprKind::Cast { .. }
            | ExprKind::Pointer { .. }
            | ExprKind::Repeat { .. }
            | ExprKind::Array { .. }
            | ExprKind::Tuple { .. }
            | ExprKind::Closure { .. }
            | ExprKind::Literal { .. }
            | ExprKind::StaticRef { .. }
            | ExprKind::Yield { .. } => {
                debug_assert!(match Category::of(&expr.kind).unwrap() {
                    // should be handled above
                    Category::Rvalue(RvalueFunc::Into) => false,

                    // must be handled above or else we get an
                    // infinite loop in the builder; see
                    // e.g., `ExprKind::VarRef` above
                    Category::Place => false,

                    _ => true,
                });

                let rvalue = unpack!(block = this.as_local_rvalue(block, expr));
                this.cfg.push_assign(block, source_info, destination, rvalue);
                block.unit()
            }
        };

        if !expr_is_block_or_scope {
            let popped = this.block_context.pop();
            assert!(popped.is_some());
        }

        block_and
    }
}
