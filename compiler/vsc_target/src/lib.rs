//! Target specifications and target-specific C calling convention lowering (`rustc_target`).

pub mod callconv;
pub mod spec;
#[cfg(test)]
mod tests;
