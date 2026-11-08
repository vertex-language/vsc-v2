//! Compiler options and the state shared by a compilation (rustc's
//! `rustc_session`).

pub mod config;
pub mod parse;

pub use parse::ParseSess;
