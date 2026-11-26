//! MIR borrow checker (rustc's `rustc_borrowck`): initialization, move,
//! mutability, aliasing/loan conflict, and escaping-reference checking.

mod borrow_set;
mod check;
mod places_conflict;

#[cfg(test)]
mod tests;

use vsc_hir::def::DefKind;
use vsc_middle::query::Providers;
use vsc_middle::ty::TyCtxt;
use vsc_span::def_id::DefId;
use vsc_trait_selection::ParamEnv;

pub fn provide(providers: &mut Providers) {
    providers.mir_borrowck = mir_borrowck;
}

fn mir_borrowck<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId) {
    let body = tcx.mir_built(def_id);
    let param_env = ParamEnv::for_item(tcx, def_id);
    check::check_body(tcx, def_id, &param_env, body);
}

/// Runs `mir_borrowck` on every body in the current crate.
pub fn check_crate(tcx: TyCtxt<'_>) {
    for (local, info) in tcx.defs.iter_enumerated() {
        let has_body = tcx.body_owned_by(local).is_some() || info.kind == DefKind::Closure;
        if has_body && info.kind != DefKind::InlineConst {
            tcx.mir_borrowck(local.to_def_id());
        }
    }
}
