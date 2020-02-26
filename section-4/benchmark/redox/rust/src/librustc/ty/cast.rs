// Helpers for handling cast expressions, used in both
// typeck and codegen.

use crate::ty::{self, Ty};

use syntax::ast;
use rustc_macros::HashStable;

/// Types that are represented as ints.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IntTy {
    U(ast::UintTy),
    I,
    CEnum,
    Bool,
    Char
}

// Valid types for the result of a non-coercion cast
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CastTy<'tcx> {
    /// Various types that are represented as ints and handled mostly
    /// in the same way, merged for easier matching.
    Int(IntTy),
    /// Floating-Point types
    Float,
    /// Function Pointers
    FnPtr,
    /// Raw pointers
    Ptr(ty::TypeAndMut<'tcx>),
    /// References
    RPtr(ty::TypeAndMut<'tcx>),
}

/// Cast Kind. See RFC 401 (or librustc_typeck/check/cast.rs)
#[derive(Copy, Clone, Debug, RustcEncodable, RustcDecodable, HashStable)]
pub enum CastKind {
    CoercionCast,
    PtrPtrCast,
    PtrAddrCast,
    AddrPtrCast,
    NumericCast,
    EnumCast,
    PrimIntCast,
    U8CharCast,
    ArrayPtrCast,
    FnPtrPtrCast,
    FnPtrAddrCast
}

impl<'tcx> CastTy<'tcx> {
    /// Returns `Some` for integral/pointer casts.
    /// casts like unsizing casts will return `None`
    pub fn from_ty(t: Ty<'tcx>) -> Option<CastTy<'tcx>> {
        match t.kind {
            ty::Bool => Some(CastTy::Int(IntTy::Bool)),
            ty::Char => Some(CastTy::Int(IntTy::Char)),
            ty::Int(_) => Some(CastTy::Int(IntTy::I)),
            ty::Infer(ty::InferTy::IntVar(_)) => Some(CastTy::Int(IntTy::I)),
            ty::Infer(ty::InferTy::FloatVar(_)) => Some(CastTy::Float),
            ty::Uint(u) => Some(CastTy::Int(IntTy::U(u))),
            ty::Float(_) => Some(CastTy::Float),
            ty::Adt(d,_) if d.is_enum() && d.is_payloadfree() =>
                Some(CastTy::Int(IntTy::CEnum)),
            ty::RawPtr(mt) => Some(CastTy::Ptr(mt)),
            ty::Ref(_, ty, mutbl) => Some(CastTy::RPtr(ty::TypeAndMut { ty, mutbl })),
            ty::FnPtr(..) => Some(CastTy::FnPtr),
            _ => None,
        }
    }
}
