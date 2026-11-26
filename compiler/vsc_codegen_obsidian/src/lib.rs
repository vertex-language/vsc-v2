//! Pure Obsidian native codegen backend (`vsc_codegen_obsidian`).
//!
//! Lowers monomorphized Vertex MIR (`CodegenUnit`s) directly into `obsidian::ir::Module`
//! (`.obs` Bedrock SSA IR) and emits native relocatable object files (ELF `.o`, Mach-O `.o`,
//! COFF `.obj`) across `{Linux, macOS, Windows} x {x86_64, arm64}` with zero external dependencies.

pub mod abi;
pub mod back;
pub mod base;
pub mod builder;
pub mod callee;
pub mod common;
pub mod consts;
pub mod context;
pub mod declare;
pub mod intrinsic;
pub mod mono_item;
pub mod type_;
pub mod type_of;
pub mod value;

#[cfg(test)]
mod tests;

use std::any::Any;

use vsc_codegen_ssa::traits::CodegenBackend;
use vsc_codegen_ssa::{CompiledModules, CrateInfo, ModuleCodegen};
use vsc_middle::ty::TyCtxt;
use vsc_session::Session;
use vsc_session::config::OutputFilenames;

use crate::context::OwnedObsModule;

/// Pure Obsidian codegen backend implementing [`CodegenBackend`].
#[derive(Default)]
pub struct ObsidianCodegenBackend;

impl ObsidianCodegenBackend {
    pub fn new() -> Self {
        Self
    }
}

/// Active per-crate codegen state returned from [`ObsidianCodegenBackend::codegen_crate`].
pub struct OngoingCodegen {
    pub modules: Vec<ModuleCodegen<OwnedObsModule>>,
}

impl CodegenBackend for ObsidianCodegenBackend {
    fn name(&self) -> &'static str {
        "obsidian"
    }

    fn codegen_crate<'tcx>(&self, tcx: TyCtxt<'tcx>) -> Box<dyn Any> {
        let cgus = tcx.collect_and_partition_mono_items(());
        let modules: Vec<ModuleCodegen<OwnedObsModule>> = cgus
            .iter()
            .map(|cgu| base::compile_codegen_unit(tcx, cgu))
            .collect();

        Box::new(OngoingCodegen { modules })
    }

    fn join_codegen(
        &self,
        ongoing_codegen: Box<dyn Any>,
        sess: &Session,
        outputs: &OutputFilenames,
        _crate_info: &CrateInfo,
    ) -> CompiledModules {
        let ongoing = ongoing_codegen
            .downcast::<OngoingCodegen>()
            .expect("expected OngoingCodegen from ObsidianCodegenBackend::codegen_crate");

        let mut compiled_modules = Vec::with_capacity(ongoing.modules.len());
        for module in ongoing.modules {
            let compiled = back::write::codegen_and_write_module(sess, outputs, module);
            compiled_modules.push(compiled);
        }

        CompiledModules {
            modules: compiled_modules,
            allocator_module: None,
        }
    }
}
