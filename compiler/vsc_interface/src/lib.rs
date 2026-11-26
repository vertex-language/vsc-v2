//! The compiler pipeline and session interface (`rustc_interface`).
//!
//! Connects front-end parsing, expansion, name resolution, HIR lowering,
//! type checking, MIR building/borrow checking, and the SSA/Obsidian codegen
//! backend behind [`Config`], [`Compiler`], [`run_compiler`], and [`Linker`].

pub mod interface;
pub mod passes;
pub mod queries;

pub use interface::{Compiler, Config, Input, run_compiler};
pub use passes::{
    AnalysisOutcome, DEFAULT_QUERY_PROVIDERS, create_and_enter_global_ctxt, default_providers,
    handle_session_prints, parse, resolve_and_lower,
};
pub use queries::Linker;
