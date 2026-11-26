//! Compiler options and the state shared by a compilation (rustc's `rustc_session`).

pub mod config;
pub mod parse;
pub mod session;
#[cfg(test)]
mod tests;

pub use parse::ParseSess;
pub use session::Session;
