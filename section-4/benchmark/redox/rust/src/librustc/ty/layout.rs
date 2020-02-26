use crate::session::{self, DataTypeKind};
use crate::ty::{self, Ty, TyCtxt, TypeFoldable, ReprOptions, subst::SubstsRef};

use syntax::ast::{self, Ident, IntTy, UintTy};
use syntax::attr;
use syntax_pos::DUMMY_SP;

use std::cmp;
use std::fmt;
use std::i128;
use std::iter;
use std::mem;
use std::ops::Bound;

use crate::hir;
use crate::ich::StableHashingContext;
use crate::mir::{GeneratorLayout, GeneratorSavedLocal};
use crate::ty::subst::Subst;
use rustc_index::bit_set::BitSet;
use rustc_data_structures::stable_hasher::{HashStable, StableHasher};
use rustc_index::vec::{IndexVec, Idx};

pub use rustc_target::abi::*;
use rustc_target::spec::{HasTargetSpec, abi::Abi as SpecAbi};
use rustc_target::abi::call::{
    ArgAttribute, ArgAttributes, ArgAbi, Conv, FnAbi, PassMode, Reg, RegKind
};

pub trait IntegerExt {
    fn to_ty<'tcx>(&self, tcx: TyCtxt<'tcx>, signed: bool) -> Ty<'tcx>;
    fn from_attr<C: HasDataLayout>(cx: &C, ity: attr::IntType) -> Integer;
    fn repr_discr<'tcx>(
        tcx: TyCtxt<'tcx>,
        ty: Ty<'tcx>,
        repr: &ReprOptions,
        min: i128,
        max: i128,
    ) -> (Integer, bool);
}

impl IntegerExt for Integer {
    fn to_ty<'tcx>(&self, tcx: TyCtxt<'tcx>, signed: bool) -> Ty<'tcx> {
        match (*self, signed) {
            (I8, false) => tcx.types.u8,
            (I16, false) => tcx.types.u16,
            (I32, false) => tcx.types.u32,
            (I64, false) => tcx.types.u64,
            (I128, false) => tcx.types.u128,
            (I8, true) => tcx.types.i8,
            (I16, true) => tcx.types.i16,
            (I32, true) => tcx.types.i32,
            (I64, true) => tcx.types.i64,
            (I128, true) => tcx.types.i128,
        }
    }

    /// Gets the Integer type from an attr::IntType.
    fn from_attr<C: HasDataLayout>(cx: &C, ity: attr::IntType) -> Integer {
        let dl = cx.data_layout();

        match ity {
            attr::SignedInt(IntTy::I8) | attr::UnsignedInt(UintTy::U8) => I8,
            attr::SignedInt(IntTy::I16) | attr::UnsignedInt(UintTy::U16) => I16,
            attr::SignedInt(IntTy::I32) | attr::UnsignedInt(UintTy::U32) => I32,
            attr::SignedInt(IntTy::I64) | attr::UnsignedInt(UintTy::U64) => I64,
            attr::SignedInt(IntTy::I128) | attr::UnsignedInt(UintTy::U128) => I128,
            attr::SignedInt(IntTy::Isize) | attr::UnsignedInt(UintTy::Usize) => {
                dl.ptr_sized_integer()
            }
        }
    }

    /// Finds the appropriate Integer type and signedness for the given
    /// signed discriminant range and #[repr] attribute.
    /// N.B.: u128 values above i128::MAX will be treated as signed, but
    /// that shouldn't affect anything, other than maybe debuginfo.
    fn repr_discr<'tcx>(
        tcx: TyCtxt<'tcx>,
        ty: Ty<'tcx>,
        repr: &ReprOptions,
        min: i128,
        max: i128,
    ) -> (Integer, bool) {
        // Theoretically, negative values could be larger in unsigned representation
        // than the unsigned representation of the signed minimum. However, if there
        // are any negative values, the only valid unsigned representation is u128
        // which can fit all i128 values, so the result remains unaffected.
        let unsigned_fit = Integer::fit_unsigned(cmp::max(min as u128, max as u128));
        let signed_fit = cmp::max(Integer::fit_signed(min), Integer::fit_signed(max));

        let mut min_from_extern = None;
        let min_default = I8;

        if let Some(ity) = repr.int {
            let discr = Integer::from_attr(&tcx, ity);
            let fit = if ity.is_signed() { signed_fit } else { unsigned_fit };
            if discr < fit {
                bug!("Integer::repr_discr: `#[repr]` hint too small for \
                      discriminant range of enum `{}", ty)
            }
            return (discr, ity.is_signed());
        }

        if repr.c() {
            match &tcx.sess.target.target.arch[..] {
                // WARNING: the ARM EABI has two variants; the one corresponding
                // to `at_least == I32` appears to be used on Linux and NetBSD,
                // but some systems may use the variant corresponding to no
                // lower bound. However, we don't run on those yet...?
                "arm" => min_from_extern = Some(I32),
                _ => min_from_extern = Some(I32),
            }
        }

        let at_least = min_from_extern.unwrap_or(min_default);

        // If there are no negative values, we can use the unsigned fit.
        if min >= 0 {
            (cmp::max(unsigned_fit, at_least), false)
        } else {
            (cmp::max(signed_fit, at_least), true)
        }
    }
}

pub trait PrimitiveExt {
    fn to_ty<'tcx>(&self, tcx: TyCtxt<'tcx>) -> Ty<'tcx>;
    fn to_int_ty<'tcx>(&self, tcx: TyCtxt<'tcx>) -> Ty<'tcx>;
}

impl PrimitiveExt for Primitive {
    fn to_ty<'tcx>(&self, tcx: TyCtxt<'tcx>) -> Ty<'tcx> {
        match *self {
            Int(i, signed) => i.to_ty(tcx, signed),
            F32 => tcx.types.f32,
            F64 => tcx.types.f64,
            Pointer => tcx.mk_mut_ptr(tcx.mk_unit()),
        }
    }

    /// Return an *integer* type matching this primitive.
    /// Useful in particular when dealing with enum discriminants.
    fn to_int_ty(&self, tcx: TyCtxt<'tcx>) -> Ty<'tcx> {
        match *self {
            Int(i, signed) => i.to_ty(tcx, signed),
            Pointer => tcx.types.usize,
            F32 | F64 => bug!("floats do not have an int type"),
        }
    }
}

/// The first half of a fat pointer.
///
/// - For a trait object, this is the address of the box.
/// - For a slice, this is the base address.
pub const FAT_PTR_ADDR: usize = 0;

/// The second half of a fat pointer.
///
/// - For a trait object, this is the address of the vtable.
/// - For a slice, this is the length.
pub const FAT_PTR_EXTRA: usize = 1;

#[derive(Copy, Clone, Debug, RustcEncodable, RustcDecodable)]
pub enum LayoutError<'tcx> {
    Unknown(Ty<'tcx>),
    SizeOverflow(Ty<'tcx>)
}

impl<'tcx> fmt::Display for LayoutError<'tcx> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            LayoutError::Unknown(ty) => {
                write!(f, "the type `{:?}` has an unknown layout", ty)
            }
            LayoutError::SizeOverflow(ty) => {
                write!(f, "the type `{:?}` is too big for the current architecture", ty)
            }
        }
    }
}

fn layout_raw<'tcx>(
    tcx: TyCtxt<'tcx>,
    query: ty::ParamEnvAnd<'tcx, Ty<'tcx>>,
) -> Result<&'tcx LayoutDetails, LayoutError<'tcx>> {
    ty::tls::with_related_context(tcx, move |icx| {
        let rec_limit = *tcx.sess.recursion_limit.get();
        let (param_env, ty) = query.into_parts();

        if icx.layout_depth > rec_limit {
            tcx.sess.fatal(
                &format!("overflow representing the type `{}`", ty));
        }

        // Update the ImplicitCtxt to increase the layout_depth
        let icx = ty::tls::ImplicitCtxt {
            layout_depth: icx.layout_depth + 1,
            ..icx.clone()
        };

        ty::tls::enter_context(&icx, |_| {
            let cx = LayoutCx { tcx, param_env };
            let layout = cx.layout_raw_uncached(ty);
            // Type-level uninhabitedness should always imply ABI uninhabitedness.
            if let Ok(layout) = layout {
                if ty.conservative_is_privately_uninhabited(tcx) {
                    assert!(layout.abi.is_uninhabited());
                }
            }
            layout
        })
    })
}

pub fn provide(providers: &mut ty::query::Providers<'_>) {
    *providers = ty::query::Providers {
        layout_raw,
        ..*providers
    };
}

pub struct LayoutCx<'tcx, C> {
    pub tcx: C,
    pub param_env: ty::ParamEnv<'tcx>,
}

#[derive(Copy, Clone, Debug)]
enum StructKind {
    /// A tuple, closure, or univariant which cannot be coerced to unsized.
    AlwaysSized,
    /// A univariant, the last field of which may be coerced to unsized.
    MaybeUnsized,
    /// A univariant, but with a prefix of an arbitrary size & alignment (e.g., enum tag).
    Prefixed(Size, Align),
}

// Invert a bijective mapping, i.e. `invert(map)[y] = x` if `map[x] = y`.
// This is used to go between `memory_index` (source field order to memory order)
// and `inverse_memory_index` (memory order to source field order).
// See also `FieldPlacement::Arbitrary::memory_index` for more details.
// FIXME(eddyb) build a better abstraction for permutations, if possible.
fn invert_mapping(map: &[u32]) -> Vec<u32> {
    let mut inverse = vec![0; map.len()];
    for i in 0..map.len() {
        inverse[map[i] as usize] = i as u32;
    }
    inverse
}

