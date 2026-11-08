//! Item signatures (rustc's `collect.rs` and `collect/*`): the providers
//! for `generics_of`, `type_of`, `fn_sig`, `predicates_of`, `adt_def`,
//! `impl_trait_ref` and `associated_items`.

use vsc_data_structures::FxHashMap;
use vsc_hir::def::{CtorKind, DefKind};
use vsc_hir::{self as hir};
use vsc_middle::hir::map::Node;
use vsc_middle::query::Providers;
use vsc_middle::ty::*;
use vsc_span::def_id::DefId;
use vsc_span::{Span, kw, sym};

use crate::hir_ty_lowering::{ElidedRegions, ItemCtxt};

pub fn provide(p: &mut Providers) {
    p.generics_of = generics_of;
    p.type_of = type_of;
    p.fn_sig = fn_sig;
    p.predicates_of = predicates_of;
    p.adt_def = adt_def;
    p.impl_trait_ref = impl_trait_ref;
    p.associated_items = associated_items;
}

fn node<'tcx>(tcx: TyCtxt<'tcx>, def: DefId) -> Option<Node<'tcx>> {
    tcx.hir_node(def.as_local()?)
}

/// The HIR generics an item declares.
pub(crate) fn hir_generics<'tcx>(tcx: TyCtxt<'tcx>, def: DefId) -> Option<&'tcx hir::Generics> {
    match node(tcx, def)? {
        Node::Item(i) => match &i.kind {
            hir::ItemKind::Fn { generics, .. }
            | hir::ItemKind::TyAlias(_, generics)
            | hir::ItemKind::Enum(_, generics)
            | hir::ItemKind::Struct(_, generics)
            | hir::ItemKind::Class(_, generics)
            | hir::ItemKind::Union(_, generics)
            | hir::ItemKind::Const(_, generics, _)
            | hir::ItemKind::Trait { generics, .. } => Some(generics),
            hir::ItemKind::Impl(imp) => Some(&imp.generics),
            _ => None,
        },
        Node::AssocItem(a) => Some(&a.generics),
        Node::ForeignItem(hir::ForeignItem { kind: hir::ForeignItemKind::Fn(_, _, g), .. }) => Some(g),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// generics_of
// ---------------------------------------------------------------------------

fn generics_of(tcx: TyCtxt<'_>, def: DefId) -> Generics {
    let kind = tcx.def_kind(def);
    let parent = match kind {
        DefKind::AssocFn | DefKind::AssocConst | DefKind::AssocTy | DefKind::Variant | DefKind::Field => Some(tcx.parent(def)),
        DefKind::Ctor(..) => {
            let p = tcx.parent(def);
            Some(if tcx.def_kind(p) == DefKind::Variant { tcx.parent(p) } else { p })
        }
        DefKind::Closure | DefKind::AnonConst | DefKind::InlineConst => tcx.opt_parent(def),
        _ => None,
    };
    let parent_count = parent.map_or(0, |p| tcx.generics_of(p).count());
    let mut own_params = Vec::new();
    let mut param_def_id_to_index = FxHashMap::default();
    let has_self = kind == DefKind::Trait;
    if has_self {
        own_params.push(GenericParamDef {
            name: kw::SelfUpper,
            def_id: def,
            index: parent_count as u32,
            kind: GenericParamDefKind::Type { has_default: false, synthetic: true },
        });
    }
    if let Some(g) = hir_generics(tcx, def) {
        for p in &g.params {
            let index = (parent_count + own_params.len()) as u32;
            let kind = match &p.kind {
                hir::GenericParamKind::Lifetime => GenericParamDefKind::Lifetime,
                hir::GenericParamKind::Type { default } => GenericParamDefKind::Type { has_default: default.is_some(), synthetic: false },
                hir::GenericParamKind::Const { default, .. } => GenericParamDefKind::Const { has_default: default.is_some() },
            };
            param_def_id_to_index.insert(p.def_id.to_def_id(), index);
            own_params.push(GenericParamDef { name: p.name.name, def_id: p.def_id.to_def_id(), index, kind });
        }
    }
    Generics { parent, parent_count, own_params, param_def_id_to_index, has_self }
}

// ---------------------------------------------------------------------------
// type_of
// ---------------------------------------------------------------------------

fn type_of<'tcx>(tcx: TyCtxt<'tcx>, def: DefId) -> Ty<'tcx> {
    let lower = |item_def: DefId, elided, t: &hir::Ty| ItemCtxt::new(tcx, item_def, elided).lower_ty(t);
    match tcx.def_kind(def) {
        DefKind::Fn | DefKind::AssocFn | DefKind::Ctor(_, CtorKind::Fn) => tcx.mk_fn_def(def, tcx.identity_args(def)),
        DefKind::Ctor(_, CtorKind::Const) | DefKind::Variant => tcx.type_of(adt_of(tcx, def)),
        DefKind::Struct | DefKind::Enum | DefKind::Union | DefKind::Class => tcx.mk_adt(tcx.adt_def(def), tcx.identity_args(def)),
        DefKind::ForeignTy => tcx.mk_ty(TyKind::Foreign(def)),
        kind => match node(tcx, def) {
            Some(Node::Field(f)) => lower(def, ElidedRegions::Error, &f.ty),
            Some(Node::Item(i)) => match &i.kind {
                hir::ItemKind::Const(t, ..) | hir::ItemKind::Static(t, ..) => lower(def, ElidedRegions::Static, t),
                hir::ItemKind::TyAlias(t, _) => lower(def, ElidedRegions::Error, t),
                hir::ItemKind::Impl(imp) => lower(def, ElidedRegions::Error, &imp.self_ty),
                _ => tcx.types.error,
            },
            Some(Node::ForeignItem(hir::ForeignItem { kind: hir::ForeignItemKind::Static(t, ..), .. })) => {
                lower(def, ElidedRegions::Static, t)
            }
            Some(Node::AssocItem(a)) => match &a.kind {
                hir::AssocItemKind::Const(t, _) => lower(def, ElidedRegions::Static, t),
                hir::AssocItemKind::Type(_, Some(t)) => lower(def, ElidedRegions::Error, t),
                // A trait's associated type without a default is the
                // projection of itself.
                hir::AssocItemKind::Type(_, None) => tcx.mk_ty(TyKind::Alias(AliasTy { def_id: def, args: tcx.identity_args(tcx.parent(def)) })),
                hir::AssocItemKind::Fn(..) => tcx.mk_fn_def(def, tcx.identity_args(def)),
            },
            Some(Node::GenericParam(p)) => {
                let owner = tcx.parent(def);
                if p.from_flat_receiver {
                    if let Some(ty) = flat_receiver_const_ty(tcx, owner, def) {
                        return ty;
                    }
                }
                match &p.kind {
                    hir::GenericParamKind::Type { default: Some(t) } => lower(owner, ElidedRegions::Error, t),
                    hir::GenericParamKind::Const { ty, .. } => lower(owner, ElidedRegions::Error, ty),
                    _ => tcx.types.error,
                }
            }
            _ => panic!("type_of({def:?}): no type for a {}", kind.descr()),
        },
    }
}

