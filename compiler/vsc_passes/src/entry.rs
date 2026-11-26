//! Entry point (`main` or `@start`) discovery (`rustc_passes::entry`).

use vsc_ast::{AttrKind, Attribute};
use vsc_hir::def::DefKind;
use vsc_middle::ty::{EntryFnType, TyCtxt};
use vsc_session::config::CrateType;
use vsc_span::def_id::{CRATE_DEF_ID, DefId};
use vsc_span::sym;

pub fn has_no_main_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| attr.name() == Some(sym::noMain))
}

fn has_start_attr(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| match &attr.kind {
        AttrKind::Normal(n) => n.path.is_ident(sym::start),
        _ => false,
    })
}

pub fn entry_fn(tcx: TyCtxt<'_>, _: ()) -> Option<(DefId, EntryFnType)> {
    let crate_attrs = tcx.owner_attrs(CRATE_DEF_ID);
    let no_main = has_no_main_attr(crate_attrs);

    let mut start_fn: Option<(DefId, vsc_span::Span)> = None;
    let mut main_fn: Option<DefId> = None;

    for (local, info) in tcx.defs.iter_enumerated() {
        if info.kind != DefKind::Fn {
            continue;
        }
        let def_id = local.to_def_id();
        let attrs = tcx.owner_attrs(local);

        if has_start_attr(attrs) {
            if let Some((_, first_span)) = start_fn {
                let mut err = tcx.dcx().struct_span_err(
                    info.span,
                    "multiple `@start` functions defined",
                );
                err.span_note(first_span, "first `@start` function defined here");
                err.emit();
            } else {
                start_fn = Some((def_id, info.span));
            }
        }

        if tcx.opt_parent(def_id) == Some(CRATE_DEF_ID.to_def_id()) && info.name == sym::main {
            main_fn = Some(def_id);
        }
    }

    if let Some((def_id, _)) = start_fn {
        return Some((def_id, EntryFnType::Start));
    }

    if no_main {
        return None;
    }

    main_fn.map(|def_id| (def_id, EntryFnType::Main { sigpipe: 0 }))
}

/// Verifies that an entry point exists when building an executable crate.
pub fn check_entry_fn(tcx: TyCtxt<'_>) {
    let is_exe = tcx.options.effective_crate_types().contains(&CrateType::Executable);
    if !is_exe {
        return;
    }
    let no_main = has_no_main_attr(tcx.owner_attrs(CRATE_DEF_ID));
    if no_main {
        return;
    }
    let Some((entry_def, _)) = tcx.entry_fn(()) else {
        tcx.dcx().span_err(
            tcx.def_span(CRATE_DEF_ID.to_def_id()),
            "`main` function not found in package",
        );
        return;
    };
    if let Some(local) = entry_def.as_local()
        && matches!(
            tcx.hir_node(local),
            Some(vsc_middle::hir::map::Node::Item(vsc_hir::Item {
                kind: vsc_hir::ItemKind::Fn { sig, .. },
                ..
            })) if sig.header.is_async
        )
        && tcx.lang_items().get(vsc_hir::LangItem::BlockOn).is_none()
    {
        tcx.dcx().span_err(
            tcx.def_span(entry_def),
            "`async func main` requires a runtime defining `@lang(\"blockOn\")` (e.g. `import \"runtime\"`)",
        );
    }
}
