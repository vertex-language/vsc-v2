//! Instance resolution (`rustc_ty_utils::instance`).

use vsc_hir::def::DefKind;
use vsc_infer::InferCtxt;
use vsc_middle::ty::{
    GenericArg, GenericArgsRef, Instance, InstanceKind, TraitRef, TyCtxt, TyKind,
};
use vsc_span::DUMMY_SP;
use vsc_span::def_id::DefId;
use vsc_trait_selection::{Candidate, ParamEnv, Selection, SelectionCtxt};

pub(crate) fn resolve_instance_raw<'tcx>(
    tcx: TyCtxt<'tcx>,
    (def_id, args): (DefId, GenericArgsRef<'tcx>),
) -> Option<Instance<'tcx>> {
    let args = tcx.erase_regions_args(args);
    let revealed_args: Vec<GenericArg<'tcx>> = args
        .iter()
        .map(|a| match *a {
            GenericArg::Type(t) => GenericArg::Type(tcx.reveal_opaque(t)),
            other => other,
        })
        .collect();
    let args = tcx.mk_args(&revealed_args);

    match tcx.def_kind(def_id) {
        DefKind::AssocFn | DefKind::AssocConst => {
            let container = tcx.parent(def_id);
            if tcx.def_kind(container) == DefKind::Trait {
                return resolve_associated_item(tcx, def_id, container, args);
            }
        }
        DefKind::Fn => {
            let parent = tcx.parent(def_id);
            if let Some(local) = parent.as_local()
                && let Some(vsc_middle::hir::map::Node::Item(item)) = tcx.hir_node(local)
                && let vsc_hir::ItemKind::ForeignMod { abi: Some(abi), .. } = &item.kind
                && abi.as_str() == "rust-intrinsic"
            {
                return Some(Instance { def: InstanceKind::Intrinsic(def_id), args });
            }
        }
        _ => {}
    }

    if let Some(abi) = fn_abi_str(tcx, def_id)
        && abi == "rust-intrinsic"
    {
        return Some(Instance { def: InstanceKind::Intrinsic(def_id), args });
    }

    Some(Instance::new_raw(def_id, args))
}

fn fn_abi_str<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> Option<&'tcx str> {
    match tcx.def_kind(def_id) {
        DefKind::Fn | DefKind::AssocFn => tcx.fn_sig(def_id).value.abi.map(|s| s.as_str()),
        _ => None,
    }
}

fn resolve_associated_item<'tcx>(
    tcx: TyCtxt<'tcx>,
    trait_item_id: DefId,
    trait_id: DefId,
    args: GenericArgsRef<'tcx>,
) -> Option<Instance<'tcx>> {
    let trait_generics = tcx.generics_of(trait_id);
    let trait_arg_count = trait_generics.count().min(args.len());
    let trait_args = tcx.mk_args(&args[..trait_arg_count]);
    let self_ty = trait_args.first().map(|a| a.expect_ty()).unwrap_or(tcx.types.error);

    let infcx = InferCtxt::new(tcx);
    let param_env = ParamEnv::default();
    let selcx = SelectionCtxt::new(&infcx, &param_env);
    let trait_ref = TraitRef { def_id: trait_id, args: trait_args };

    match selcx.select(trait_ref, DUMMY_SP) {
        Selection::Selected(Candidate::Impl(impl_def_id, impl_args), _) => {
            let resolved_impl_args: Vec<GenericArg<'tcx>> = impl_args
                .iter()
                .map(|a| match a {
                    GenericArg::Type(t) => {
                        GenericArg::Type(tcx.erase_regions(infcx.resolve_vars_if_possible(*t)))
                    }
                    GenericArg::Region(_) => GenericArg::Region(tcx.regions.re_erased),
                    GenericArg::Const(c) => GenericArg::Const(*c),
                })
                .collect();

            let item_name = tcx.item_name(trait_item_id);
            if let Some(impl_assoc) =
                tcx.associated_items(impl_def_id).iter().find(|a| a.name == item_name)
            {
                let mut combined = resolved_impl_args;
                combined.extend_from_slice(&args[trait_arg_count..]);
                let combined_args = tcx.erase_regions_args(tcx.mk_args(&combined));
                return Some(Instance::new_raw(impl_assoc.def_id, combined_args));
            }
            // Fallback to trait default method body using trait args.
            Some(Instance::new_raw(trait_item_id, args))
        }
        Selection::Selected(Candidate::Builtin, _) => {
            if let TyKind::Closure(closure_def, cargs) = *self_ty.kind() {
                if let Some(kind) = crate::instance::closure_trait_kind(tcx, trait_id) {
                    return Some(Instance::resolve_closure(tcx, closure_def, cargs, kind));
                }
            }
            if let TyKind::Coroutine(coro_def, cargs) = *self_ty.kind() {
                if tcx.is_lang_item(trait_id, vsc_hir::LangItem::Future) {
                    let parent_args = if cargs.len() > 1 { &cargs[..cargs.len() - 1] } else { &[] };
                    return Some(Instance::new_raw(coro_def, tcx.mk_args(parent_args)));
                }
            }
            Some(Instance { def: InstanceKind::Intrinsic(trait_item_id), args })
        }
        _ => Some(Instance::new_raw(trait_item_id, args)),
    }
}

fn closure_trait_kind(tcx: TyCtxt<'_>, trait_id: DefId) -> Option<vsc_middle::ty::ClosureKind> {
    if tcx.is_lang_item(trait_id, vsc_hir::LangItem::Fn) {
        Some(vsc_middle::ty::ClosureKind::Fn)
    } else if tcx.is_lang_item(trait_id, vsc_hir::LangItem::FnMut) {
        Some(vsc_middle::ty::ClosureKind::FnMut)
    } else if tcx.is_lang_item(trait_id, vsc_hir::LangItem::FnOnce) {
        Some(vsc_middle::ty::ClosureKind::FnOnce)
    } else {
        None
    }
}