/// A flat receiver's const parameter (`N` in `func (s: &Stack<T, N>)`)
/// has the type the receiver type declares at that position.
fn flat_receiver_const_ty<'tcx>(tcx: TyCtxt<'tcx>, imp: DefId, param: DefId) -> Option<Ty<'tcx>> {
    let Some(Node::Item(hir::Item { kind: hir::ItemKind::Impl(i), .. })) = node(tcx, imp) else { return None };
    let hir::TyKind::Path(hir::QPath::Resolved(None, path)) = &i.self_ty.kind else { return None };
    let vsc_hir::def::Res::Def(_, adt) = path.res else { return None };
    let args = path.segments.last()?.args.as_deref()?;
    let position = args.args.iter().position(|a| match a {
        hir::GenericArg::Const(hir::ConstArg { kind: hir::ConstArgKind::Path(hir::QPath::Resolved(None, p)), .. }) => {
            p.res.def_id() == Some(param)
        }
        _ => false,
    })?;
    let declared = tcx.generics_of(adt).own_params.get(position)?;
    matches!(declared.kind, GenericParamDefKind::Const { .. }).then(|| tcx.type_of(declared.def_id))
}

/// The struct or enum a constructor or variant belongs to.
fn adt_of(tcx: TyCtxt<'_>, def: DefId) -> DefId {
    let mut d = def;
    loop {
        match tcx.def_kind(d) {
            DefKind::Struct | DefKind::Enum | DefKind::Union | DefKind::Class => return d,
            _ => d = tcx.parent(d),
        }
    }
}

