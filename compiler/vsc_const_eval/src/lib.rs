//! Evaluating MIR (rustc's `rustc_const_eval`): the interpreter that runs
//! programs before code generation exists, as Miri does for Rust, and that
//! constant evaluation will use.

pub mod interpret;

#[cfg(test)]
mod tests;

pub use interpret::{Output, RunResult, run_main};
