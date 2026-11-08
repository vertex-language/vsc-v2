//! The Vertex abstract syntax tree (rustc's `rustc_ast`).
//!
//! The tree follows Rust's, with the changes the grammar needs: source files
//! with a `package` declaration and imports, `class` items, flat receivers,
//! origins in place of lifetimes, function literals, and built-ins such as
//! `vec[…]` and `asm(…)` instead of macro calls.

pub mod ast;
pub mod mut_visit;
pub mod node_id;
pub mod token;
pub mod tokenstream;
pub mod util;
pub mod visit;

pub use ast::*;
pub use node_id::{DUMMY_NODE_ID, NodeId};
