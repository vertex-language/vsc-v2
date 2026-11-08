//! The high-level IR (rustc's `rustc_hir`).
//!
//! HIR is the AST after lowering: names resolved, Vertex's spellings gone,
//! and surface forms desugared (`for`, `while`, `?`, ranges, built-ins) into
//! core constructs and lang items. From here on, the compiler follows rustc
//! with no Vertex cases except the few exceptions that reach type checking
//! (2, 3, 10, 13 and 14).

pub mod def;
pub mod hir;
pub mod hir_id;
pub mod intravisit;
pub mod lang_items;

pub use hir::*;
pub use hir_id::{HirId, ItemLocalId};
pub use lang_items::LangItem;