impl<'tcx> LayoutCx<'tcx, TyCtxt<'tcx>> {
    fn scalar_pair(&self, a: Scalar, b: Scalar) -> LayoutDetails {
        let dl = self.data_layout();
        let b_align = b.value.align(dl);
        let align = a.value.align(dl).max(b_align).max(dl.aggregate_align);
        let b_offset = a.value.size(dl).align_to(b_align.abi);
        let size = (b_offset + b.value.size(dl)).align_to(align.abi);

        // HACK(nox): We iter on `b` and then `a` because `max_by_key`
        // returns the last maximum.
        let largest_niche = Niche::from_scalar(dl, b_offset, b.clone())
            .into_iter()
            .chain(Niche::from_scalar(dl, Size::ZERO, a.clone()))
            .max_by_key(|niche| niche.available(dl));

        LayoutDetails {
            variants: Variants::Single { index: VariantIdx::new(0) },
            fields: FieldPlacement::Arbitrary {
                offsets: vec![Size::ZERO, b_offset],
                memory_index: vec![0, 1]
            },
            abi: Abi::ScalarPair(a, b),
            largest_niche,
            align,
            size
        }
    }

    fn univariant_uninterned(&self,
                             ty: Ty<'tcx>,
                             fields: &[TyLayout<'_>],
                             repr: &ReprOptions,
                             kind: StructKind) -> Result<LayoutDetails, LayoutError<'tcx>> {
        let dl = self.data_layout();
        let pack = repr.pack;
        if pack.is_some() && repr.align.is_some() {
            bug!("struct cannot be packed and aligned");
        }

        let mut align = if pack.is_some() {
            dl.i8_align
        } else {
            dl.aggregate_align
        };

        let mut sized = true;
        let mut offsets = vec![Size::ZERO; fields.len()];
        let mut inverse_memory_index: Vec<u32> = (0..fields.len() as u32).collect();

        let mut optimize = !repr.inhibit_struct_field_reordering_opt();
        if let StructKind::Prefixed(_, align) = kind {
            optimize &= align.bytes() == 1;
        }

        if optimize {
            let end = if let StructKind::MaybeUnsized = kind {
                fields.len() - 1
            } else {
                fields.len()
            };
            let optimizing = &mut inverse_memory_index[..end];
            let field_align = |f: &TyLayout<'_>| {
                if let Some(pack) = pack { f.align.abi.min(pack) } else { f.align.abi }
            };
            match kind {
                StructKind::AlwaysSized |
                StructKind::MaybeUnsized => {
                    optimizing.sort_by_key(|&x| {
                        // Place ZSTs first to avoid "interesting offsets",
                        // especially with only one or two non-ZST fields.
                        let f = &fields[x as usize];
                        (!f.is_zst(), cmp::Reverse(field_align(f)))
                    });
                }
                StructKind::Prefixed(..) => {
                    optimizing.sort_by_key(|&x| field_align(&fields[x as usize]));
                }
            }
        }

        // inverse_memory_index holds field indices by increasing memory offset.
        // That is, if field 5 has offset 0, the first element of inverse_memory_index is 5.
        // We now write field offsets to the corresponding offset slot;
        // field 5 with offset 0 puts 0 in offsets[5].
        // At the bottom of this function, we invert `inverse_memory_index` to
        // produce `memory_index` (see `invert_mapping`).


        let mut offset = Size::ZERO;
        let mut largest_niche = None;
        let mut largest_niche_available = 0;

        if let StructKind::Prefixed(prefix_size, prefix_align) = kind {
            let prefix_align = if let Some(pack) = pack {
                prefix_align.min(pack)
            } else {
                prefix_align
            };
            align = align.max(AbiAndPrefAlign::new(prefix_align));
            offset = prefix_size.align_to(prefix_align);
        }

        for &i in &inverse_memory_index {
            let field = fields[i as usize];
            if !sized {
                bug!("univariant: field #{} of `{}` comes after unsized field",
                     offsets.len(), ty);
            }

            if field.is_unsized() {
                sized = false;
            }

            // Invariant: offset < dl.obj_size_bound() <= 1<<61
            let field_align = if let Some(pack) = pack {
                field.align.min(AbiAndPrefAlign::new(pack))
            } else {
                field.align
            };
            offset = offset.align_to(field_align.abi);
            align = align.max(field_align);

            debug!("univariant offset: {:?} field: {:#?}", offset, field);
            offsets[i as usize] = offset;

            if let Some(mut niche) = field.largest_niche.clone() {
                let available = niche.available(dl);
                if available > largest_niche_available {
                    largest_niche_available = available;
                    niche.offset += offset;
                    largest_niche = Some(niche);
                }
            }

            offset = offset.checked_add(field.size, dl)
                .ok_or(LayoutError::SizeOverflow(ty))?;
        }

        if let Some(repr_align) = repr.align {
            align = align.max(AbiAndPrefAlign::new(repr_align));
        }

        debug!("univariant min_size: {:?}", offset);
        let min_size = offset;

        // As stated above, inverse_memory_index holds field indices by increasing offset.
        // This makes it an already-sorted view of the offsets vec.
        // To invert it, consider:
        // If field 5 has offset 0, offsets[0] is 5, and memory_index[5] should be 0.
        // Field 5 would be the first element, so memory_index is i:
        // Note: if we didn't optimize, it's already right.

        let memory_index;
        if optimize {
            memory_index = invert_mapping(&inverse_memory_index);
        } else {
            memory_index = inverse_memory_index;
        }

        let size = min_size.align_to(align.abi);
        let mut abi = Abi::Aggregate { sized };

        // Unpack newtype ABIs and find scalar pairs.
        if sized && size.bytes() > 0 {
            // All other fields must be ZSTs, and we need them to all start at 0.
            let mut zst_offsets =
                offsets.iter().enumerate().filter(|&(i, _)| fields[i].is_zst());
            if zst_offsets.all(|(_, o)| o.bytes() == 0) {
                let mut non_zst_fields =
                    fields.iter().enumerate().filter(|&(_, f)| !f.is_zst());

                match (non_zst_fields.next(), non_zst_fields.next(), non_zst_fields.next()) {
                    // We have exactly one non-ZST field.
                    (Some((i, field)), None, None) => {
                        // Field fills the struct and it has a scalar or scalar pair ABI.
                        if offsets[i].bytes() == 0 &&
                           align.abi == field.align.abi &&
                           size == field.size {
                            match field.abi {
                                // For plain scalars, or vectors of them, we can't unpack
                                // newtypes for `#[repr(C)]`, as that affects C ABIs.
                                Abi::Scalar(_) | Abi::Vector { .. } if optimize => {
                                    abi = field.abi.clone();
                                }
                                // But scalar pairs are Rust-specific and get
                                // treated as aggregates by C ABIs anyway.
                                Abi::ScalarPair(..) => {
                                    abi = field.abi.clone();
                                }
                                _ => {}
                            }
                        }
                    }

                    // Two non-ZST fields, and they're both scalars.
                    (Some((i, &TyLayout {
                        details: &LayoutDetails { abi: Abi::Scalar(ref a), .. }, ..
                    })), Some((j, &TyLayout {
                        details: &LayoutDetails { abi: Abi::Scalar(ref b), .. }, ..
                    })), None) => {
                        // Order by the memory placement, not source order.
                        let ((i, a), (j, b)) = if offsets[i] < offsets[j] {
                            ((i, a), (j, b))
                        } else {
                            ((j, b), (i, a))
                        };
                        let pair = self.scalar_pair(a.clone(), b.clone());
                        let pair_offsets = match pair.fields {
                            FieldPlacement::Arbitrary {
                                ref offsets,
                                ref memory_index
                            } => {
                                assert_eq!(memory_index, &[0, 1]);
                                offsets
                            }
                            _ => bug!()
                        };
                        if offsets[i] == pair_offsets[0] &&
                           offsets[j] == pair_offsets[1] &&
                           align == pair.align &&
                           size == pair.size {
                            // We can use `ScalarPair` only when it matches our
                            // already computed layout (including `#[repr(C)]`).
                            abi = pair.abi;
                        }
                    }

                    _ => {}
                }
            }
        }

        if sized && fields.iter().any(|f| f.abi.is_uninhabited()) {
            abi = Abi::Uninhabited;
        }

        Ok(LayoutDetails {
            variants: Variants::Single { index: VariantIdx::new(0) },
            fields: FieldPlacement::Arbitrary {
                offsets,
                memory_index
            },
            abi,
            largest_niche,
            align,
            size
        })
    }

    fn layout_raw_uncached(&self, ty: Ty<'tcx>) -> Result<&'tcx LayoutDetails, LayoutError<'tcx>> {
        let tcx = self.tcx;
        let param_env = self.param_env;
        let dl = self.data_layout();
        let scalar_unit = |value: Primitive| {
            let bits = value.size(dl).bits();
            assert!(bits <= 128);
            Scalar {
                value,
                valid_range: 0..=(!0 >> (128 - bits))
            }
        };
        let scalar = |value: Primitive| {
            tcx.intern_layout(LayoutDetails::scalar(self, scalar_unit(value)))
        };

        let univariant = |fields: &[TyLayout<'_>], repr: &ReprOptions, kind| {
            Ok(tcx.intern_layout(self.univariant_uninterned(ty, fields, repr, kind)?))
        };
        debug_assert!(!ty.has_infer_types());

        Ok(match ty.kind {
            // Basic scalars.
            ty::Bool => {
                tcx.intern_layout(LayoutDetails::scalar(self, Scalar {
                    value: Int(I8, false),
                    valid_range: 0..=1
                }))
            }
            ty::Char => {
                tcx.intern_layout(LayoutDetails::scalar(self, Scalar {
                    value: Int(I32, false),
                    valid_range: 0..=0x10FFFF
                }))
            }
            ty::Int(ity) => {
                scalar(Int(Integer::from_attr(dl, attr::SignedInt(ity)), true))
            }
            ty::Uint(ity) => {
                scalar(Int(Integer::from_attr(dl, attr::UnsignedInt(ity)), false))
            }
            ty::Float(fty) => scalar(match fty {
                ast::FloatTy::F32 => F32,
                ast::FloatTy::F64 => F64,
            }),
            ty::FnPtr(_) => {
                let mut ptr = scalar_unit(Pointer);
                ptr.valid_range = 1..=*ptr.valid_range.end();
                tcx.intern_layout(LayoutDetails::scalar(self, ptr))
            }

            // The never type.
            ty::Never => {
                tcx.intern_layout(LayoutDetails {
                    variants: Variants::Single { index: VariantIdx::new(0) },
                    fields: FieldPlacement::Union(0),
                    abi: Abi::Uninhabited,
                    largest_niche: None,
                    align: dl.i8_align,
                    size: Size::ZERO
                })
            }

            // Potentially-fat pointers.
            ty::Ref(_, pointee, _) |
            ty::RawPtr(ty::TypeAndMut { ty: pointee, .. }) => {
                let mut data_ptr = scalar_unit(Pointer);
                if !ty.is_unsafe_ptr() {
                    data_ptr.valid_range = 1..=*data_ptr.valid_range.end();
                }

                let pointee = tcx.normalize_erasing_regions(param_env, pointee);
                if pointee.is_sized(tcx.at(DUMMY_SP), param_env) {
                    return Ok(tcx.intern_layout(LayoutDetails::scalar(self, data_ptr)));
                }

                let unsized_part = tcx.struct_tail_erasing_lifetimes(pointee, param_env);
                let metadata = match unsized_part.kind {
                    ty::Foreign(..) => {
                        return Ok(tcx.intern_layout(LayoutDetails::scalar(self, data_ptr)));
                    }
                    ty::Slice(_) | ty::Str => {
                        scalar_unit(Int(dl.ptr_sized_integer(), false))
                    }
                    ty::Dynamic(..) => {
                        let mut vtable = scalar_unit(Pointer);
                        vtable.valid_range = 1..=*vtable.valid_range.end();
                        vtable
                    }
                    _ => return Err(LayoutError::Unknown(unsized_part))
                };

                // Effectively a (ptr, meta) tuple.
                tcx.intern_layout(self.scalar_pair(data_ptr, metadata))
            }

            // Arrays and slices.
            ty::Array(element, mut count) => {
                if count.has_projections() {
                    count = tcx.normalize_erasing_regions(param_env, count);
                    if count.has_projections() {
                        return Err(LayoutError::Unknown(ty));
                    }
                }

                let count = count.try_eval_usize(tcx, param_env).ok_or(LayoutError::Unknown(ty))?;
                let element = self.layout_of(element)?;
                let size = element.size.checked_mul(count, dl)
                    .ok_or(LayoutError::SizeOverflow(ty))?;

                let abi = if count != 0 && ty.conservative_is_privately_uninhabited(tcx) {
                    Abi::Uninhabited
                } else {
                    Abi::Aggregate { sized: true }
                };

                let largest_niche = if count != 0 {
                    element.largest_niche.clone()
                } else {
                    None
                };

                tcx.intern_layout(LayoutDetails {
                    variants: Variants::Single { index: VariantIdx::new(0) },
                    fields: FieldPlacement::Array {
                        stride: element.size,
                        count
                    },
                    abi,
                    largest_niche,
                    align: element.align,
                    size
                })
            }
            ty::Slice(element) => {
                let element = self.layout_of(element)?;
                tcx.intern_layout(LayoutDetails {
                    variants: Variants::Single { index: VariantIdx::new(0) },
                    fields: FieldPlacement::Array {
                        stride: element.size,
                        count: 0
                    },
                    abi: Abi::Aggregate { sized: false },
                    largest_niche: None,
                    align: element.align,
                    size: Size::ZERO
                })
            }
            ty::Str => {
                tcx.intern_layout(LayoutDetails {
                    variants: Variants::Single { index: VariantIdx::new(0) },
                    fields: FieldPlacement::Array {
                        stride: Size::from_bytes(1),
                        count: 0
                    },
                    abi: Abi::Aggregate { sized: false },
                    largest_niche: None,
                    align: dl.i8_align,
                    size: Size::ZERO
                })
            }

            // Odd unit types.
            ty::FnDef(..) => {
                univariant(&[], &ReprOptions::default(), StructKind::AlwaysSized)?
            }
            ty::Dynamic(..) | ty::Foreign(..) => {
                let mut unit = self.univariant_uninterned(ty, &[], &ReprOptions::default(),
                  StructKind::AlwaysSized)?;
                match unit.abi {
                    Abi::Aggregate { ref mut sized } => *sized = false,
                    _ => bug!()
                }
                tcx.intern_layout(unit)
            }

            ty::Generator(def_id, substs, _) => self.generator_layout(ty, def_id, substs)?,

            ty::Closure(def_id, ref substs) => {
                let tys = substs.as_closure().upvar_tys(def_id, tcx);
                univariant(&tys.map(|ty| self.layout_of(ty)).collect::<Result<Vec<_>, _>>()?,
                    &ReprOptions::default(),
                    StructKind::AlwaysSized)?
            }

            ty::Tuple(tys) => {
                let kind = if tys.len() == 0 {
                    StructKind::AlwaysSized
                } else {
                    StructKind::MaybeUnsized
                };

                univariant(&tys.iter().map(|k| {
                    self.layout_of(k.expect_ty())
                }).collect::<Result<Vec<_>, _>>()?, &ReprOptions::default(), kind)?
            }

            // SIMD vector types.
            ty::Adt(def, ..) if def.repr.simd() => {
                let element = self.layout_of(ty.simd_type(tcx))?;
                let count = ty.simd_size(tcx);
                assert!(count > 0);
                let scalar = match element.abi {
                    Abi::Scalar(ref scalar) => scalar.clone(),
                    _ => {
                        tcx.sess.fatal(&format!("monomorphising SIMD type `{}` with \
                                                 a non-machine element type `{}`",
                                                ty, element.ty));
                    }
                };
                let size = element.size.checked_mul(count, dl)
                    .ok_or(LayoutError::SizeOverflow(ty))?;
                let align = dl.vector_align(size);
                let size = size.align_to(align.abi);

                tcx.intern_layout(LayoutDetails {
                    variants: Variants::Single { index: VariantIdx::new(0) },
                    fields: FieldPlacement::Array {
                        stride: element.size,
                        count
                    },
                    abi: Abi::Vector {
                        element: scalar,
                        count
                    },
                    largest_niche: element.largest_niche.clone(),
                    size,
                    align,
                })
            }

            // ADTs.
            ty::Adt(def, substs) => {
                // Cache the field layouts.
                let variants = def.variants.iter().map(|v| {
                    v.fields.iter().map(|field| {
                        self.layout_of(field.ty(tcx, substs))
                    }).collect::<Result<Vec<_>, _>>()
                }).collect::<Result<IndexVec<VariantIdx, _>, _>>()?;

                if def.is_union() {
                    if def.repr.pack.is_some() && def.repr.align.is_some() {
                        bug!("union cannot be packed and aligned");
                    }

                    let mut align = if def.repr.pack.is_some() {
                        dl.i8_align
                    } else {
                        dl.aggregate_align
                    };

                    if let Some(repr_align) = def.repr.align {
                        align = align.max(AbiAndPrefAlign::new(repr_align));
                    }

                    let optimize = !def.repr.inhibit_union_abi_opt();
                    let mut size = Size::ZERO;
                    let mut abi = Abi::Aggregate { sized: true };
                    let index = VariantIdx::new(0);
                    for field in &variants[index] {
                        assert!(!field.is_unsized());
                        align = align.max(field.align);

                        // If all non-ZST fields have the same ABI, forward this ABI
                        if optimize && !field.is_zst() {
                            // Normalize scalar_unit to the maximal valid range
                            let field_abi = match &field.abi {
                                Abi::Scalar(x) => Abi::Scalar(scalar_unit(x.value)),
                                Abi::ScalarPair(x, y) => {
                                    Abi::ScalarPair(
                                        scalar_unit(x.value),
                                        scalar_unit(y.value),
                                    )
                                }
                                Abi::Vector { element: x, count } => {
                                    Abi::Vector {
                                        element: scalar_unit(x.value),
                                        count: *count,
                                    }
                                }
                                Abi::Uninhabited |
                                Abi::Aggregate { .. }  => Abi::Aggregate { sized: true },
                            };

                            if size == Size::ZERO {
                                // first non ZST: initialize 'abi'
                                abi = field_abi;
                            } else if abi != field_abi  {
                                // different fields have different ABI: reset to Aggregate
                                abi = Abi::Aggregate { sized: true };
                            }
                        }

                        size = cmp::max(size, field.size);
                    }

                    if let Some(pack) = def.repr.pack {
                        align = align.min(AbiAndPrefAlign::new(pack));
                    }

                    return Ok(tcx.intern_layout(LayoutDetails {
                        variants: Variants::Single { index },
                        fields: FieldPlacement::Union(variants[index].len()),
                        abi,
                        largest_niche: None,
                        align,
                        size: size.align_to(align.abi)
                    }));
                }

                // A variant is absent if it's uninhabited and only has ZST fields.
                // Present uninhabited variants only require space for their fields,
                // but *not* an encoding of the discriminant (e.g., a tag value).
                // See issue #49298 for more details on the need to leave space
                // for non-ZST uninhabited data (mostly partial initialization).
                let absent = |fields: &[TyLayout<'_>]| {
                    let uninhabited = fields.iter().any(|f| f.abi.is_uninhabited());
                    let is_zst = fields.iter().all(|f| f.is_zst());
                    uninhabited && is_zst
                };
                let (present_first, present_second) = {
                    let mut present_variants = variants.iter_enumerated().filter_map(|(i, v)| {
                        if absent(v) {
                            None
                        } else {
                            Some(i)
                        }
                    });
                    (present_variants.next(), present_variants.next())
                };
                let present_first = match present_first {
                    present_first @ Some(_) => present_first,
                    // Uninhabited because it has no variants, or only absent ones.
                    None if def.is_enum() => return tcx.layout_raw(param_env.and(tcx.types.never)),
                    // if it's a struct, still compute a layout so that we can still compute the
                    // field offsets
                    None => Some(VariantIdx::new(0)),
                };

                let is_struct = !def.is_enum() ||
                    // Only one variant is present.
                    (present_second.is_none() &&
                    // Representation optimizations are allowed.
                    !def.repr.inhibit_enum_layout_opt());
                if is_struct {
                    // Struct, or univariant enum equivalent to a struct.
                    // (Typechecking will reject discriminant-sizing attrs.)

                    let v = present_first.unwrap();
                    let kind = if def.is_enum() || variants[v].len() == 0 {
                        StructKind::AlwaysSized
                    } else {
                        let param_env = tcx.param_env(def.did);
                        let last_field = def.variants[v].fields.last().unwrap();
                        let always_sized = tcx.type_of(last_field.did)
                                              .is_sized(tcx.at(DUMMY_SP), param_env);
                        if !always_sized { StructKind::MaybeUnsized }
                        else { StructKind::AlwaysSized }
                    };

                    let mut st = self.univariant_uninterned(ty, &variants[v], &def.repr, kind)?;
                    st.variants = Variants::Single { index: v };
                    let (start, end) = self.tcx.layout_scalar_valid_range(def.did);
                    match st.abi {
                        Abi::Scalar(ref mut scalar) |
                        Abi::ScalarPair(ref mut scalar, _) => {
                            // the asserts ensure that we are not using the
                            // `#[rustc_layout_scalar_valid_range(n)]`
                            // attribute to widen the range of anything as that would probably
                            // result in UB somewhere
                            // FIXME(eddyb) the asserts are probably not needed,
                            // as larger validity ranges would result in missed
                            // optimizations, *not* wrongly assuming the inner
                            // value is valid. e.g. unions enlarge validity ranges,
                            // because the values may be uninitialized.
                            if let Bound::Included(start) = start {
                                // FIXME(eddyb) this might be incorrect - it doesn't
                                // account for wrap-around (end < start) ranges.
                                assert!(*scalar.valid_range.start() <= start);
                                scalar.valid_range = start..=*scalar.valid_range.end();
                            }
                            if let Bound::Included(end) = end {
                                // FIXME(eddyb) this might be incorrect - it doesn't
                                // account for wrap-around (end < start) ranges.
                                assert!(*scalar.valid_range.end() >= end);
                                scalar.valid_range = *scalar.valid_range.start()..=end;
                            }

                            // Update `largest_niche` if we have introduced a larger niche.
                            let niche = Niche::from_scalar(dl, Size::ZERO, scalar.clone());
                            if let Some(niche) = niche {
                                match &st.largest_niche {
                                    Some(largest_niche) => {
                                        // Replace the existing niche even if they're equal,
                                        // because this one is at a lower offset.
                                        if largest_niche.available(dl) <= niche.available(dl) {
                                            st.largest_niche = Some(niche);
                                        }
                                    }
                                    None => st.largest_niche = Some(niche),
                                }
                            }
                        }
                        _ => assert!(
                            start == Bound::Unbounded && end == Bound::Unbounded,
                            "nonscalar layout for layout_scalar_valid_range type {:?}: {:#?}",
                            def,
                            st,
                        ),
                    }

                    return Ok(tcx.intern_layout(st));
                }

                // The current code for niche-filling relies on variant indices
                // instead of actual discriminants, so dataful enums with
                // explicit discriminants (RFC #2363) would misbehave.
                let no_explicit_discriminants = def.variants.iter_enumerated()
                    .all(|(i, v)| v.discr == ty::VariantDiscr::Relative(i.as_u32()));

                // Niche-filling enum optimization.
                if !def.repr.inhibit_enum_layout_opt() && no_explicit_discriminants {
                    let mut dataful_variant = None;
                    let mut niche_variants = VariantIdx::MAX..=VariantIdx::new(0);

                    // Find one non-ZST variant.
                    'variants: for (v, fields) in variants.iter_enumerated() {
                        if absent(fields) {
                            continue 'variants;
                        }
                        for f in fields {
                            if !f.is_zst() {
                                if dataful_variant.is_none() {
                                    dataful_variant = Some(v);
                                    continue 'variants;
                                } else {
                                    dataful_variant = None;
                                    break 'variants;
                                }
                            }
                        }
                        niche_variants = *niche_variants.start().min(&v)..=v;
                    }

                    if niche_variants.start() > niche_variants.end() {
                        dataful_variant = None;
                    }

                    if let Some(i) = dataful_variant {
                        let count = (
                            niche_variants.end().as_u32() - niche_variants.start().as_u32() + 1
                        ) as u128;
                        // FIXME(#62691) use the largest niche across all fields,
                        // not just the first one.
                        for (field_index, &field) in variants[i].iter().enumerate() {
                            let niche = match &field.largest_niche {
                                Some(niche) => niche,
                                _ => continue,
                            };
                            let (niche_start, niche_scalar) = match niche.reserve(self, count) {
                                Some(pair) => pair,
                                None => continue,
                            };

                            let mut align = dl.aggregate_align;
                            let st = variants.iter_enumerated().map(|(j, v)| {
                                let mut st = self.univariant_uninterned(ty, v,
                                    &def.repr, StructKind::AlwaysSized)?;
                                st.variants = Variants::Single { index: j };

                                align = align.max(st.align);

                                Ok(st)
                            }).collect::<Result<IndexVec<VariantIdx, _>, _>>()?;

                            let offset = st[i].fields.offset(field_index) + niche.offset;
                            let size = st[i].size;

                            let mut abi = match st[i].abi {
                                Abi::Scalar(_) => Abi::Scalar(niche_scalar.clone()),
                                Abi::ScalarPair(ref first, ref second) => {
                                    // We need to use scalar_unit to reset the
                                    // valid range to the maximal one for that
                                    // primitive, because only the niche is
                                    // guaranteed to be initialised, not the
                                    // other primitive.
                                    if offset.bytes() == 0 {
                                        Abi::ScalarPair(
                                            niche_scalar.clone(),
                                            scalar_unit(second.value),
                                        )
                                    } else {
                                        Abi::ScalarPair(
                                            scalar_unit(first.value),
                                            niche_scalar.clone(),
                                        )
                                    }
                                }
                                _ => Abi::Aggregate { sized: true },
                            };

                            if st.iter().all(|v| v.abi.is_uninhabited()) {
                                abi = Abi::Uninhabited;
                            }


                            let largest_niche =
                                Niche::from_scalar(dl, offset, niche_scalar.clone());

                            return Ok(tcx.intern_layout(LayoutDetails {
                                variants: Variants::Multiple {
                                    discr: niche_scalar,
                                    discr_kind: DiscriminantKind::Niche {
                                        dataful_variant: i,
                                        niche_variants,
                                        niche_start,
                                    },
                                    discr_index: 0,
                                    variants: st,
                                },
                                fields: FieldPlacement::Arbitrary {
                                    offsets: vec![offset],
                                    memory_index: vec![0]
                                },
                                abi,
                                largest_niche,
                                size,
                                align,
                            }));
                        }
                    }
                }

                let (mut min, mut max) = (i128::max_value(), i128::min_value());
                let discr_type = def.repr.discr_type();
                let bits = Integer::from_attr(self, discr_type).size().bits();
                for (i, discr) in def.discriminants(tcx) {
                    if variants[i].iter().any(|f| f.abi.is_uninhabited()) {
                        continue;
                    }
                    let mut x = discr.val as i128;
                    if discr_type.is_signed() {
                        // sign extend the raw representation to be an i128
                        x = (x << (128 - bits)) >> (128 - bits);
                    }
                    if x < min { min = x; }
                    if x > max { max = x; }
                }
                // We might have no inhabited variants, so pretend there's at least one.
                if (min, max) == (i128::max_value(), i128::min_value()) {
                    min = 0;
                    max = 0;
                }
                assert!(min <= max, "discriminant range is {}...{}", min, max);
                let (min_ity, signed) = Integer::repr_discr(tcx, ty, &def.repr, min, max);

                let mut align = dl.aggregate_align;
                let mut size = Size::ZERO;

                // We're interested in the smallest alignment, so start large.
                let mut start_align = Align::from_bytes(256).unwrap();
                assert_eq!(Integer::for_align(dl, start_align), None);

                // repr(C) on an enum tells us to make a (tag, union) layout,
                // so we need to grow the prefix alignment to be at least
                // the alignment of the union. (This value is used both for
                // determining the alignment of the overall enum, and the
                // determining the alignment of the payload after the tag.)
                let mut prefix_align = min_ity.align(dl).abi;
                if def.repr.c() {
                    for fields in &variants {
                        for field in fields {
                            prefix_align = prefix_align.max(field.align.abi);
                        }
                    }
                }

                // Create the set of structs that represent each variant.
                let mut layout_variants = variants.iter_enumerated().map(|(i, field_layouts)| {
                    let mut st = self.univariant_uninterned(ty, &field_layouts,
                        &def.repr, StructKind::Prefixed(min_ity.size(), prefix_align))?;
                    st.variants = Variants::Single { index: i };
                    // Find the first field we can't move later
                    // to make room for a larger discriminant.
                    for field in st.fields.index_by_increasing_offset().map(|j| field_layouts[j]) {
                        if !field.is_zst() || field.align.abi.bytes() != 1 {
                            start_align = start_align.min(field.align.abi);
                            break;
                        }
                    }
                    size = cmp::max(size, st.size);
                    align = align.max(st.align);
                    Ok(st)
                }).collect::<Result<IndexVec<VariantIdx, _>, _>>()?;

                // Align the maximum variant size to the largest alignment.
                size = size.align_to(align.abi);

                if size.bytes() >= dl.obj_size_bound() {
                    return Err(LayoutError::SizeOverflow(ty));
                }

                let typeck_ity = Integer::from_attr(dl, def.repr.discr_type());
                if typeck_ity < min_ity {
                    // It is a bug if Layout decided on a greater discriminant size than typeck for
                    // some reason at this point (based on values discriminant can take on). Mostly
                    // because this discriminant will be loaded, and then stored into variable of
                    // type calculated by typeck. Consider such case (a bug): typeck decided on
                    // byte-sized discriminant, but layout thinks we need a 16-bit to store all
                    // discriminant values. That would be a bug, because then, in codegen, in order
                    // to store this 16-bit discriminant into 8-bit sized temporary some of the
                    // space necessary to represent would have to be discarded (or layout is wrong
                    // on thinking it needs 16 bits)
                    bug!("layout decided on a larger discriminant type ({:?}) than typeck ({:?})",
                         min_ity, typeck_ity);
                    // However, it is fine to make discr type however large (as an optimisation)
                    // after this point – we’ll just truncate the value we load in codegen.
                }

                // Check to see if we should use a different type for the
                // discriminant. We can safely use a type with the same size
                // as the alignment of the first field of each variant.
                // We increase the size of the discriminant to avoid LLVM copying
                // padding when it doesn't need to. This normally causes unaligned
                // load/stores and excessive memcpy/memset operations. By using a
                // bigger integer size, LLVM can be sure about its contents and
                // won't be so conservative.

                // Use the initial field alignment
                let mut ity = if def.repr.c() || def.repr.int.is_some() {
                    min_ity
                } else {
                    Integer::for_align(dl, start_align).unwrap_or(min_ity)
                };

                // If the alignment is not larger than the chosen discriminant size,
                // don't use the alignment as the final size.
                if ity <= min_ity {
                    ity = min_ity;
                } else {
                    // Patch up the variants' first few fields.
                    let old_ity_size = min_ity.size();
                    let new_ity_size = ity.size();
                    for variant in &mut layout_variants {
                        match variant.fields {
                            FieldPlacement::Arbitrary { ref mut offsets, .. } => {
                                for i in offsets {
                                    if *i <= old_ity_size {
                                        assert_eq!(*i, old_ity_size);
                                        *i = new_ity_size;
                                    }
                                }
                                // We might be making the struct larger.
                                if variant.size <= old_ity_size {
                                    variant.size = new_ity_size;
                                }
                            }
                            _ => bug!()
                        }
                    }
                }

                let tag_mask = !0u128 >> (128 - ity.size().bits());
                let tag = Scalar {
                    value: Int(ity, signed),
                    valid_range: (min as u128 & tag_mask)..=(max as u128 & tag_mask),
                };
                let mut abi = Abi::Aggregate { sized: true };
                if tag.value.size(dl) == size {
                    abi = Abi::Scalar(tag.clone());
                } else {
                    // Try to use a ScalarPair for all tagged enums.
                    let mut common_prim = None;
                    for (field_layouts, layout_variant) in variants.iter().zip(&layout_variants) {
                        let offsets = match layout_variant.fields {
                            FieldPlacement::Arbitrary { ref offsets, .. } => offsets,
                            _ => bug!(),
                        };
                        let mut fields = field_layouts
                            .iter()
                            .zip(offsets)
                            .filter(|p| !p.0.is_zst());
                        let (field, offset) = match (fields.next(), fields.next()) {
                            (None, None) => continue,
                            (Some(pair), None) => pair,
                            _ => {
                                common_prim = None;
                                break;
                            }
                        };
                        let prim = match field.details.abi {
                            Abi::Scalar(ref scalar) => scalar.value,
                            _ => {
                                common_prim = None;
                                break;
                            }
                        };
                        if let Some(pair) = common_prim {
                            // This is pretty conservative. We could go fancier
                            // by conflating things like i32 and u32, or even
                            // realising that (u8, u8) could just cohabit with
                            // u16 or even u32.
                            if pair != (prim, offset) {
                                common_prim = None;
                                break;
                            }
                        } else {
                            common_prim = Some((prim, offset));
                        }
                    }
                    if let Some((prim, offset)) = common_prim {
                        let pair = self.scalar_pair(tag.clone(), scalar_unit(prim));
                        let pair_offsets = match pair.fields {
                            FieldPlacement::Arbitrary {
                                ref offsets,
                                ref memory_index
                            } => {
                                assert_eq!(memory_index, &[0, 1]);
                                offsets
                            }
                            _ => bug!()
                        };
                        if pair_offsets[0] == Size::ZERO &&
                            pair_offsets[1] == *offset &&
                            align == pair.align &&
                            size == pair.size {
                            // We can use `ScalarPair` only when it matches our
                            // already computed layout (including `#[repr(C)]`).
                            abi = pair.abi;
                        }
                    }
                }

                if layout_variants.iter().all(|v| v.abi.is_uninhabited()) {
                    abi = Abi::Uninhabited;
                }

                let largest_niche = Niche::from_scalar(dl, Size::ZERO, tag.clone());

                tcx.intern_layout(LayoutDetails {
                    variants: Variants::Multiple {
                        discr: tag,
                        discr_kind: DiscriminantKind::Tag,
                        discr_index: 0,
                        variants: layout_variants,
                    },
                    fields: FieldPlacement::Arbitrary {
                        offsets: vec![Size::ZERO],
                        memory_index: vec![0]
                    },
                    largest_niche,
                    abi,
                    align,
                    size
                })
            }

            // Types with no meaningful known layout.
            ty::Projection(_) | ty::Opaque(..) => {
                let normalized = tcx.normalize_erasing_regions(param_env, ty);
                if ty == normalized {
                    return Err(LayoutError::Unknown(ty));
                }
                tcx.layout_raw(param_env.and(normalized))?
            }

            ty::Bound(..) |
            ty::Placeholder(..) |
            ty::UnnormalizedProjection(..) |
            ty::GeneratorWitness(..) |
            ty::Infer(_) => {
                bug!("LayoutDetails::compute: unexpected type `{}`", ty)
            }

            ty::Param(_) | ty::Error => {
                return Err(LayoutError::Unknown(ty));
            }
        })
    }
}

