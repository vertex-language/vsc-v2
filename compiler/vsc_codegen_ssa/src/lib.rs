//! Backend-agnostic code generation infrastructure (`rustc_codegen_ssa`).
//!
//! Defines [`CodegenBackend`], compiled module metadata, MIR operand/place
//! abstractions, and host archive/linker invocation for executables and native
//! libraries.

pub mod back;
pub mod common;
pub mod mir;
pub mod traits;

use std::path::PathBuf;

pub use crate::back::write::{CompiledModule, CompiledModules, ModuleKind};
use vsc_middle::ty::{EntryFnType, TyCtxt};
use vsc_session::config::{CrateType, NativeLib, OutputFilenames, OutputType};
use vsc_span::Symbol;
use vsc_span::def_id::DefId;

/// Backend capabilities queried during configuration/compilation.
#[derive(Clone, Copy, Debug)]
pub struct TargetConfig {
    pub has_reliable_f16: bool,
    pub has_reliable_f16b: bool,
    pub has_reliable_f128: bool,
}

impl Default for TargetConfig {
    fn default() -> Self {
        Self {
            has_reliable_f16: true,
            has_reliable_f16b: true,
            has_reliable_f128: true,
        }
    }
}

/// Summary of crate metadata required after `TyCtxt` finishes codegen, for linking.
#[derive(Clone, Debug)]
pub struct CrateInfo {
    pub local_crate_name: Symbol,
    pub crate_types: Vec<CrateType>,
    pub entry_fn: Option<(DefId, EntryFnType)>,
    pub used_libraries: Vec<NativeLib>,
}

impl CrateInfo {
    pub fn new(tcx: TyCtxt<'_>) -> Self {
        Self {
            local_crate_name: tcx.crate_name,
            crate_types: tcx.options.effective_crate_types(),
            entry_fn: tcx.entry_fn(()),
            used_libraries: tcx.options.libs.clone(),
        }
    }
}

/// A single generated backend module (`CodegenUnit` output) ready to optimize & emit.
pub struct ModuleCodegen<M> {
    pub name: String,
    pub module_backend: M,
    pub kind: ModuleKind,
}

impl<M> ModuleCodegen<M> {
    pub fn new_regular(name: impl Into<String>, module: M) -> Self {
        Self { name: name.into(), module_backend: module, kind: ModuleKind::Regular }
    }

    pub fn new_shim(name: impl Into<String>, module: M) -> Self {
        Self { name: name.into(), module_backend: module, kind: ModuleKind::Allocator }
    }

    pub fn into_compiled_module(
        self,
        emit_obj: bool,
        emit_bc: bool,
        emit_asm: bool,
        emit_ir: bool,
        outputs: &OutputFilenames,
    ) -> CompiledModule {
        let object = emit_obj.then(|| outputs.temp_path(OutputType::Object, Some(&self.name)));
        let bytecode = emit_bc.then(|| outputs.temp_path(OutputType::Bitcode, Some(&self.name)));
        let assembly = emit_asm.then(|| outputs.temp_path(OutputType::Assembly, Some(&self.name)));
        let obs_ir = emit_ir.then(|| outputs.temp_path(OutputType::ObsIr, Some(&self.name)));

        CompiledModule {
            name: self.name,
            kind: self.kind,
            object,
            bytecode,
            assembly,
            obs_ir,
        }
    }
}

/// Resolve the final path for a single-output artifact (`OutFileName::Real`) or `None`.
pub fn temp_dir_for_outputs(outputs: &OutputFilenames) -> PathBuf {
    outputs
        .temps_directory
        .clone()
        .unwrap_or_else(|| outputs.out_directory.clone())
}
