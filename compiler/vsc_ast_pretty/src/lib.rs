//! Printing the AST as Vertex source (rustc's `rustc_ast_pretty`).
//!
//! The output is canonical: one statement, field, variant or arm per line,
//! four-space indents, and no `;` at line ends. Parentheses come only from
//! `Paren` nodes, so printing a parsed file keeps its structure exactly.
//! `vsc -Z unpretty=ast` and the parser's round-trip tests use it; `vsfmt`
//! will build on it.

pub mod pprust;

pub use pprust::{
    attribute_to_string, expr_to_string, item_to_string, pat_to_string, path_to_string, source_file_to_string, ty_to_string,
};
