//! Monomorphized compilation items and codegen units (`rustc_middle::mono`).

use std::hash::Hash;

use vsc_data_structures::FxIndexMap;
use vsc_hir::ItemId;
use vsc_span::Symbol;
use vsc_span::def_id::DefId;

use crate::middle::codegen_fn_attrs::Linkage;
use crate::ty::{Instance, InstanceKind, SymbolName, TyCtxt};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstantiationMode {
    GloballyShared { may_conflict: bool },
    LocalCopy,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MonoItem<'tcx> {
    Fn(Instance<'tcx>),
    Static(DefId),
    GlobalAsm(ItemId),
}

impl<'tcx> MonoItem<'tcx> {
    pub fn is_user_defined(&self) -> bool {
        match *self {
            MonoItem::Fn(instance) => instance.is_user_defined(),
            MonoItem::Static(..) | MonoItem::GlobalAsm(..) => true,
        }
    }

    pub fn symbol_name(&self, tcx: TyCtxt<'tcx>) -> SymbolName<'tcx> {
        match *self {
            MonoItem::Fn(instance) => tcx.symbol_name(instance),
            MonoItem::Static(def_id) => tcx.symbol_name(Instance::mono(tcx, def_id)),
            MonoItem::GlobalAsm(item_id) => SymbolName::new(
                tcx,
                &format!("global_asm_{}", item_id.owner_id.local_def_index.as_u32()),
            ),
        }
    }

    pub fn explicit_linkage(&self, tcx: TyCtxt<'tcx>) -> Option<Linkage> {
        match *self {
            MonoItem::Fn(instance) => tcx.codegen_instance_attrs(instance.def).linkage,
            MonoItem::Static(def_id) => tcx.codegen_fn_attrs(def_id).linkage,
            MonoItem::GlobalAsm(..) => None,
        }
    }

    pub fn instantiation_mode(&self, tcx: TyCtxt<'tcx>) -> InstantiationMode {
        match *self {
            MonoItem::Fn(instance) => match instance.def {
                InstanceKind::DropGlue(..)
                | InstanceKind::ClosureOnceShim { .. }
                | InstanceKind::FnPtrShim(..)
                | InstanceKind::ReifyShim(..)
                | InstanceKind::VTableShim(..) => {
                    let is_generic = !instance.args.is_empty();
                    InstantiationMode::GloballyShared { may_conflict: is_generic }
                }
                InstanceKind::Item(_) => {
                    let is_generic = instance.def.requires_monomorphization(tcx);
                    InstantiationMode::GloballyShared { may_conflict: is_generic }
                }
                InstanceKind::Intrinsic(_) | InstanceKind::Virtual(..) => {
                    InstantiationMode::LocalCopy
                }
            },
            MonoItem::Static(..) | MonoItem::GlobalAsm(..) => {
                InstantiationMode::GloballyShared { may_conflict: false }
            }
        }
    }

    pub fn is_generic_mono_item(&self, tcx: TyCtxt<'tcx>) -> bool {
        match *self {
            MonoItem::Fn(instance) => instance.def.requires_monomorphization(tcx),
            MonoItem::Static(..) | MonoItem::GlobalAsm(..) => false,
        }
    }

    pub fn size_estimate(&self, tcx: TyCtxt<'tcx>) -> usize {
        match *self {
            MonoItem::Fn(instance) => {
                let def_id = instance.def_id();
                if def_id.is_local() && tcx.body_owned_by(def_id.expect_local()).is_some() {
                    let mir = tcx.optimized_mir(def_id);
                    mir.basic_blocks.len().saturating_mul(4).max(1)
                } else {
                    4
                }
            }
            MonoItem::Static(..) | MonoItem::GlobalAsm(..) => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Visibility {
    Default,
    Hidden,
    Protected,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MonoItemData {
    pub inlined: bool,
    pub linkage: Linkage,
    pub visibility: Visibility,
    pub size_estimate: usize,
}

#[derive(Debug)]
pub struct CodegenUnit<'tcx> {
    name: Symbol,
    items: FxIndexMap<MonoItem<'tcx>, MonoItemData>,
    size_estimate: usize,
    primary: bool,
}

impl<'tcx> CodegenUnit<'tcx> {
    #[inline]
    pub fn new(name: Symbol) -> Self {
        CodegenUnit {
            name,
            items: FxIndexMap::default(),
            size_estimate: 0,
            primary: false,
        }
    }

    #[inline]
    pub fn name(&self) -> Symbol {
        self.name
    }

    #[inline]
    pub fn items(&self) -> &FxIndexMap<MonoItem<'tcx>, MonoItemData> {
        &self.items
    }

    #[inline]
    pub fn items_mut(&mut self) -> &mut FxIndexMap<MonoItem<'tcx>, MonoItemData> {
        &mut self.items
    }

    #[inline]
    pub fn modify_size_estimate(&mut self, delta: usize) {
        self.size_estimate = self.size_estimate.saturating_add(delta);
    }

    #[inline]
    pub fn size_estimate(&self) -> usize {
        self.size_estimate
    }

    #[inline]
    pub fn is_primary(&self) -> bool {
        self.primary
    }

    #[inline]
    pub fn set_is_primary(&mut self, primary: bool) {
        self.primary = primary;
    }

    pub fn items_in_deterministic_order(
        &self,
        tcx: TyCtxt<'tcx>,
    ) -> Vec<(MonoItem<'tcx>, MonoItemData)> {
        let mut items: Vec<_> = self.items.iter().map(|(&k, &v)| (k, v)).collect();
        items.sort_by(|(a, _), (b, _)| a.symbol_name(tcx).name.cmp(b.symbol_name(tcx).name));
        items
    }
}

#[derive(Debug)]
pub struct MonoItemPartitions<'tcx> {
    pub codegen_units: &'tcx [CodegenUnit<'tcx>],
}
