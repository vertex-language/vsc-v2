//! Aliasing overlap check between two MIR [`Place`]s (`rustc_borrowck::places_conflict`).

use vsc_middle::mir::{Body, Place, PlaceElem};
use vsc_middle::ty::{AdtKind, TyCtxt, TyKind};
use vsc_mir_dataflow::place_ty;

/// Returns whether accessing `p1` and `p2` touches overlapping memory/places.
pub fn places_conflict<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    p1: &Place<'tcx>,
    p2: &Place<'tcx>,
) -> bool {
    if p1.local != p2.local {
        return false;
    }

    let mut ty1 = body.local_decls[p1.local].ty;
    let mut prefix1 = Place::from_local(p1.local);
    for (e1, e2) in p1.projection.iter().zip(p2.projection.iter()) {
        match (e1, e2) {
            (PlaceElem::Deref, PlaceElem::Deref) => {}
            (PlaceElem::Field(f1, _), PlaceElem::Field(f2, _)) => {
                let is_union = matches!(ty1.kind(), TyKind::Adt(adt, _) if adt.kind() == AdtKind::Union);
                if !is_union && f1 != f2 {
                    return false;
                }
            }
            (PlaceElem::Downcast(v1), PlaceElem::Downcast(v2)) => {
                if v1 != v2 {
                    return false;
                }
            }
            (
                PlaceElem::ConstantIndex { offset: o1, from_end: f1, .. },
                PlaceElem::ConstantIndex { offset: o2, from_end: f2, .. },
            ) if f1 == f2 => {
                if o1 != o2 {
                    return false;
                }
            }
            (PlaceElem::Deref, _) | (_, PlaceElem::Deref) => return true,
            _ => {}
        }
        prefix1.projection.push(*e1);
        ty1 = place_ty(tcx, body, &prefix1);
    }

    true
}