/// Overlap eligibility and variant assignment for each GeneratorSavedLocal.
#[derive(Clone, Debug, PartialEq)]
enum SavedLocalEligibility {
    Unassigned,
    Assigned(VariantIdx),
    // FIXME: Use newtype_index so we aren't wasting bytes
    Ineligible(Option<u32>),
}

// When laying out generators, we divide our saved local fields into two
// categories: overlap-eligible and overlap-ineligible.
//
// Those fields which are ineligible for overlap go in a "prefix" at the
// beginning of the layout, and always have space reserved for them.
//
// Overlap-eligible fields are only assigned to one variant, so we lay
// those fields out for each variant and put them right after the
// prefix.
//
// Finally, in the layout details, we point to the fields from the
// variants they are assigned to. It is possible for some fields to be
// included in multiple variants. No field ever "moves around" in the
// layout; its offset is always the same.
//
// Also included in the layout are the upvars and the discriminant.
// These are included as fields on the "outer" layout; they are not part
// of any variant.
impl<'tcx> LayoutCx<'tcx, TyCtxt<'tcx>> {
    /// Compute the eligibility and assignment of each local.
    fn generator_saved_local_eligibility(&self, info: &GeneratorLayout<'tcx>)
    -> (BitSet<GeneratorSavedLocal>, IndexVec<GeneratorSavedLocal, SavedLocalEligibility>) {
        use SavedLocalEligibility::*;

        let mut assignments: IndexVec<GeneratorSavedLocal, SavedLocalEligibility> =
            IndexVec::from_elem_n(Unassigned, info.field_tys.len());

        // The saved locals not eligible for overlap. These will get
        // "promoted" to the prefix of our generator.
        let mut ineligible_locals = BitSet::new_empty(info.field_tys.len());

        // Figure out which of our saved locals are fields in only
        // one variant. The rest are deemed ineligible for overlap.
        for (variant_index, fields) in info.variant_fields.iter_enumerated() {
            for local in fields {
                match assignments[*local] {
                    Unassigned => {
                        assignments[*local] = Assigned(variant_index);
                    }
                    Assigned(idx) => {
                        // We've already seen this local at another suspension
                        // point, so it is no longer a candidate.
                        trace!("removing local {:?} in >1 variant ({:?}, {:?})",
                               local, variant_index, idx);
                        ineligible_locals.insert(*local);
                        assignments[*local] = Ineligible(None);
                    }
                    Ineligible(_) => {},
                }
            }
        }

        // Next, check every pair of eligible locals to see if they
        // conflict.
        for local_a in info.storage_conflicts.rows() {
            let conflicts_a = info.storage_conflicts.count(local_a);
            if ineligible_locals.contains(local_a) {
                continue;
            }

            for local_b in info.storage_conflicts.iter(local_a) {
                // local_a and local_b are storage live at the same time, therefore they
                // cannot overlap in the generator layout. The only way to guarantee
                // this is if they are in the same variant, or one is ineligible
                // (which means it is stored in every variant).
                if ineligible_locals.contains(local_b) ||
                    assignments[local_a] == assignments[local_b]
                {
                    continue;
                }

                // If they conflict, we will choose one to make ineligible.
                // This is not always optimal; it's just a greedy heuristic that
                // seems to produce good results most of the time.
                let conflicts_b = info.storage_conflicts.count(local_b);
                let (remove, other) = if conflicts_a > conflicts_b {
                    (local_a, local_b)
                } else {
                    (local_b, local_a)
                };
                ineligible_locals.insert(remove);
                assignments[remove] = Ineligible(None);
                trace!("removing local {:?} due to conflict with {:?}", remove, other);
            }
        }

        // Count the number of variants in use. If only one of them, then it is
        // impossible to overlap any locals in our layout. In this case it's
        // always better to make the remaining locals ineligible, so we can
        // lay them out with the other locals in the prefix and eliminate
        // unnecessary padding bytes.
        {
            let mut used_variants = BitSet::new_empty(info.variant_fields.len());
            for assignment in &assignments {
                match assignment {
                    Assigned(idx) => { used_variants.insert(*idx); }
                    _ => {}
                }
            }
            if used_variants.count() < 2 {
                for assignment in assignments.iter_mut() {
                    *assignment = Ineligible(None);
                }
                ineligible_locals.insert_all();
            }
        }

        // Write down the order of our locals that will be promoted to the prefix.
        {
            let mut idx = 0u32;
            for local in ineligible_locals.iter() {
                assignments[local] = Ineligible(Some(idx));
                idx += 1;
            }
        }
        debug!("generator saved local assignments: {:?}", assignments);

        (ineligible_locals, assignments)
    }

    /// Compute the full generator layout.
    fn generator_layout(
        &self,
        ty: Ty<'tcx>,
        def_id: hir::def_id::DefId,
        substs: SubstsRef<'tcx>,
    ) -> Result<&'tcx LayoutDetails, LayoutError<'tcx>> {
        use SavedLocalEligibility::*;
        let tcx = self.tcx;

        let subst_field = |ty: Ty<'tcx>| { ty.subst(tcx, substs) };

        let info = tcx.generator_layout(def_id);
        let (ineligible_locals, assignments) = self.generator_saved_local_eligibility(&info);

        // Build a prefix layout, including "promoting" all ineligible
        // locals as part of the prefix. We compute the layout of all of
        // these fields at once to get optimal packing.
        let discr_index = substs.as_generator().prefix_tys(def_id, tcx).count();
        // FIXME(eddyb) set the correct vaidity range for the discriminant.
        let discr_layout = self.layout_of(substs.as_generator().discr_ty(tcx))?;
        let discr = match &discr_layout.abi {
            Abi::Scalar(s) => s.clone(),
            _ => bug!(),
        };
        let promoted_layouts = ineligible_locals.iter()
            .map(|local| subst_field(info.field_tys[local]))
            .map(|ty| tcx.mk_maybe_uninit(ty))
            .map(|ty| self.layout_of(ty));
        let prefix_layouts = substs.as_generator().prefix_tys(def_id, tcx)
            .map(|ty| self.layout_of(ty))
            .chain(iter::once(Ok(discr_layout)))
            .chain(promoted_layouts)
            .collect::<Result<Vec<_>, _>>()?;
        let prefix = self.univariant_uninterned(
            ty,
            &prefix_layouts,
            &ReprOptions::default(),
            StructKind::AlwaysSized,
        )?;

        let (prefix_size, prefix_align) = (prefix.size, prefix.align);

        // Split the prefix layout into the "outer" fields (upvars and
        // discriminant) and the "promoted" fields. Promoted fields will
        // get included in each variant that requested them in
        // GeneratorLayout.
        debug!("prefix = {:#?}", prefix);
        let (outer_fields, promoted_offsets, promoted_memory_index) = match prefix.fields {
            FieldPlacement::Arbitrary { mut offsets, memory_index } => {
                let mut inverse_memory_index = invert_mapping(&memory_index);

                // "a" (`0..b_start`) and "b" (`b_start..`) correspond to
                // "outer" and "promoted" fields respectively.
                let b_start = (discr_index + 1) as u32;
                let offsets_b = offsets.split_off(b_start as usize);
                let offsets_a = offsets;

                // Disentangle the "a" and "b" components of `inverse_memory_index`
                // by preserving the order but keeping only one disjoint "half" each.
                // FIXME(eddyb) build a better abstraction for permutations, if possible.
                let inverse_memory_index_b: Vec<_> =
                    inverse_memory_index.iter().filter_map(|&i| i.checked_sub(b_start)).collect();
                inverse_memory_index.retain(|&i| i < b_start);
                let inverse_memory_index_a = inverse_memory_index;

                // Since `inverse_memory_index_{a,b}` each only refer to their
                // respective fields, they can be safely inverted
                let memory_index_a = invert_mapping(&inverse_memory_index_a);
                let memory_index_b = invert_mapping(&inverse_memory_index_b);

                let outer_fields = FieldPlacement::Arbitrary {
                    offsets: offsets_a,
                    memory_index: memory_index_a,
                };
                (outer_fields, offsets_b, memory_index_b)
            }
            _ => bug!(),
        };

        let mut size = prefix.size;
        let mut align = prefix.align;
        let variants = info.variant_fields.iter_enumerated().map(|(index, variant_fields)| {
            // Only include overlap-eligible fields when we compute our variant layout.
            let variant_only_tys = variant_fields
                .iter()
                .filter(|local| {
                    match assignments[**local] {
                        Unassigned => bug!(),
                        Assigned(v) if v == index => true,
                        Assigned(_) => bug!("assignment does not match variant"),
                        Ineligible(_) => false,
                    }
                })
                .map(|local| subst_field(info.field_tys[*local]));

            let mut variant = self.univariant_uninterned(
                ty,
                &variant_only_tys
                    .map(|ty| self.layout_of(ty))
                    .collect::<Result<Vec<_>, _>>()?,
                &ReprOptions::default(),
                StructKind::Prefixed(prefix_size, prefix_align.abi))?;
            variant.variants = Variants::Single { index };

            let (offsets, memory_index) = match variant.fields {
                FieldPlacement::Arbitrary { offsets, memory_index } => {
                    (offsets, memory_index)
                }
                _ => bug!(),
            };

            // Now, stitch the promoted and variant-only fields back together in
            // the order they are mentioned by our GeneratorLayout.
            // Because we only use some subset (that can differ between variants)
            // of the promoted fields, we can't just pick those elements of the
            // `promoted_memory_index` (as we'd end up with gaps).
            // So instead, we build an "inverse memory_index", as if all of the
            // promoted fields were being used, but leave the elements not in the
            // subset as `INVALID_FIELD_IDX`, which we can filter out later to
            // obtain a valid (bijective) mapping.
            const INVALID_FIELD_IDX: u32 = !0;
            let mut combined_inverse_memory_index =
                vec![INVALID_FIELD_IDX; promoted_memory_index.len() + memory_index.len()];
            let mut offsets_and_memory_index = offsets.into_iter().zip(memory_index);
            let combined_offsets = variant_fields.iter().enumerate().map(|(i, local)| {
                let (offset, memory_index) = match assignments[*local] {
                    Unassigned => bug!(),
                    Assigned(_) => {
                        let (offset, memory_index) = offsets_and_memory_index.next().unwrap();
                        (offset, promoted_memory_index.len() as u32 + memory_index)
                    }
                    Ineligible(field_idx) => {
                        let field_idx = field_idx.unwrap() as usize;
                        (promoted_offsets[field_idx], promoted_memory_index[field_idx])
                    }
                };
                combined_inverse_memory_index[memory_index as usize] = i as u32;
                offset
            }).collect();

            // Remove the unused slots and invert the mapping to obtain the
            // combined `memory_index` (also see previous comment).
            combined_inverse_memory_index.retain(|&i| i != INVALID_FIELD_IDX);
            let combined_memory_index = invert_mapping(&combined_inverse_memory_index);

            variant.fields = FieldPlacement::Arbitrary {
                offsets: combined_offsets,
                memory_index: combined_memory_index,
            };

            size = size.max(variant.size);
            align = align.max(variant.align);
            Ok(variant)
        }).collect::<Result<IndexVec<VariantIdx, _>, _>>()?;

        size = size.align_to(align.abi);

        let abi = if prefix.abi.is_uninhabited() ||
                     variants.iter().all(|v| v.abi.is_uninhabited()) {
            Abi::Uninhabited
        } else {
            Abi::Aggregate { sized: true }
        };

        let layout = tcx.intern_layout(LayoutDetails {
            variants: Variants::Multiple {
                discr,
                discr_kind: DiscriminantKind::Tag,
                discr_index,
                variants,
            },
            fields: outer_fields,
            abi,
            largest_niche: prefix.largest_niche,
            size,
            align,
        });
        debug!("generator layout ({:?}): {:#?}", ty, layout);
        Ok(layout)
    }

    /// This is invoked by the `layout_raw` query to record the final
    /// layout of each type.
    #[inline(always)]
    fn record_layout_for_printing(&self, layout: TyLayout<'tcx>) {
        // If we are running with `-Zprint-type-sizes`, maybe record layouts
        // for dumping later.
        if self.tcx.sess.opts.debugging_opts.print_type_sizes {
            self.record_layout_for_printing_outlined(layout)
        }
    }

    fn record_layout_for_printing_outlined(&self, layout: TyLayout<'tcx>) {
        // Ignore layouts that are done with non-empty environments or
        // non-monomorphic layouts, as the user only wants to see the stuff
        // resulting from the final codegen session.
        if
            layout.ty.has_param_types() ||
            !self.param_env.caller_bounds.is_empty()
        {
            return;
        }

        // (delay format until we actually need it)
        let record = |kind, packed, opt_discr_size, variants| {
            let type_desc = format!("{:?}", layout.ty);
            self.tcx.sess.code_stats.record_type_size(kind,
                                                      type_desc,
                                                      layout.align.abi,
                                                      layout.size,
                                                      packed,
                                                      opt_discr_size,
                                                      variants);
        };

        let adt_def = match layout.ty.kind {
            ty::Adt(ref adt_def, _) => {
                debug!("print-type-size t: `{:?}` process adt", layout.ty);
                adt_def
            }

            ty::Closure(..) => {
                debug!("print-type-size t: `{:?}` record closure", layout.ty);
                record(DataTypeKind::Closure, false, None, vec![]);
                return;
            }

            _ => {
                debug!("print-type-size t: `{:?}` skip non-nominal", layout.ty);
                return;
            }
        };

        let adt_kind = adt_def.adt_kind();
        let adt_packed = adt_def.repr.pack.is_some();

        let build_variant_info = |n: Option<Ident>,
                                  flds: &[ast::Name],
                                  layout: TyLayout<'tcx>| {
            let mut min_size = Size::ZERO;
            let field_info: Vec<_> = flds.iter().enumerate().map(|(i, &name)| {
                match layout.field(self, i) {
                    Err(err) => {
                        bug!("no layout found for field {}: `{:?}`", name, err);
                    }
                    Ok(field_layout) => {
                        let offset = layout.fields.offset(i);
                        let field_end = offset + field_layout.size;
                        if min_size < field_end {
                            min_size = field_end;
                        }
                        session::FieldInfo {
                            name: name.to_string(),
                            offset: offset.bytes(),
                            size: field_layout.size.bytes(),
                            align: field_layout.align.abi.bytes(),
                        }
                    }
                }
            }).collect();

            session::VariantInfo {
                name: n.map(|n| n.to_string()),
                kind: if layout.is_unsized() {
                    session::SizeKind::Min
                } else {
                    session::SizeKind::Exact
                },
                align: layout.align.abi.bytes(),
                size: if min_size.bytes() == 0 {
                    layout.size.bytes()
                } else {
                    min_size.bytes()
                },
                fields: field_info,
            }
        };

        match layout.variants {
            Variants::Single { index } => {
                debug!("print-type-size `{:#?}` variant {}",
                       layout, adt_def.variants[index].ident);
                if !adt_def.variants.is_empty() {
                    let variant_def = &adt_def.variants[index];
                    let fields: Vec<_> =
                        variant_def.fields.iter().map(|f| f.ident.name).collect();
                    record(adt_kind.into(),
                           adt_packed,
                           None,
                           vec![build_variant_info(Some(variant_def.ident),
                                                   &fields,
                                                   layout)]);
                } else {
                    // (This case arises for *empty* enums; so give it
                    // zero variants.)
                    record(adt_kind.into(), adt_packed, None, vec![]);
                }
            }

            Variants::Multiple { ref discr, ref discr_kind, .. } => {
                debug!("print-type-size `{:#?}` adt general variants def {}",
                       layout.ty, adt_def.variants.len());
                let variant_infos: Vec<_> =
                    adt_def.variants.iter_enumerated().map(|(i, variant_def)| {
                        let fields: Vec<_> =
                            variant_def.fields.iter().map(|f| f.ident.name).collect();
                        build_variant_info(Some(variant_def.ident),
                                           &fields,
                                           layout.for_variant(self, i))
                    })
                    .collect();
                record(adt_kind.into(), adt_packed, match discr_kind {
                    DiscriminantKind::Tag => Some(discr.value.size(self)),
                    _ => None
                }, variant_infos);
            }
        }
    }
}

