//! Data structures shared by the compiler (rustc's `rustc_data_structures`).
//!
//! rustc gets several of these from crates.io (`rustc-hash`, `indexmap`,
//! `smallvec`). vsc writes its own on top of std.

pub mod fx;
pub mod fx_index_map;

pub use fx::{FxBuildHasher, FxHashMap, FxHashSet, FxHasher};
pub use fx_index_map::FxIndexMap;
