//! The lang items found in the crate (rustc's `middle/lang_items.rs`).
//!
//! The library marks them with `@lang(name)`. Until vsc-v2 can build
//! `library/`, a crate can define the ones it needs itself, as rustc's
//! `#![no_core]` tests do.

use vsc_data_structures::FxHashMap;
use vsc_hir::LangItem;
use vsc_span::def_id::DefId;

#[derive(Default, Debug)]
pub struct LanguageItems {
    pub items: FxHashMap<LangItem, DefId>,
}

impl LanguageItems {
    pub fn get(&self, item: LangItem) -> Option<DefId> {
        self.items.get(&item).copied()
    }

    /// Which lang item a definition is, if any.
    pub fn from_def_id(&self, def: DefId) -> Option<LangItem> {
        self.items.iter().find(|&(_, &d)| d == def).map(|(&l, _)| l)
    }
}