/// Type size "skeleton", i.e., the only information determining a type's size.
/// While this is conservative, (aside from constant sizes, only pointers,
/// newtypes thereof and null pointer optimized enums are allowed), it is
/// enough to statically check common use cases of transmute.
#[derive(Copy, Clone, Debug)]
pub enum SizeSkeleton<'tcx> {
    /// Any statically computable Layout.
    Known(Size),

    /// A potentially-fat pointer.
    Pointer {
        /// If true, this pointer is never null.
        non_zero: bool,
        /// The type which determines the unsized metadata, if any,
        /// of this pointer. Either a type parameter or a projection
        /// depending on one, with regions erased.
        tail: Ty<'tcx>
    }
}

impl<'tcx> SizeSkeleton<'tcx> {
    pub fn compute(
        ty: Ty<'tcx>,
        tcx: TyCtxt<'tcx>,
        param_env: ty::ParamEnv<'tcx>,
    ) -> Result<SizeSkeleton<'tcx>, LayoutError<'tcx>> {
        debug_assert!(!ty.has_infer_types());

        // First try computing a static layout.
        let err = match tcx.layout_of(param_env.and(ty)) {
            Ok(layout) => {
                return Ok(SizeSkeleton::Known(layout.size));
            }
            Err(err) => err
        };

        match ty.kind {
            ty::Ref(_, pointee, _) |
            ty::RawPtr(ty::TypeAndMut { ty: pointee, .. }) => {
                let non_zero = !ty.is_unsafe_ptr();
                let tail = tcx.struct_tail_erasing_lifetimes(pointee, param_env);
                match tail.kind {
                    ty::Param(_) | ty::Projection(_) => {
                        debug_assert!(tail.has_param_types());
                        Ok(SizeSkeleton::Pointer {
                            non_zero,
                            tail: tcx.erase_regions(&tail)
                        })
                    }
                    _ => {
                        bug!("SizeSkeleton::compute({}): layout errored ({}), yet \
                              tail `{}` is not a type parameter or a projection",
                             ty, err, tail)
                    }
                }
            }

            ty::Adt(def, substs) => {
                // Only newtypes and enums w/ nullable pointer optimization.
                if def.is_union() || def.variants.is_empty() || def.variants.len() > 2 {
                    return Err(err);
                }

                // Get a zero-sized variant or a pointer newtype.
                let zero_or_ptr_variant = |i| {
                    let i = VariantIdx::new(i);
                    let fields = def.variants[i].fields.iter().map(|field| {
                        SizeSkeleton::compute(field.ty(tcx, substs), tcx, param_env)
                    });
                    let mut ptr = None;
                    for field in fields {
                        let field = field?;
                        match field {
                            SizeSkeleton::Known(size) => {
                                if size.bytes() > 0 {
                                    return Err(err);
                                }
                            }
                            SizeSkeleton::Pointer {..} => {
                                if ptr.is_some() {
                                    return Err(err);
                                }
                                ptr = Some(field);
                            }
                        }
                    }
                    Ok(ptr)
                };

                let v0 = zero_or_ptr_variant(0)?;
                // Newtype.
                if def.variants.len() == 1 {
                    if let Some(SizeSkeleton::Pointer { non_zero, tail }) = v0 {
                        return Ok(SizeSkeleton::Pointer {
                            non_zero: non_zero || match tcx.layout_scalar_valid_range(def.did) {
                                (Bound::Included(start), Bound::Unbounded) => start > 0,
                                (Bound::Included(start), Bound::Included(end)) =>
                                    0 < start && start < end,
                                _ => false,
                            },
                            tail,
                        });
                    } else {
                        return Err(err);
                    }
                }

                let v1 = zero_or_ptr_variant(1)?;
                // Nullable pointer enum optimization.
                match (v0, v1) {
                    (Some(SizeSkeleton::Pointer { non_zero: true, tail }), None) |
                    (None, Some(SizeSkeleton::Pointer { non_zero: true, tail })) => {
                        Ok(SizeSkeleton::Pointer {
                            non_zero: false,
                            tail,
                        })
                    }
                    _ => Err(err)
                }
            }

            ty::Projection(_) | ty::Opaque(..) => {
                let normalized = tcx.normalize_erasing_regions(param_env, ty);
                if ty == normalized {
                    Err(err)
                } else {
                    SizeSkeleton::compute(normalized, tcx, param_env)
                }
            }

            _ => Err(err)
        }
    }

