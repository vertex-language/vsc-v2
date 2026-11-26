//! MIR optimization and lowering passes (rustc's `rustc_mir_transform`),
//! providing [`optimized_mir`] and [`coroutine_layout`].

mod add_storage_markers;
mod coroutine;
mod elaborate_drops;
mod simplify;

#[cfg(test)]
mod tests;

use vsc_hir::def::DefKind;
use vsc_middle::mir::{Body, CoroutineLayout};
use vsc_middle::query::Providers;
use vsc_middle::ty::TyCtxt;
use vsc_span::def_id::DefId;
use vsc_trait_selection::ParamEnv;

pub fn provide(providers: &mut Providers) {
    providers.optimized_mir = optimized_mir;
    providers.coroutine_layout = coroutine_layout;
}

fn coroutine_layout<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> CoroutineLayout<'tcx> {
    if tcx.def_kind(def_id) != DefKind::Coroutine {
        return CoroutineLayout::default();
    }
    let mut body = tcx.mir_built(def_id).clone();
    let param_env = ParamEnv::for_item(tcx, def_id);
    elaborate_drops::run(tcx, &param_env, &mut body);
    coroutine::compute_layout(tcx, def_id, &body)
}

fn optimized_mir<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) -> Body<'tcx> {
    let mut body = tcx.mir_built(def_id).clone();
    let param_env = ParamEnv::for_item(tcx, def_id);

    elaborate_drops::run(tcx, &param_env, &mut body);
    if tcx.def_kind(def_id) == DefKind::Coroutine {
        coroutine::transform(tcx, def_id, &mut body);
    }
    add_storage_markers::run(&mut body);
    simplify::remove_redundant_copies(&mut body);
    simplify::simplify_cfg(&mut body);

    body
}
