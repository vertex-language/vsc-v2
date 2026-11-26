//! Checks and collections over HIR that aren't type checking (rustc's
//! `rustc_passes`).

pub mod entry;
pub mod lang_items;
pub mod loops;

use vsc_middle::query::Providers;

pub fn provide(providers: &mut Providers) {
    providers.lang_items = lang_items::collect;
    providers.entry_fn = entry::entry_fn;
}