    pub fn same_size(self, other: SizeSkeleton<'_>) -> bool {
        match (self, other) {
            (SizeSkeleton::Known(a), SizeSkeleton::Known(b)) => a == b,
            (SizeSkeleton::Pointer { tail: a, .. },
             SizeSkeleton::Pointer { tail: b, .. }) => a == b,
            _ => false
        }
    }
}

pub trait HasTyCtxt<'tcx>: HasDataLayout {
    fn tcx(&self) -> TyCtxt<'tcx>;
}

pub trait HasParamEnv<'tcx> {
    fn param_env(&self) -> ty::ParamEnv<'tcx>;
}

impl<'tcx> HasDataLayout for TyCtxt<'tcx> {
    fn data_layout(&self) -> &TargetDataLayout {
        &self.data_layout
    }
}

impl<'tcx> HasTyCtxt<'tcx> for TyCtxt<'tcx> {
    fn tcx(&self) -> TyCtxt<'tcx> {
        *self
    }
}

impl<'tcx, C> HasParamEnv<'tcx> for LayoutCx<'tcx, C> {
    fn param_env(&self) -> ty::ParamEnv<'tcx> {
        self.param_env
    }
}

impl<'tcx, T: HasDataLayout> HasDataLayout for LayoutCx<'tcx, T> {
    fn data_layout(&self) -> &TargetDataLayout {
        self.tcx.data_layout()
    }
}

