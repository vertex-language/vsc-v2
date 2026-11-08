//! HIR node ids (rustc's `rustc_hir::hir_id`): an owner (an item) and a
//! number within it, so editing one item doesn't renumber the others.

use std::fmt;

use vsc_span::def_id::LocalDefId;

vsc_index::newtype_index! {
    /// A node's number within its owner. The owner itself is 0.
    pub struct ItemLocalId {}
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HirId {
    pub owner: LocalDefId,
    pub local_id: ItemLocalId,
}

impl HirId {
    pub fn make_owner(owner: LocalDefId) -> HirId {
        HirId { owner, local_id: ItemLocalId::ZERO }
    }
}

impl fmt::Debug for HirId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HirId({}.{})", self.owner.local_def_index.as_u32(), self.local_id.as_u32())
    }
}
