//! Computing every signature, and checking trait implementations
//! (rustc's `check/` and `compare_impl_item`).

use vsc_errors::ErrCode;
use vsc_hir::def::DefKind;
use vsc_hir::{self as hir};
use vsc_middle::hir::map::Node;
use vsc_middle::ty::*;
use vsc_span::def_id::DefId;

pub(crate) fn check_items(tcx: TyCtxt<'_>) {
    for (local, info) in tcx.defs.iter_enumerated() {
        let def = local.to_def_id();
        match info.kind {
            DefKind::Struct | DefKind::Enum | DefKind::Union | DefKind::Class => {
                tcx.predicates_of(def);
                for v in tcx.adt_def(def).variants() {
                    for f in &v.fields {
                        tcx.type_of(f.did);
                    }
                }
            }
            DefKind::Fn | DefKind::AssocFn => {
                tcx.predicates_of(def);
                tcx.fn_sig(def);
            }
            DefKind::Const | DefKind::Static { .. } | DefKind::AssocConst | DefKind::TyAlias | DefKind::ForeignTy => {
                tcx.type_of(def);
            }
            DefKind::AssocTy => {
                tcx.predicates_of(def);
                tcx.type_of(def);
            }
            DefKind::Trait => {
                tcx.predicates_of(def);
                tcx.associated_items(def);
            }
            DefKind::Impl { of_trait } => {
                tcx.predicates_of(def);
                tcx.type_of(def);
                if of_trait {
                    if let Some(tr) = tcx.impl_trait_ref(def) {
                        check_impl_items(tcx, def, tr);
                    }
                }
            }
            DefKind::TyParam | DefKind::ConstParam => {
                if let Some(Node::GenericParam(p)) = tcx.hir_node(local) {
                    if matches!(p.kind, hir::GenericParamKind::Type { default: Some(_) } | hir::GenericParamKind::Const { .. }) {
                        tcx.type_of(def);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Does a trait's item have a default (a body or a value)?
fn has_default(tcx: TyCtxt<'_>, item: DefId) -> bool {
    match item.as_local().and_then(|d| tcx.hir_node(d)) {
        Some(Node::AssocItem(a)) => match &a.kind {
            hir::AssocItemKind::Fn(_, body) => body.is_some(),
            hir::AssocItemKind::Const(_, body) => body.is_some(),
            hir::AssocItemKind::Type(_, ty) => ty.is_some(),
        },
        _ => true,
    }
}

fn check_impl_items(tcx: TyCtxt<'_>, imp: DefId, tr: TraitRef<'_>) {
    if !tr.def_id.is_local() {
        return;
    }
    let negative = matches!(
        imp.as_local().and_then(|d| tcx.hir_node(d)),
        Some(Node::Item(hir::Item { kind: hir::ItemKind::Impl(hir::Impl { negative: true, .. }), .. }))
    );
    let trait_items = tcx.associated_items(tr.def_id);
    let impl_items = tcx.associated_items(imp);
    let trait_name = tcx.def_path_str(tr.def_id);
    let same_kind = |a: AssocKind, b: AssocKind| std::mem::discriminant(&a) == std::mem::discriminant(&b);
    for item in impl_items {
        if trait_items.iter().any(|t| t.name == item.name && same_kind(t.kind, item.kind)) {
            continue;
        }
        let (code, what) = match item.kind {
            AssocKind::Fn { .. } => (407, "method"),
            AssocKind::Type => (437, "type"),
            AssocKind::Const => (438, "const"),
        };
        let span = tcx.def_span(item.def_id);
        tcx.dcx()
            .struct_span_err(span, format!("{what} `{}` is not a member of trait `{trait_name}`", item.name))
            .with_code(ErrCode(code))
            .with_span_label(span, format!("not a member of trait `{trait_name}`"))
            .emit();
    }
    if negative {
        return;
    }
    let missing: Vec<String> = trait_items
        .iter()
        .filter(|t| !has_default(tcx, t.def_id))
        .filter(|t| !impl_items.iter().any(|i| i.name == t.name && same_kind(i.kind, t.kind)))
        .map(|t| format!("`{}`", t.name))
        .collect();
    if !missing.is_empty() {
        let span = tcx.def_span(imp);
        tcx.dcx()
            .struct_span_err(span, format!("not all trait items implemented, missing: {}", missing.join(", ")))
            .with_code(ErrCode(46))
            .with_span_label(span, format!("missing {} in implementation", missing.join(", ")))
            .emit();
    }
}
