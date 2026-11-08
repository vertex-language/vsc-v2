//! Building MIR (rustc's `rustc_mir_build`): THIR, the checks that run on
//! it (`check_unsafety`, and `check_match` for exhaustiveness and
//! reachability of patterns), then MIR from THIR (`builder`).

pub mod builder;
mod check_match;
mod check_unsafety;
pub mod thir;
pub mod validate;

#[cfg(test)]
mod tests;

use vsc_hir::def::DefKind;
use vsc_middle::query::Providers;
use vsc_middle::ty::TyCtxt;
use vsc_span::def_id::LocalDefId;

pub fn provide(providers: &mut Providers) {
    providers.thir_body = thir::cx::thir_body;
    providers.mir_built = builder::mir_built;
}

pub(crate) fn body_of(tcx: TyCtxt<'_>, def: LocalDefId) -> Option<vsc_hir::BodyId> {
    tcx.body_owned_by(def)
}

/// Runs the THIR checks on every body.
pub fn check_crate(tcx: TyCtxt<'_>) {
    for (local, info) in tcx.defs.iter_enumerated() {
        if body_of(tcx, local).is_none() || matches!(info.kind, DefKind::Closure | DefKind::InlineConst) {
            continue;
        }
        check_unsafety::check_unsafety(tcx, local.to_def_id());
        check_match::check_match(tcx, local.to_def_id());
    }
}

/// `-Z unpretty=thir-tree`: every body's THIR, function literals' after
/// their enclosing body's.
pub fn thir_tree(tcx: TyCtxt<'_>) -> String {
    let mut out = String::new();
    for (local, info) in tcx.defs.iter_enumerated() {
        let has_body = body_of(tcx, local).is_some() || info.kind == DefKind::Closure;
        if !has_body || info.kind == DefKind::InlineConst {
            continue;
        }
        out.push_str(&format!("// {}\n{:#?}\n", tcx.def_path_str(local.to_def_id()), tcx.thir_body(local.to_def_id())));
    }
    out
}

/// Builds every body's MIR (function literals' too).
pub fn build_mir(tcx: TyCtxt<'_>) {
    for (local, info) in tcx.defs.iter_enumerated() {
        let has_body = body_of(tcx, local).is_some() || info.kind == DefKind::Closure;
        if has_body && info.kind != DefKind::InlineConst {
            tcx.mir_built(local.to_def_id());
        }
    }
}

/// `-Z dump-mir`: every body's MIR.
pub fn dump_mir(tcx: TyCtxt<'_>, skip_file: impl Fn(&str) -> bool) -> String {
    let mut out = String::new();
    for (local, info) in tcx.defs.iter_enumerated() {
        let has_body = body_of(tcx, local).is_some() || info.kind == DefKind::Closure;
        if !has_body || matches!(info.kind, DefKind::InlineConst | DefKind::AnonConst) {
            continue;
        }
        if let Some(f) = tcx.sess.source_map.lookup_source_file(info.span.lo()) {
            if skip_file(&f.name.to_string()) {
                continue;
            }
        }
        out.push_str(&tcx.mir_to_string(tcx.mir_built(local.to_def_id())));
        out.push('\n');
    }
    out
}

/// `-Z validate-mir`: reports malformed MIR, which is a bug in vsc.
pub fn validate_all(tcx: TyCtxt<'_>) {
    for (local, info) in tcx.defs.iter_enumerated() {
        let has_body = body_of(tcx, local).is_some() || info.kind == DefKind::Closure;
        if !has_body || info.kind == DefKind::InlineConst {
            continue;
        }
        let body = tcx.mir_built(local.to_def_id());
        for e in validate::validate(tcx, body) {
            tcx.dcx()
                .struct_span_err(body.span, format!("invalid MIR in `{}`: {e}", tcx.def_path_str(local.to_def_id())))
                .with_note("this is a bug in vsc")
                .emit();
        }
    }
}
