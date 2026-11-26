//! Compiled module metadata and output emission routing (`rustc_codegen_ssa::back::write`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use vsc_session::Session;
use vsc_session::config::{OutFileName, OutputFilenames, OutputType};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleKind {
    Regular,
    Metadata,
    Allocator,
}

#[derive(Clone, Debug)]
pub struct CompiledModule {
    pub name: String,
    pub kind: ModuleKind,
    pub object: Option<PathBuf>,
    pub bytecode: Option<PathBuf>,
    pub assembly: Option<PathBuf>,
    pub obs_ir: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct CompiledModules {
    pub modules: Vec<CompiledModule>,
    pub allocator_module: Option<CompiledModule>,
}

impl CompiledModules {
    pub fn all_objects(&self) -> Vec<PathBuf> {
        let mut objs = Vec::new();
        for m in &self.modules {
            if let Some(ref p) = m.object {
                objs.push(p.clone());
            }
        }
        if let Some(ref alloc) = self.allocator_module
            && let Some(ref p) = alloc.object
        {
            objs.push(p.clone());
        }
        objs
    }
}

/// Copies a generated single-module artifact (`--emit=obj`, `asm`, `obs-ir`, `bc`)
/// from its temporary CGU path into the final user-requested output path when appropriate.
pub fn finalize_single_cgu_outputs(
    sess: &Session,
    outputs: &OutputFilenames,
    modules: &mut [CompiledModule],
) {
    let regular_count = modules.iter().filter(|m| m.kind == ModuleKind::Regular).count();
    if regular_count != 1 {
        return;
    }

    let Some(m) = modules.iter_mut().find(|m| m.kind == ModuleKind::Regular) else {
        return;
    };

    let pairs: [(bool, OutputType, &mut Option<PathBuf>); 4] = [
        (
            outputs.outputs.contains_key(OutputType::Object),
            OutputType::Object,
            &mut m.object,
        ),
        (
            outputs.outputs.contains_key(OutputType::Bitcode),
            OutputType::Bitcode,
            &mut m.bytecode,
        ),
        (
            outputs.outputs.contains_key(OutputType::Assembly),
            OutputType::Assembly,
            &mut m.assembly,
        ),
        (
            outputs.outputs.contains_key(OutputType::ObsIr),
            OutputType::ObsIr,
            &mut m.obs_ir,
        ),
    ];

    for (requested, flavor, slot) in pairs {
        if !requested {
            continue;
        }
        let Some(src) = slot.as_ref() else { continue };
        match outputs.path(flavor) {
            OutFileName::Real(dest) => {
                if *src != dest {
                    if let Some(parent) = dest.parent()
                        && !parent.as_os_str().is_empty()
                    {
                        let _ = fs::create_dir_all(parent);
                    }
                    if let Err(err) = move_or_copy(src, &dest) {
                        sess.dcx().fatal(format!(
                            "failed to write output `{}`: {err}",
                            dest.display()
                        ));
                    }
                    *slot = Some(dest);
                }
            }
            OutFileName::Stdout => {
                if let Ok(bytes) = fs::read(src) {
                    let _ = io::Write::write_all(&mut io::stdout(), &bytes);
                    let _ = fs::remove_file(src);
                }
                *slot = None;
            }
        }
    }
}

fn move_or_copy(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            fs::copy(src, dst)?;
            let _ = fs::remove_file(src);
            Ok(())
        }
    }
}