// ---------------------------------------------------------------------------
// fn_sig
// ---------------------------------------------------------------------------

fn fn_sig<'tcx>(tcx: TyCtxt<'tcx>, def: DefId) -> PolyFnSig<'tcx> {
    let mut icx = ItemCtxt::new(tcx, def, ElidedRegions::Fn);
    match node(tcx, def) {
        Some(Node::Item(hir::Item { kind: hir::ItemKind::Fn { sig, .. }, .. }))
        | Some(Node::AssocItem(hir::AssocItem { kind: hir::AssocItemKind::Fn(sig, _), .. })) => {
            icx.lower_fn_sig(&sig.decl, Some(&sig.header), None)
        }
        Some(Node::ForeignItem(hir::ForeignItem { kind: hir::ForeignItemKind::Fn(sig, ..), .. })) => {
            let abi = match node(tcx, tcx.parent(def)) {
                Some(Node::Item(hir::Item { kind: hir::ItemKind::ForeignMod { abi, .. }, .. })) => abi.unwrap_or(sym::C),
                _ => sym::C,
            };
            icx.lower_fn_sig(&sig.decl, Some(&sig.header), Some(abi))
        }
        Some(Node::Ctor(data)) => {
            let mut io: Vec<Ty<'tcx>> = data.fields().iter().map(|f| tcx.type_of(f.def_id.to_def_id())).collect();
            io.push(tcx.type_of(adt_of(tcx, def)));
            Binder::dummy(FnSig { inputs_and_output: tcx.mk_type_list(&io), c_variadic: false, safety: Safety::Safe, abi: None })
        }
        _ => panic!("fn_sig({def:?}): not a function"),
    }
}

// ---------------------------------------------------------------------------
// predicates_of
// ---------------------------------------------------------------------------

fn predicates_of<'tcx>(tcx: TyCtxt<'tcx>, def: DefId) -> GenericPredicates<'tcx> {
    let generics = tcx.generics_of(def);
    let mut out: Vec<(Clause<'tcx>, Span)> = Vec::new();
    let mut icx = ItemCtxt::new(tcx, def, ElidedRegions::Error);
    let span = tcx.def_span(def);
    match node(tcx, def) {
        Some(Node::Item(hir::Item { kind: hir::ItemKind::Trait { bounds, .. }, .. })) => {
            // `Self: Trait`, then the supertraits.
            out.push((Clause::Trait(TraitRef { def_id: def, args: tcx.identity_args(def) }), span));
            icx.lower_bounds(tcx.types.self_param, bounds, &mut out);
        }
        Some(Node::AssocItem(hir::AssocItem { kind: hir::AssocItemKind::Type(bounds, _), .. }))
            if tcx.def_kind(tcx.parent(def)) == DefKind::Trait =>
        {
            let alias = tcx.mk_ty(TyKind::Alias(AliasTy { def_id: def, args: tcx.identity_args(tcx.parent(def)) }));
            icx.lower_bounds(alias, bounds, &mut out);
        }
        _ => {}
    }
    if let Some(g) = hir_generics(tcx, def) {
        for pred in &g.predicates {
            match pred {
                hir::WherePredicate::Bound { bounded_ty, bounds, .. } => {
                    let ty = icx.lower_ty(bounded_ty);
                    icx.lower_bounds(ty, bounds, &mut out);
                }
                hir::WherePredicate::Lifetime { span, lifetime, bounds } => {
                    let r = icx.lower_lifetime(lifetime);
                    for b in bounds {
                        let b = icx.lower_lifetime(b);
                        out.push((Clause::RegionOutlives(r, b), *span));
                    }
                }
            }
        }
    }
    GenericPredicates { parent: generics.parent, predicates: tcx.alloc_clauses(out) }
}

