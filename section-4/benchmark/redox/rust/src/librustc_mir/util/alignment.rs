use rustc::ty::{self, TyCtxt};
use rustc::mir::*;

/// Returns `true` if this place is allowed to be less aligned
/// than its containing struct (because it is within a packed
/// struct).
pub fn is_disaligned<'tcx, L>(
    tcx: TyCtxt<'tcx>,
    local_decls: &L,
    param_env: ty::ParamEnv<'tcx>,
    place: &Place<'tcx>,
) -> bool
where
    L: HasLocalDecls<'tcx>,
{
    debug!("is_disaligned({:?})", place);
    if !is_within_packed(tcx, local_decls, place) {
        debug!("is_disaligned({:?}) - not within packed", place);
        return false
    }

    let ty = place.ty(local_decls, tcx).ty;
    match tcx.layout_raw(param_env.and(ty)) {
        Ok(layout) if layout.align.abi.bytes() == 1 => {
            // if the alignment is 1, the type can't be further
            // disaligned.
            debug!("is_disaligned({:?}) - align = 1", place);
            false
        }
        _ => {
            debug!("is_disaligned({:?}) - true", place);
            true
        }
    }
}

fn is_within_packed<'tcx, L>(tcx: TyCtxt<'tcx>, local_decls: &L, place: &Place<'tcx>) -> bool
where
    L: HasLocalDecls<'tcx>,
{
    let mut cursor = place.projection.as_ref();
    while let &[ref proj_base @ .., elem] = cursor {
        cursor = proj_base;

        match elem {
            // encountered a Deref, which is ABI-aligned
            ProjectionElem::Deref => break,
            ProjectionElem::Field(..) => {
                let ty = Place::ty_from(&place.base, proj_base, local_decls, tcx).ty;
                match ty.kind {
                    ty::Adt(def, _) if def.repr.packed() => {
                        return true
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    false
}
