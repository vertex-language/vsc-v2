//! An item's generic parameters (rustc's `ty/generics.rs`).

use vsc_data_structures::FxHashMap;
use vsc_span::Symbol;
use vsc_span::def_id::DefId;

use super::TyCtxt;

/// The parameters an item declares, after its parent's. Indices count
/// from the outermost parent: in `implement<T> S<T> { func f<U>() }`, `T`
/// is 0 and `U` is 1. A trait's `Self` is its parameter 0.
#[derive(Debug)]
pub struct Generics {
    pub parent: Option<DefId>,
    pub parent_count: usize,
    pub own_params: Vec<GenericParamDef>,
    pub param_def_id_to_index: FxHashMap<DefId, u32>,
    pub has_self: bool,
}

#[derive(Clone, Debug)]
pub struct GenericParamDef {
    pub name: Symbol,
    pub def_id: DefId,
    pub index: u32,
    pub kind: GenericParamDefKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenericParamDefKind {
    /// `src: Lifetime` (exception 13).
    Lifetime,
    Type {
        has_default: bool,
        /// A trait's `Self`, or a flat receiver's introduced parameter.
        synthetic: bool,
    },
    Const {
        has_default: bool,
    },
}

impl Generics {
    pub fn count(&self) -> usize {
        self.parent_count + self.own_params.len()
    }

    /// The parameter at `index`, which may be a parent's.
    pub fn param_at<'tcx>(&'tcx self, index: usize, tcx: TyCtxt<'tcx>) -> &'tcx GenericParamDef {
        if index < self.parent_count {
            tcx.generics_of(self.parent.expect("parent generics")).param_at(index, tcx)
        } else {
            &self.own_params[index - self.parent_count]
        }
    }

    /// How many of each kind the item itself declares.
    pub fn own_counts(&self) -> (usize, usize, usize) {
        let mut c = (0, 0, 0);
        for p in &self.own_params {
            match p.kind {
                GenericParamDefKind::Lifetime => c.0 += 1,
                GenericParamDefKind::Type { synthetic: false, .. } => c.1 += 1,
                GenericParamDefKind::Type { synthetic: true, .. } => {}
                GenericParamDefKind::Const { .. } => c.2 += 1,
            }
        }
        c
    }
}