// ---------------------------------------------------------------------------
// adt_def, impl_trait_ref, associated_items
// ---------------------------------------------------------------------------

fn adt_def(tcx: TyCtxt<'_>, def: DefId) -> AdtDefData {
    let Some(Node::Item(item)) = node(tcx, def) else { panic!("adt_def({def:?}): not an item") };
    let variant = |def_id: DefId, name, data: &hir::VariantData, discr: Option<DefId>| VariantDef {
        def_id,
        ctor: match data {
            hir::VariantData::Tuple(_, _, c) => Some((CtorKind::Fn, c.to_def_id())),
            hir::VariantData::Unit(_, c) => Some((CtorKind::Const, c.to_def_id())),
            hir::VariantData::Struct { .. } => None,
        },
        name,
        discr,
        fields: data.fields().iter().map(|f| FieldDef { did: f.def_id.to_def_id(), name: f.ident.name }).collect(),
    };
    let (kind, is_class, variants) = match &item.kind {
        hir::ItemKind::Struct(data, _) => (AdtKind::Struct, false, vec![variant(def, item.ident.name, data, None)]),
        hir::ItemKind::Class(data, _) => (AdtKind::Struct, true, vec![variant(def, item.ident.name, data, None)]),
        hir::ItemKind::Union(data, _) => (AdtKind::Union, false, vec![variant(def, item.ident.name, data, None)]),
        hir::ItemKind::Enum(e, _) => (
            AdtKind::Enum,
            false,
            e.variants
                .iter()
                .map(|v| variant(v.def_id.to_def_id(), v.ident.name, &v.data, v.disr_expr.as_ref().map(|d| d.def_id.to_def_id())))
                .collect(),
        ),
        _ => panic!("adt_def({def:?}): a {} isn't an ADT", item.kind.descr()),
    };
    AdtDefData { did: def, kind, is_class, variants }
}

fn impl_trait_ref<'tcx>(tcx: TyCtxt<'tcx>, def: DefId) -> Option<TraitRef<'tcx>> {
    let Some(Node::Item(hir::Item { kind: hir::ItemKind::Impl(imp), .. })) = node(tcx, def) else { return None };
    let t = imp.of_trait.as_ref()?;
    let self_ty = tcx.type_of(def);
    ItemCtxt::new(tcx, def, ElidedRegions::Error).lower_trait_ref(t, self_ty).map(|(tr, _)| tr)
}

fn associated_items(tcx: TyCtxt<'_>, def: DefId) -> Vec<AssocItem> {
    let items: &[vsc_span::def_id::LocalDefId] = match node(tcx, def) {
        Some(Node::Item(hir::Item { kind: hir::ItemKind::Trait { items, .. }, .. })) => items,
        Some(Node::Item(hir::Item { kind: hir::ItemKind::Impl(imp), .. })) => &imp.items,
        _ => &[],
    };
    items
        .iter()
        .filter_map(|&item| {
            let Some(Node::AssocItem(a)) = tcx.hir_node(item) else { return None };
            let kind = match &a.kind {
                hir::AssocItemKind::Const(..) => AssocKind::Const,
                hir::AssocItemKind::Fn(sig, _) => AssocKind::Fn { has_self: sig.decl.implicit_self != hir::ImplicitSelfKind::None },
                hir::AssocItemKind::Type(..) => AssocKind::Type,
            };
            Some(AssocItem { def_id: item.to_def_id(), name: a.ident.name, kind, container: def })
        })
        .collect()
}
