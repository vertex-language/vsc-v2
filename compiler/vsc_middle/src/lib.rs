//! The middle of the compiler (rustc's `rustc_middle`): `TyCtxt`, the
//! semantic types in `ty`, and the queries passes ask of each other.
//!
//! Queries are methods on `TyCtxt` backed by a cache and a provider
//! function, installed by the crate that implements them
//! (`vsc_hir_analysis` provides `type_of`, `generics_of` and the rest), as
//! in rustc. There is no incremental cache yet.

pub mod arena;
pub mod hir;
pub mod middle;
pub mod mir;
pub mod query;
pub mod thir;
pub mod ty;
