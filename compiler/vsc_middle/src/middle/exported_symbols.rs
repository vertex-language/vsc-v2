//! Symbols exported from a crate (`rustc_middle::middle::exported_symbols`).

use vsc_span::def_id::DefId;

use crate::ty::{self, GenericArgsRef, Instance, SymbolName, Ty, TyCtxt};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SymbolExportLevel {
    C,
    Rust,
}

impl SymbolExportLevel {
    pub fn is_below_threshold(self, threshold: SymbolExportLevel) -> bool {
        threshold == SymbolExportLevel::Rust || self == SymbolExportLevel::C
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SymbolExportKind {
    Text,
    Data,
    Tls,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SymbolExportInfo {
    pub level: SymbolExportLevel,
    pub kind: SymbolExportKind,
    pub used: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ExportedSymbol<'tcx> {
    NonGeneric(DefId),
    Generic(DefId, GenericArgsRef<'tcx>),
    DropGlue(Ty<'tcx>),
    NoDefId(SymbolName<'tcx>),
}

impl<'tcx> ExportedSymbol<'tcx> {
    pub fn symbol_name_for_local_instance(&self, tcx: TyCtxt<'tcx>) -> ty::SymbolName<'tcx> {
        match *self {
            ExportedSymbol::NonGeneric(def_id) => tcx.symbol_name(Instance::mono(tcx, def_id)),
            ExportedSymbol::Generic(def_id, args) => {
                tcx.symbol_name(Instance::new_raw(def_id, args))
            }
            ExportedSymbol::DropGlue(ty) => {
                tcx.symbol_name(Instance::resolve_drop_in_place(tcx, ty))
            }
            ExportedSymbol::NoDefId(name) => name,
        }
    }
}
