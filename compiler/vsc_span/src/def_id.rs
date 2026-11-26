//! Definition ids (rustc's `rustc_span::def_id`).

use std::fmt;

vsc_index::newtype_index! {
    /// A crate: the local one, or a dependency.
    pub struct CrateNum {}
}

/// The crate being compiled.
pub const LOCAL_CRATE: CrateNum = CrateNum::ZERO;

vsc_index::newtype_index! {
    /// A definition's index within its crate.
    pub struct DefIndex {}
}

/// The crate root is always definition 0.
pub const CRATE_DEF_INDEX: DefIndex = DefIndex::ZERO;

/// A definition anywhere: a crate and an index in it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId {
    pub krate: CrateNum,
    pub index: DefIndex,
}

impl DefId {
    pub fn is_local(self) -> bool {
        self.krate == LOCAL_CRATE
    }

    pub fn as_local(self) -> Option<LocalDefId> {
        self.is_local().then_some(LocalDefId { local_def_index: self.index })
    }

    #[track_caller]
    pub fn expect_local(self) -> LocalDefId {
        self.as_local().unwrap_or_else(|| panic!("DefId::expect_local: {self:?} isn't local"))
    }
}

impl fmt::Debug for DefId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DefId({}:{})", self.krate.as_u32(), self.index.as_u32())
    }
}

/// A definition in the local crate.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalDefId {
    pub local_def_index: DefIndex,
}

/// The local crate's root.
pub const CRATE_DEF_ID: LocalDefId = LocalDefId { local_def_index: CRATE_DEF_INDEX };

impl LocalDefId {
    #[inline]
    pub fn to_def_id(self) -> DefId {
        DefId { krate: LOCAL_CRATE, index: self.local_def_index }
    }
}

impl vsc_index::Idx for LocalDefId {
    fn new(idx: usize) -> Self {
        LocalDefId { local_def_index: DefIndex::from_usize(idx) }
    }
    fn index(self) -> usize {
        self.local_def_index.as_usize()
    }
}

impl fmt::Debug for LocalDefId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LocalDefId({})", self.local_def_index.as_u32())
    }
}
