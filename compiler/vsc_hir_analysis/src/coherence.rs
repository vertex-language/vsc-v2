//! Inherent implementations (rustc's `coherence/inherent_impls.rs` and
//! `inherent_impls_overlap.rs`). Exception 8 makes these common: every
//! flat receiver method is its own one-method `implement` block, so two
//! flat methods with the same name on the same type are E0592 here.

use vsc_data_structures::FxIndexMap;
use vsc_errors::ErrCode;
use vsc_hir::LangItem;
use vsc_hir::def::DefKind;
use vsc_middle::ty::*;
use vsc_span::def_id::DefId;

pub(crate) fn check(tcx: TyCtxt<'_>) {
    // Inherent impls, by the type they're for.
    let mut by_type: FxIndexMap<DefId, Vec<DefId>> = FxIndexMap::default();
    for (local, info) in tcx.defs.iter_enumerated() {
        let def = local.to_def_id();
        match info.kind {
            DefKind::Impl { of_trait: false } => {}
            DefKind::Impl { of_trait: true } => {
                check_duplicates_within(tcx, def);
                continue;
            }
            _ => continue,
        }
        check_duplicates_within(tcx, def);
        let self_ty = tcx.type_of(def);
        let key = match *self_ty.kind() {
            TyKind::Adt(adt, args) if tcx.is_lang_item(adt.did(), LangItem::Class) && args.len() == 1 => match *args[0].expect_ty().kind() {
                TyKind::Adt(inner, _) => Some(inner.did()),
                _ => None,
            },
            TyKind::Adt(adt, _) => Some(adt.did()),
            TyKind::Foreign(d) => Some(d),
            TyKind::Error => continue,
            _ => None,
        };
        match key {
            Some(k) => by_type.entry_or_insert_with(k, Vec::new).push(def),
            None => {
                let span = tcx.def_span(def);
                tcx.dcx()
                    .struct_span_err(span, format!("cannot define inherent methods for `{}`", tcx.ty_to_string(self_ty)))
                    .with_code(ErrCode(390))
                    .with_span_label(span, "not a struct, enum, union or class of this crate")
                    .with_help("define a trait with the methods and implement it for the type")
                    .emit();
            }
        }
    }
    for impls in by_type.values() {
        for (i, &a) in impls.iter().enumerate() {
            for &b in &impls[i + 1..] {
                if could_unify(tcx.type_of(a), tcx.type_of(b)) {
                    check_overlap(tcx, a, b);
                }
            }
        }
    }
}

/// Do two items share a namespace? Methods and constants are values.
fn same_ns(a: AssocKind, b: AssocKind) -> bool {
    (a == AssocKind::Type) == (b == AssocKind::Type)
}

fn check_overlap(tcx: TyCtxt<'_>, a: DefId, b: DefId) {
    for x in tcx.associated_items(a) {
        for y in tcx.associated_items(b) {
            if x.name == y.name && same_ns(x.kind, y.kind) {
                let span = tcx.def_span(y.def_id);
                tcx.dcx()
                    .struct_span_err(span, format!("duplicate definitions with name `{}`", y.name))
                    .with_code(ErrCode(592))
                    .with_span_label(span, format!("duplicate definitions for `{}`", y.name))
                    .with_span_note(tcx.def_span(x.def_id), format!("other definition for `{}`", x.name))
                    .emit();
            }
        }
    }
}

fn check_duplicates_within(tcx: TyCtxt<'_>, imp: DefId) {
    let items = tcx.associated_items(imp);
    for (i, x) in items.iter().enumerate() {
        if let Some(y) = items[..i].iter().find(|y| y.name == x.name && same_ns(x.kind, y.kind)) {
            let span = tcx.def_span(x.def_id);
            tcx.dcx()
                .struct_span_err(span, format!("duplicate definitions with name `{}`", x.name))
                .with_code(ErrCode(201))
                .with_span_label(span, "duplicate definition")
                .with_span_note(tcx.def_span(y.def_id), "previous definition here")
                .emit();
        }
    }
}

/// Could the two types be equal for some choice of their parameters?
fn could_unify(a: Ty<'_>, b: Ty<'_>) -> bool {
    match (*a.kind(), *b.kind()) {
        (TyKind::Param(_), _) | (_, TyKind::Param(_)) | (TyKind::Error, _) | (_, TyKind::Error) => true,
        (TyKind::Alias(_), _) | (_, TyKind::Alias(_)) => true,
        (TyKind::Adt(d1, a1), TyKind::Adt(d2, a2)) => {
            d1 == d2
                && a1.iter().zip(a2.iter()).all(|(x, y)| match (x, y) {
                    (GenericArg::Type(x), GenericArg::Type(y)) => could_unify(*x, *y),
                    (GenericArg::Const(x), GenericArg::Const(y)) => {
                        x == y || matches!(x.kind(), ConstKind::Param(_)) || matches!(y.kind(), ConstKind::Param(_))
                    }
                    _ => true,
                })
        }
        (TyKind::Ref(_, x, m1), TyKind::Ref(_, y, m2)) => m1 == m2 && could_unify(x, y),
        (TyKind::RawPtr(x, m1), TyKind::RawPtr(y, m2)) => m1 == m2 && could_unify(x, y),
        (TyKind::Slice(x), TyKind::Slice(y)) | (TyKind::Array(x, _), TyKind::Array(y, _)) => could_unify(x, y),
        (TyKind::Tuple(x), TyKind::Tuple(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| could_unify(*a, *b)),
        _ => a == b,
    }
}