impl<'tcx, T: HasTyCtxt<'tcx>> HasTyCtxt<'tcx> for LayoutCx<'tcx, T> {
    fn tcx(&self) -> TyCtxt<'tcx> {
        self.tcx.tcx()
    }
}

pub trait MaybeResult<T> {
    type Error;

    fn from(x: Result<T, Self::Error>) -> Self;
    fn to_result(self) -> Result<T, Self::Error>;
}

impl<T> MaybeResult<T> for T {
    type Error = !;

    fn from(x: Result<T, Self::Error>) -> Self {
        let Ok(x) = x;
        x
    }
    fn to_result(self) -> Result<T, Self::Error> {
        Ok(self)
    }
}

impl<T, E> MaybeResult<T> for Result<T, E> {
    type Error = E;

    fn from(x: Result<T, Self::Error>) -> Self {
        x
    }
    fn to_result(self) -> Result<T, Self::Error> {
        self
    }
}

pub type TyLayout<'tcx> = ::rustc_target::abi::TyLayout<'tcx, Ty<'tcx>>;

impl<'tcx> LayoutOf for LayoutCx<'tcx, TyCtxt<'tcx>> {
    type Ty = Ty<'tcx>;
    type TyLayout = Result<TyLayout<'tcx>, LayoutError<'tcx>>;

    /// Computes the layout of a type. Note that this implicitly
    /// executes in "reveal all" mode.
    fn layout_of(&self, ty: Ty<'tcx>) -> Self::TyLayout {
        let param_env = self.param_env.with_reveal_all();
        let ty = self.tcx.normalize_erasing_regions(param_env, ty);
        let details = self.tcx.layout_raw(param_env.and(ty))?;
        let layout = TyLayout {
            ty,
            details
        };

        // N.B., this recording is normally disabled; when enabled, it
        // can however trigger recursive invocations of `layout_of`.
        // Therefore, we execute it *after* the main query has
        // completed, to avoid problems around recursive structures
        // and the like. (Admittedly, I wasn't able to reproduce a problem
        // here, but it seems like the right thing to do. -nmatsakis)
        self.record_layout_for_printing(layout);

        Ok(layout)
    }
}

impl LayoutOf for LayoutCx<'tcx, ty::query::TyCtxtAt<'tcx>> {
    type Ty = Ty<'tcx>;
    type TyLayout = Result<TyLayout<'tcx>, LayoutError<'tcx>>;

    /// Computes the layout of a type. Note that this implicitly
    /// executes in "reveal all" mode.
    fn layout_of(&self, ty: Ty<'tcx>) -> Self::TyLayout {
        let param_env = self.param_env.with_reveal_all();
        let ty = self.tcx.normalize_erasing_regions(param_env, ty);
        let details = self.tcx.layout_raw(param_env.and(ty))?;
        let layout = TyLayout {
            ty,
            details
        };

        // N.B., this recording is normally disabled; when enabled, it
        // can however trigger recursive invocations of `layout_of`.
        // Therefore, we execute it *after* the main query has
        // completed, to avoid problems around recursive structures
        // and the like. (Admittedly, I wasn't able to reproduce a problem
        // here, but it seems like the right thing to do. -nmatsakis)
        let cx = LayoutCx {
            tcx: *self.tcx,
            param_env: self.param_env
        };
        cx.record_layout_for_printing(layout);

        Ok(layout)
    }
}

// Helper (inherent) `layout_of` methods to avoid pushing `LayoutCx` to users.
impl TyCtxt<'tcx> {
    /// Computes the layout of a type. Note that this implicitly
    /// executes in "reveal all" mode.
    #[inline]
    pub fn layout_of(self, param_env_and_ty: ty::ParamEnvAnd<'tcx, Ty<'tcx>>)
                     -> Result<TyLayout<'tcx>, LayoutError<'tcx>> {
        let cx = LayoutCx {
            tcx: self,
            param_env: param_env_and_ty.param_env
        };
        cx.layout_of(param_env_and_ty.value)
    }
}

impl ty::query::TyCtxtAt<'tcx> {
    /// Computes the layout of a type. Note that this implicitly
    /// executes in "reveal all" mode.
    #[inline]
    pub fn layout_of(self, param_env_and_ty: ty::ParamEnvAnd<'tcx, Ty<'tcx>>)
                     -> Result<TyLayout<'tcx>, LayoutError<'tcx>> {
        let cx = LayoutCx {
            tcx: self.at(self.span),
            param_env: param_env_and_ty.param_env
        };
        cx.layout_of(param_env_and_ty.value)
    }
}

