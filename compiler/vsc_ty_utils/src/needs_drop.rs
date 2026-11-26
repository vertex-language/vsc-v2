//! Checking whether a type requires non-trivial drop glue (`rustc_ty_utils::needs_drop`).

use vsc_data_structures::FxHashSet;
use vsc_hir::LangItem;
use vsc_middle::ty::{AdtKind, ConstKind, Ty, TyCtxt, TyKind};

pub(crate) fn needs_drop_raw<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> bool {
    let mut visited = FxHashSet::default();
    ty_needs_drop(tcx, ty, &mut visited)
}

fn ty_needs_drop<'tcx>(
    tcx: TyCtxt<'tcx>,
    ty: Ty<'tcx>,
    visited: &mut FxHashSet<Ty<'tcx>>,
) -> bool {
    let ty = tcx.erase_regions(ty);
    if !visited.insert(ty) {
        return false;
    }

    match *ty.kind() {
        TyKind::Bool
        | TyKind::Char
        | TyKind::Int(_)
        | TyKind::Uint(_)
        | TyKind::Float(_)
        | TyKind::Str
        | TyKind::Never
        | TyKind::FnDef(..)
        | TyKind::FnPtr(_)
        | TyKind::RawPtr(..)
        | TyKind::Ref(..)
        | TyKind::Foreign(_)
        | TyKind::Error => false,

        TyKind::Tuple(tys) => tys.iter().any(|&elem| ty_needs_drop(tcx, elem, visited)),

        TyKind::Array(elem, len) => {
            if let ConstKind::Value(_, 0) = *len.kind() {
                return false;
            }
            ty_needs_drop(tcx, elem, visited)
        }

        TyKind::Slice(elem) => ty_needs_drop(tcx, elem, visited),

        TyKind::Closure(def_id, _) => {
            tcx.closure_upvars(def_id).iter().any(|&upvar| ty_needs_drop(tcx, upvar, visited))
        }

        TyKind::Coroutine(..) => true,

        TyKind::Opaque(..) => {
            let revealed = tcx.reveal_opaque(ty);
            if revealed == ty {
                true
            } else {
                ty_needs_drop(tcx, revealed, visited)
            }
        }

        TyKind::Adt(adt_def, args) => {
            if tcx.is_lang_item(adt_def.did(), LangItem::ManuallyDrop)
                || tcx.is_lang_item(adt_def.did(), LangItem::PhantomData)
            {
                return false;
            }
            if adt_def.destructor(tcx).is_some() {
                return true;
            }
            if adt_def.kind() == AdtKind::Union {
                return false;
            }
            adt_def.variants().iter().any(|variant| {
                variant.fields.iter().any(|field| {
                    let fty = field.ty(tcx, args);
                    ty_needs_drop(tcx, fty, visited)
                })
            })
        }

        TyKind::Alias(alias) => {
            let infcx = vsc_infer::InferCtxt::new(tcx);
            let param_env = vsc_trait_selection::ParamEnv::default();
            let selcx = vsc_trait_selection::SelectionCtxt::new(&infcx, &param_env);
            if let Some((norm, _)) = selcx.normalize_projection(alias, vsc_span::DUMMY_SP) {
                let norm = tcx.reveal_opaque(tcx.erase_regions(infcx.resolve_vars_if_possible(norm)));
                if norm != ty && norm != tcx.types.error {
                    return ty_needs_drop(tcx, norm, visited);
                }
            }
            true
        }

        // Class references use runtime RC and dynamic drop; parameters conservatively may drop.
        TyKind::Dynamic(..) | TyKind::Param(_) | TyKind::Infer(_) => true,
    }
}
