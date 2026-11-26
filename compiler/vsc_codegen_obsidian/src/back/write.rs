//! Serializes `.obs` IR and compiles [`OwnedObsModule`] into native object files (`vsc_codegen_obsidian::back::write`).

use std::fs;
use std::path::Path;

use vsc_codegen_ssa::{CompiledModule, ModuleCodegen};
use vsc_session::Session;
use vsc_session::config::{CrateType, OptLevel, OutFileName, OutputFilenames, OutputType};

use crate::context::OwnedObsModule;

fn map_opt_level(level: OptLevel) -> obsidian::opt::OptLevel {
    match level {
        OptLevel::No => obsidian::opt::OptLevel::None,
        OptLevel::Less => obsidian::opt::OptLevel::Basic,
        OptLevel::Default | OptLevel::Size | OptLevel::SizeMin => obsidian::opt::OptLevel::Default,
        OptLevel::Aggressive => obsidian::opt::OptLevel::Aggressive,
    }
}

/// Emits `.obs` IR/assembly/bitcode/object files for a single [`ModuleCodegen<OwnedObsModule>`].
pub fn codegen_and_write_module(
    sess: &Session,
    outputs: &OutputFilenames,
    mut module: ModuleCodegen<OwnedObsModule>,
) -> CompiledModule {
    let needs_link = sess.opts.effective_crate_types().iter().any(|ct| {
        matches!(
            ct,
            CrateType::Executable
                | CrateType::Staticlib
                | CrateType::Dylib
                | CrateType::Cdylib
                | CrateType::Rlib
        )
    });

    let emit_ir = outputs.outputs.contains_key(OutputType::ObsIr);
    let emit_bc = outputs.outputs.contains_key(OutputType::Bitcode);
    let emit_asm = outputs.outputs.contains_key(OutputType::Assembly);
    let emit_obj = needs_link || outputs.outputs.contains_key(OutputType::Object);

    let obs_opt_level = map_opt_level(sess.opts.cg.opt_level);
    if obs_opt_level != obsidian::opt::OptLevel::None {
        let crate_types = sess.opts.effective_crate_types();
        let is_exe_only = crate_types.len() == 1 && crate_types[0] == CrateType::Executable;
        if is_exe_only || sess.opts.cg.lto {
            let lto_cfg = obsidian::opt::LtoConfig {
                opt_level: obs_opt_level,
                exported_roots: if is_exe_only {
                    vec!["main".to_string()]
                } else {
                    Vec::new()
                },
                internalize: is_exe_only,
            };
            obsidian::opt::optimize_lto(&mut module.module_backend.module, &lto_cfg);
        } else {
            obsidian::opt::optimize_module(&mut module.module_backend.module, obs_opt_level);
        }
    }

    let obs_mod = &module.module_backend.module;

    if emit_ir {
        let ir_path = outputs.temp_path(OutputType::ObsIr, Some(&module.name));
        write_obs_ir(sess, obs_mod, &ir_path);
        if let Some(Some(OutFileName::Real(explicit))) =
            outputs.outputs.0.get(&OutputType::ObsIr)
            && *explicit != ir_path
        {
            let _ = fs::copy(&ir_path, explicit);
        }
    }

    if emit_bc {
        let bc_path = outputs.temp_path(OutputType::Bitcode, Some(&module.name));
        write_obs_ir(sess, obs_mod, &bc_path);
        if let Some(Some(OutFileName::Real(explicit))) =
            outputs.outputs.0.get(&OutputType::Bitcode)
            && *explicit != bc_path
        {
            let _ = fs::copy(&bc_path, explicit);
        }
    }

    if emit_asm {
        let asm_path = outputs.temp_path(OutputType::Assembly, Some(&module.name));
        write_obs_ir(sess, obs_mod, &asm_path);
        if let Some(Some(OutFileName::Real(explicit))) =
            outputs.outputs.0.get(&OutputType::Assembly)
            && *explicit != asm_path
        {
            let _ = fs::copy(&asm_path, explicit);
        }
    }

    if emit_obj {
        let obj_path = outputs.temp_path(OutputType::Object, Some(&module.name));
        write_native_object(sess, obs_mod, &obj_path);
        if let Some(Some(OutFileName::Real(explicit))) =
            outputs.outputs.0.get(&OutputType::Object)
            && *explicit != obj_path
        {
            let _ = fs::copy(&obj_path, explicit);
        }
    }

    module.into_compiled_module(emit_obj, emit_bc, emit_asm, emit_ir, outputs)
}

fn write_obs_ir(sess: &Session, module: &obsidian::ir::Module, path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let text = format!("{module}");
    if let Err(err) = fs::write(path, text) {
        sess.dcx().fatal(format!(
            "failed to write Obsidian IR `{}`: {err}",
            path.display()
        ));
    }
}

fn write_native_object(sess: &Session, module: &obsidian::ir::Module, path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let bytes = obsidian::compile_module_to_object(module);
    if let Err(err) = fs::write(path, bytes) {
        sess.dcx().fatal(format!(
            "failed to write native object file `{}`: {err}",
            path.display()
        ));
    }
}