impl<'tcx, C> TyLayoutMethods<'tcx, C> for Ty<'tcx>
where
    C: LayoutOf<Ty = Ty<'tcx>, TyLayout: MaybeResult<TyLayout<'tcx>>>
        + HasTyCtxt<'tcx>
        + HasParamEnv<'tcx>,
{
    fn for_variant(this: TyLayout<'tcx>, cx: &C, variant_index: VariantIdx) -> TyLayout<'tcx> {
        let details = match this.variants {
            Variants::Single { index } if index == variant_index => this.details,

            Variants::Single { index } => {
                // Deny calling for_variant more than once for non-Single enums.
                if let Ok(layout) = cx.layout_of(this.ty).to_result() {
                    assert_eq!(layout.variants, Variants::Single { index });
                }

                let fields = match this.ty.kind {
                    ty::Adt(def, _) => def.variants[variant_index].fields.len(),
                    _ => bug!()
                };
                let tcx = cx.tcx();
                tcx.intern_layout(LayoutDetails {
                    variants: Variants::Single { index: variant_index },
                    fields: FieldPlacement::Union(fields),
                    abi: Abi::Uninhabited,
                    largest_niche: None,
                    align: tcx.data_layout.i8_align,
                    size: Size::ZERO
                })
            }

            Variants::Multiple { ref variants, .. } => {
                &variants[variant_index]
            }
        };

        assert_eq!(details.variants, Variants::Single { index: variant_index });

        TyLayout {
            ty: this.ty,
            details
        }
    }

    fn field(this: TyLayout<'tcx>, cx: &C, i: usize) -> C::TyLayout {
        let tcx = cx.tcx();
        let discr_layout = |discr: &Scalar| -> C::TyLayout {
            let layout = LayoutDetails::scalar(cx, discr.clone());
            MaybeResult::from(Ok(TyLayout {
                details: tcx.intern_layout(layout),
                ty: discr.value.to_ty(tcx),
            }))
        };

        cx.layout_of(match this.ty.kind {
            ty::Bool |
            ty::Char |
            ty::Int(_) |
            ty::Uint(_) |
            ty::Float(_) |
            ty::FnPtr(_) |
            ty::Never |
            ty::FnDef(..) |
            ty::GeneratorWitness(..) |
            ty::Foreign(..) |
            ty::Dynamic(..) => {
                bug!("TyLayout::field_type({:?}): not applicable", this)
            }

            // Potentially-fat pointers.
            ty::Ref(_, pointee, _) |
            ty::RawPtr(ty::TypeAndMut { ty: pointee, .. }) => {
                assert!(i < this.fields.count());

                // Reuse the fat `*T` type as its own thin pointer data field.
                // This provides information about, e.g., DST struct pointees
                // (which may have no non-DST form), and will work as long
                // as the `Abi` or `FieldPlacement` is checked by users.
                if i == 0 {
                    let nil = tcx.mk_unit();
                    let ptr_ty = if this.ty.is_unsafe_ptr() {
                        tcx.mk_mut_ptr(nil)
                    } else {
                        tcx.mk_mut_ref(tcx.lifetimes.re_static, nil)
                    };
                    return MaybeResult::from(cx.layout_of(ptr_ty).to_result().map(|mut ptr_layout| {
                        ptr_layout.ty = this.ty;
                        ptr_layout
                    }));
                }

                match tcx.struct_tail_erasing_lifetimes(pointee, cx.param_env()).kind {
                    ty::Slice(_) |
                    ty::Str => tcx.types.usize,
                    ty::Dynamic(_, _) => {
                        tcx.mk_imm_ref(
                            tcx.lifetimes.re_static,
                            tcx.mk_array(tcx.types.usize, 3),
                        )
                        /* FIXME: use actual fn pointers
                        Warning: naively computing the number of entries in the
                        vtable by counting the methods on the trait + methods on
                        all parent traits does not work, because some methods can
                        be not object safe and thus excluded from the vtable.
                        Increase this counter if you tried to implement this but
                        failed to do it without duplicating a lot of code from
                        other places in the compiler: 2
                        tcx.mk_tup(&[
                            tcx.mk_array(tcx.types.usize, 3),
                            tcx.mk_array(Option<fn()>),
                        ])
                        */
                    }
                    _ => bug!("TyLayout::field_type({:?}): not applicable", this)
                }
            }

            // Arrays and slices.
            ty::Array(element, _) |
            ty::Slice(element) => element,
            ty::Str => tcx.types.u8,

            // Tuples, generators and closures.
            ty::Closure(def_id, ref substs) => {
                substs.as_closure().upvar_tys(def_id, tcx).nth(i).unwrap()
            }

            ty::Generator(def_id, ref substs, _) => {
                match this.variants {
                    Variants::Single { index } => {
                        substs.as_generator().state_tys(def_id, tcx)
                            .nth(index.as_usize()).unwrap()
                            .nth(i).unwrap()
                    }
                    Variants::Multiple { ref discr, discr_index, .. } => {
                        if i == discr_index {
                            return discr_layout(discr);
                        }
                        substs.as_generator().prefix_tys(def_id, tcx).nth(i).unwrap()
                    }
                }
            }

            ty::Tuple(tys) => tys[i].expect_ty(),

            // SIMD vector types.
            ty::Adt(def, ..) if def.repr.simd() => {
                this.ty.simd_type(tcx)
            }

            // ADTs.
            ty::Adt(def, substs) => {
                match this.variants {
                    Variants::Single { index } => {
                        def.variants[index].fields[i].ty(tcx, substs)
                    }

                    // Discriminant field for enums (where applicable).
                    Variants::Multiple { ref discr, .. } => {
                        assert_eq!(i, 0);
                        return discr_layout(discr);
                    }
                }
            }

            ty::Projection(_) | ty::UnnormalizedProjection(..) | ty::Bound(..) |
            ty::Placeholder(..) | ty::Opaque(..) | ty::Param(_) | ty::Infer(_) |
            ty::Error => {
                bug!("TyLayout::field_type: unexpected type `{}`", this.ty)
            }
        })
    }

    fn pointee_info_at(
        this: TyLayout<'tcx>,
        cx: &C,
        offset: Size,
    ) -> Option<PointeeInfo> {
        match this.ty.kind {
            ty::RawPtr(mt) if offset.bytes() == 0 => {
                cx.layout_of(mt.ty).to_result().ok()
                    .map(|layout| PointeeInfo {
                        size: layout.size,
                        align: layout.align.abi,
                        safe: None,
                    })
            }

            ty::Ref(_, ty, mt) if offset.bytes() == 0 => {
                let tcx = cx.tcx();
                let is_freeze = ty.is_freeze(tcx, cx.param_env(), DUMMY_SP);
                let kind = match mt {
                    hir::Mutability::Immutable => if is_freeze {
                        PointerKind::Frozen
                    } else {
                        PointerKind::Shared
                    },
                    hir::Mutability::Mutable => {
                        // Previously we would only emit noalias annotations for LLVM >= 6 or in
                        // panic=abort mode. That was deemed right, as prior versions had many bugs
                        // in conjunction with unwinding, but later versions didn’t seem to have
                        // said issues. See issue #31681.
                        //
                        // Alas, later on we encountered a case where noalias would generate wrong
                        // code altogether even with recent versions of LLVM in *safe* code with no
                        // unwinding involved. See #54462.
                        //
                        // For now, do not enable mutable_noalias by default at all, while the
                        // issue is being figured out.
                        let mutable_noalias = tcx.sess.opts.debugging_opts.mutable_noalias
                            .unwrap_or(false);
                        if mutable_noalias {
                            PointerKind::UniqueBorrowed
                        } else {
                            PointerKind::Shared
                        }
                    }
                };

                cx.layout_of(ty).to_result().ok()
                    .map(|layout| PointeeInfo {
                        size: layout.size,
                        align: layout.align.abi,
                        safe: Some(kind),
                    })
            }

            _ => {
                let mut data_variant = match this.variants {
                    // Within the discriminant field, only the niche itself is
                    // always initialized, so we only check for a pointer at its
                    // offset.
                    //
                    // If the niche is a pointer, it's either valid (according
                    // to its type), or null (which the niche field's scalar
                    // validity range encodes).  This allows using
                    // `dereferenceable_or_null` for e.g., `Option<&T>`, and
                    // this will continue to work as long as we don't start
                    // using more niches than just null (e.g., the first page of
                    // the address space, or unaligned pointers).
                    Variants::Multiple {
                        discr_kind: DiscriminantKind::Niche {
                            dataful_variant,
                            ..
                        },
                        discr_index,
                        ..
                    } if this.fields.offset(discr_index) == offset =>
                        Some(this.for_variant(cx, dataful_variant)),
                    _ => Some(this),
                };

                if let Some(variant) = data_variant {
                    // We're not interested in any unions.
                    if let FieldPlacement::Union(_) = variant.fields {
                        data_variant = None;
                    }
                }

                let mut result = None;

                if let Some(variant) = data_variant {
                    let ptr_end = offset + Pointer.size(cx);
                    for i in 0..variant.fields.count() {
                        let field_start = variant.fields.offset(i);
                        if field_start <= offset {
                            let field = variant.field(cx, i);
                            result = field.to_result().ok()
                                .and_then(|field| {
                                    if ptr_end <= field_start + field.size {
                                        // We found the right field, look inside it.
                                        field.pointee_info_at(cx, offset - field_start)
                                    } else {
                                        None
                                    }
                                });
                            if result.is_some() {
                                break;
                            }
                        }
                    }
                }

                // FIXME(eddyb) This should be for `ptr::Unique<T>`, not `Box<T>`.
                if let Some(ref mut pointee) = result {
                    if let ty::Adt(def, _) = this.ty.kind {
                        if def.is_box() && offset.bytes() == 0 {
                            pointee.safe = Some(PointerKind::UniqueOwned);
                        }
                    }
                }

                result
            }
        }
    }
}

impl<'a, 'tcx> HashStable<StableHashingContext<'a>> for LayoutError<'tcx> {
    fn hash_stable(&self, hcx: &mut StableHashingContext<'a>, hasher: &mut StableHasher) {
        use crate::ty::layout::LayoutError::*;
        mem::discriminant(self).hash_stable(hcx, hasher);

        match *self {
            Unknown(t) |
            SizeOverflow(t) => t.hash_stable(hcx, hasher)
        }
    }
}

pub trait FnAbiExt<'tcx, C>
where
    C: LayoutOf<Ty = Ty<'tcx>, TyLayout = TyLayout<'tcx>>
        + HasDataLayout
        + HasTargetSpec
        + HasTyCtxt<'tcx>
        + HasParamEnv<'tcx>,
{
    fn of_instance(cx: &C, instance: ty::Instance<'tcx>) -> Self;
    fn new(cx: &C, sig: ty::FnSig<'tcx>, extra_args: &[Ty<'tcx>]) -> Self;
    fn new_vtable(cx: &C, sig: ty::FnSig<'tcx>, extra_args: &[Ty<'tcx>]) -> Self;
    fn new_internal(
        cx: &C,
        sig: ty::FnSig<'tcx>,
        extra_args: &[Ty<'tcx>],
        mk_arg_type: impl Fn(Ty<'tcx>, Option<usize>) -> ArgAbi<'tcx, Ty<'tcx>>,
    ) -> Self;
    fn adjust_for_abi(&mut self, cx: &C, abi: SpecAbi);
}

impl<'tcx, C> FnAbiExt<'tcx, C> for call::FnAbi<'tcx, Ty<'tcx>>
where
    C: LayoutOf<Ty = Ty<'tcx>, TyLayout = TyLayout<'tcx>>
        + HasDataLayout
        + HasTargetSpec
        + HasTyCtxt<'tcx>
        + HasParamEnv<'tcx>,
{
    fn of_instance(cx: &C, instance: ty::Instance<'tcx>) -> Self {
        let sig = instance.fn_sig(cx.tcx());
        let sig = cx
            .tcx()
            .normalize_erasing_late_bound_regions(ty::ParamEnv::reveal_all(), &sig);
        call::FnAbi::new(cx, sig, &[])
    }

    fn new(cx: &C, sig: ty::FnSig<'tcx>, extra_args: &[Ty<'tcx>]) -> Self {
        call::FnAbi::new_internal(cx, sig, extra_args, |ty, _| ArgAbi::new(cx.layout_of(ty)))
    }

    fn new_vtable(cx: &C, sig: ty::FnSig<'tcx>, extra_args: &[Ty<'tcx>]) -> Self {
        FnAbiExt::new_internal(cx, sig, extra_args, |ty, arg_idx| {
            let mut layout = cx.layout_of(ty);
            // Don't pass the vtable, it's not an argument of the virtual fn.
            // Instead, pass just the data pointer, but give it the type `*const/mut dyn Trait`
            // or `&/&mut dyn Trait` because this is special-cased elsewhere in codegen
            if arg_idx == Some(0) {
                let fat_pointer_ty = if layout.is_unsized() {
                    // unsized `self` is passed as a pointer to `self`
                    // FIXME (mikeyhew) change this to use &own if it is ever added to the language
                    cx.tcx().mk_mut_ptr(layout.ty)
                } else {
                    match layout.abi {
                        Abi::ScalarPair(..) => (),
                        _ => bug!("receiver type has unsupported layout: {:?}", layout),
                    }

                    // In the case of Rc<Self>, we need to explicitly pass a *mut RcBox<Self>
                    // with a Scalar (not ScalarPair) ABI. This is a hack that is understood
                    // elsewhere in the compiler as a method on a `dyn Trait`.
                    // To get the type `*mut RcBox<Self>`, we just keep unwrapping newtypes until we
                    // get a built-in pointer type
                    let mut fat_pointer_layout = layout;
                    'descend_newtypes: while !fat_pointer_layout.ty.is_unsafe_ptr()
                        && !fat_pointer_layout.ty.is_region_ptr()
                    {
                        'iter_fields: for i in 0..fat_pointer_layout.fields.count() {
                            let field_layout = fat_pointer_layout.field(cx, i);

                            if !field_layout.is_zst() {
                                fat_pointer_layout = field_layout;
                                continue 'descend_newtypes;
                            }
                        }

                        bug!(
                            "receiver has no non-zero-sized fields {:?}",
                            fat_pointer_layout
                        );
                    }

                    fat_pointer_layout.ty
                };

                // we now have a type like `*mut RcBox<dyn Trait>`
                // change its layout to that of `*mut ()`, a thin pointer, but keep the same type
                // this is understood as a special case elsewhere in the compiler
                let unit_pointer_ty = cx.tcx().mk_mut_ptr(cx.tcx().mk_unit());
                layout = cx.layout_of(unit_pointer_ty);
                layout.ty = fat_pointer_ty;
            }
            ArgAbi::new(layout)
        })
    }

    fn new_internal(
        cx: &C,
        sig: ty::FnSig<'tcx>,
        extra_args: &[Ty<'tcx>],
        mk_arg_type: impl Fn(Ty<'tcx>, Option<usize>) -> ArgAbi<'tcx, Ty<'tcx>>,
    ) -> Self {
        debug!("FnAbi::new_internal({:?}, {:?})", sig, extra_args);

        use rustc_target::spec::abi::Abi::*;
        let conv = match cx.tcx().sess.target.target.adjust_abi(sig.abi) {
            RustIntrinsic | PlatformIntrinsic | Rust | RustCall => Conv::C,

            // It's the ABI's job to select this, not ours.
            System => bug!("system abi should be selected elsewhere"),
            EfiApi => bug!("eficall abi should be selected elsewhere"),

            Stdcall => Conv::X86Stdcall,
            Fastcall => Conv::X86Fastcall,
            Vectorcall => Conv::X86VectorCall,
            Thiscall => Conv::X86ThisCall,
            C => Conv::C,
            Unadjusted => Conv::C,
            Win64 => Conv::X86_64Win64,
            SysV64 => Conv::X86_64SysV,
            Aapcs => Conv::ArmAapcs,
            PtxKernel => Conv::PtxKernel,
            Msp430Interrupt => Conv::Msp430Intr,
            X86Interrupt => Conv::X86Intr,
            AmdGpuKernel => Conv::AmdGpuKernel,

            // These API constants ought to be more specific...
            Cdecl => Conv::C,
        };

        let mut inputs = sig.inputs();
        let extra_args = if sig.abi == RustCall {
            assert!(!sig.c_variadic && extra_args.is_empty());

            match sig.inputs().last().unwrap().kind {
                ty::Tuple(tupled_arguments) => {
                    inputs = &sig.inputs()[0..sig.inputs().len() - 1];
                    tupled_arguments.iter().map(|k| k.expect_ty()).collect()
                }
                _ => {
                    bug!(
                        "argument to function with \"rust-call\" ABI \
                         is not a tuple"
                    );
                }
            }
        } else {
            assert!(sig.c_variadic || extra_args.is_empty());
            extra_args.to_vec()
        };

        let target = &cx.tcx().sess.target.target;
        let win_x64_gnu =
            target.target_os == "windows" && target.arch == "x86_64" && target.target_env == "gnu";
        let linux_s390x =
            target.target_os == "linux" && target.arch == "s390x" && target.target_env == "gnu";
        let linux_sparc64 =
            target.target_os == "linux" && target.arch == "sparc64" && target.target_env == "gnu";
        let rust_abi = match sig.abi {
            RustIntrinsic | PlatformIntrinsic | Rust | RustCall => true,
            _ => false,
        };

        // Handle safe Rust thin and fat pointers.
        let adjust_for_rust_scalar = |attrs: &mut ArgAttributes,
                                      scalar: &Scalar,
                                      layout: TyLayout<'tcx>,
                                      offset: Size,
                                      is_return: bool| {
            // Booleans are always an i1 that needs to be zero-extended.
            if scalar.is_bool() {
                attrs.set(ArgAttribute::ZExt);
                return;
            }

            // Only pointer types handled below.
            if scalar.value != Pointer {
                return;
            }

            if scalar.valid_range.start() < scalar.valid_range.end() {
                if *scalar.valid_range.start() > 0 {
                    attrs.set(ArgAttribute::NonNull);
                }
            }

            if let Some(pointee) = layout.pointee_info_at(cx, offset) {
                if let Some(kind) = pointee.safe {
                    attrs.pointee_size = pointee.size;
                    attrs.pointee_align = Some(pointee.align);

                    // `Box` pointer parameters never alias because ownership is transferred
                    // `&mut` pointer parameters never alias other parameters,
                    // or mutable global data
                    //
                    // `&T` where `T` contains no `UnsafeCell<U>` is immutable,
                    // and can be marked as both `readonly` and `noalias`, as
                    // LLVM's definition of `noalias` is based solely on memory
                    // dependencies rather than pointer equality
                    let no_alias = match kind {
                        PointerKind::Shared => false,
                        PointerKind::UniqueOwned => true,
                        PointerKind::Frozen | PointerKind::UniqueBorrowed => !is_return,
                    };
                    if no_alias {
                        attrs.set(ArgAttribute::NoAlias);
                    }

                    if kind == PointerKind::Frozen && !is_return {
                        attrs.set(ArgAttribute::ReadOnly);
                    }
                }
            }
        };

        let arg_of = |ty: Ty<'tcx>, arg_idx: Option<usize>| {
            let is_return = arg_idx.is_none();
            let mut arg = mk_arg_type(ty, arg_idx);
            if arg.layout.is_zst() {
                // For some forsaken reason, x86_64-pc-windows-gnu
                // doesn't ignore zero-sized struct arguments.
                // The same is true for s390x-unknown-linux-gnu
                // and sparc64-unknown-linux-gnu.
                if is_return || rust_abi || (!win_x64_gnu && !linux_s390x && !linux_sparc64) {
                    arg.mode = PassMode::Ignore;
                }
            }

            // FIXME(eddyb) other ABIs don't have logic for scalar pairs.
            if !is_return && rust_abi {
                if let Abi::ScalarPair(ref a, ref b) = arg.layout.abi {
                    let mut a_attrs = ArgAttributes::new();
                    let mut b_attrs = ArgAttributes::new();
                    adjust_for_rust_scalar(&mut a_attrs, a, arg.layout, Size::ZERO, false);
                    adjust_for_rust_scalar(
                        &mut b_attrs,
                        b,
                        arg.layout,
                        a.value.size(cx).align_to(b.value.align(cx).abi),
                        false,
                    );
                    arg.mode = PassMode::Pair(a_attrs, b_attrs);
                    return arg;
                }
            }

            if let Abi::Scalar(ref scalar) = arg.layout.abi {
                if let PassMode::Direct(ref mut attrs) = arg.mode {
                    adjust_for_rust_scalar(attrs, scalar, arg.layout, Size::ZERO, is_return);
                }
            }

            arg
        };

        let mut fn_abi = FnAbi {
            ret: arg_of(sig.output(), None),
            args: inputs
                .iter()
                .cloned()
                .chain(extra_args)
                .enumerate()
                .map(|(i, ty)| arg_of(ty, Some(i)))
                .collect(),
            c_variadic: sig.c_variadic,
            conv,
        };
        fn_abi.adjust_for_abi(cx, sig.abi);
        fn_abi
    }

    fn adjust_for_abi(&mut self, cx: &C, abi: SpecAbi) {
        if abi == SpecAbi::Unadjusted {
            return;
        }

        if abi == SpecAbi::Rust
            || abi == SpecAbi::RustCall
            || abi == SpecAbi::RustIntrinsic
            || abi == SpecAbi::PlatformIntrinsic
        {
            let fixup = |arg: &mut ArgAbi<'tcx, Ty<'tcx>>| {
                if arg.is_ignore() {
                    return;
                }

                match arg.layout.abi {
                    Abi::Aggregate { .. } => {}

                    // This is a fun case! The gist of what this is doing is
                    // that we want callers and callees to always agree on the
                    // ABI of how they pass SIMD arguments. If we were to *not*
                    // make these arguments indirect then they'd be immediates
                    // in LLVM, which means that they'd used whatever the
                    // appropriate ABI is for the callee and the caller. That
                    // means, for example, if the caller doesn't have AVX
                    // enabled but the callee does, then passing an AVX argument
                    // across this boundary would cause corrupt data to show up.
                    //
                    // This problem is fixed by unconditionally passing SIMD
                    // arguments through memory between callers and callees
                    // which should get them all to agree on ABI regardless of
                    // target feature sets. Some more information about this
                    // issue can be found in #44367.
                    //
                    // Note that the platform intrinsic ABI is exempt here as
                    // that's how we connect up to LLVM and it's unstable
                    // anyway, we control all calls to it in libstd.
                    Abi::Vector { .. }
                        if abi != SpecAbi::PlatformIntrinsic
                            && cx.tcx().sess.target.target.options.simd_types_indirect =>
                    {
                        arg.make_indirect();
                        return;
                    }

                    _ => return,
                }

                let size = arg.layout.size;
                if arg.layout.is_unsized() || size > Pointer.size(cx) {
                    arg.make_indirect();
                } else {
                    // We want to pass small aggregates as immediates, but using
                    // a LLVM aggregate type for this leads to bad optimizations,
                    // so we pick an appropriately sized integer type instead.
                    arg.cast_to(Reg {
                        kind: RegKind::Integer,
                        size,
                    });
                }
            };
            fixup(&mut self.ret);
            for arg in &mut self.args {
                fixup(arg);
            }
            if let PassMode::Indirect(ref mut attrs, _) = self.ret.mode {
                attrs.set(ArgAttribute::StructRet);
            }
            return;
        }

        if let Err(msg) = self.adjust_for_cabi(cx, abi) {
            cx.tcx().sess.fatal(&msg);
        }
    }
}
