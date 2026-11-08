//! `-Z dump-signatures`: every item's signature, for tests and debugging
//! (rustc tests these with `#[rustc_dump_*]` attributes).

use std::fmt::Write;

use vsc_hir::def::DefKind;
use vsc_middle::ty::*;
use vsc_span::def_id::DefId;

pub fn dump_signatures(tcx: TyCtxt<'_>) -> String {
    let mut out = String::new();
    for (local, info) in tcx.defs.iter_enumerated() {
        let def = local.to_def_id();
        let path = || tcx.def_path_str(def);
        let line = match info.kind {
            DefKind::Fn | DefKind::AssocFn => format!("fn {}{}: {}", path(), generics(tcx, def), tcx.fn_sig_to_string(tcx.fn_sig(def))),
            DefKind::Struct | DefKind::Enum | DefKind::Union | DefKind::Class => {
                let adt = tcx.adt_def(def);
                let mut s = format!("{} {}{}", adt.descr(), path(), generics(tcx, def));
                for v in adt.variants() {
                    let indent = if adt.is_enum() {
                        let _ = write!(s, "\n    {}", v.name);
                        "        "
                    } else {
                        "    "
                    };
                    for f in &v.fields {
                        let _ = write!(s, "\n{indent}{}: {}", f.name, tcx.ty_to_string(tcx.type_of(f.did)));
                    }
                }
                s
            }
            DefKind::Const | DefKind::AssocConst => format!("const {}: {}", path(), tcx.ty_to_string(tcx.type_of(def))),
            DefKind::Static { mutability } => format!(
                "static {}{}: {}",
                if mutability.is_mut() { "var " } else { "" },
                path(),
                tcx.ty_to_string(tcx.type_of(def))
            ),
            DefKind::TyAlias => format!("type {}{} = {}", path(), generics(tcx, def), tcx.ty_to_string(tcx.type_of(def))),
            DefKind::AssocTy => {
                let ty = tcx.type_of(def);
                match ty.kind() {
                    TyKind::Alias(a) if a.def_id == def => format!("type {}{}", path(), generics(tcx, def)),
                    _ => format!("type {}{} = {}", path(), generics(tcx, def), tcx.ty_to_string(ty)),
                }
            }
            DefKind::ForeignTy => format!("type {}", path()),
            DefKind::Trait => format!("trait {}{}", path(), generics(tcx, def)),
            DefKind::Impl { .. } => {
                let self_ty = tcx.ty_to_string(tcx.type_of(def));
                match tcx.impl_trait_ref(def) {
                    Some(tr) => format!("implement{} {} for {self_ty}", generics(tcx, def), tcx.trait_ref_to_string(tr)),
                    None => format!("implement{} {self_ty}", generics(tcx, def)),
                }
            }
            _ => continue,
        };
        out.push_str(&line);
        out.push('\n');
        if matches!(
            info.kind,
            DefKind::Fn | DefKind::AssocFn | DefKind::Struct | DefKind::Enum | DefKind::Union | DefKind::Class | DefKind::Trait | DefKind::Impl { .. } | DefKind::TyAlias | DefKind::AssocTy
        ) {
            for (clause, _) in tcx.predicates_of(def).predicates {
                let _ = writeln!(out, "    where {}", tcx.clause_to_string(*clause));
            }
        }
    }
    out
}

/// `<T, a, const N: uint>`: the item's own parameters.
fn generics(tcx: TyCtxt<'_>, def: DefId) -> String {
    let g = tcx.generics_of(def);
    let parts: Vec<String> = g
        .own_params
        .iter()
        .filter(|p| !matches!(p.kind, GenericParamDefKind::Type { synthetic: true, .. } if p.name == vsc_span::kw::SelfUpper))
        .map(|p| match p.kind {
            GenericParamDefKind::Lifetime => p.name.to_string(),
            GenericParamDefKind::Type { .. } => p.name.to_string(),
            GenericParamDefKind::Const { .. } => format!("const {}: {}", p.name, tcx.ty_to_string(tcx.type_of(p.def_id))),
        })
        .collect();
    if parts.is_empty() { String::new() } else { format!("<{}>", parts.join(", ")) }
}
