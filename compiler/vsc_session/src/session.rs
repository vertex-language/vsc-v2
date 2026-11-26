//! Compiler session state (`rustc_session::Session`).

use std::path::{Path, PathBuf};

use vsc_errors::DiagCtxt;
use vsc_span::SourceMap;
use vsc_target::spec::{PanicStrategy, Target};

use crate::ParseSess;
use crate::config::{CrateType, Options, OutFileName, OutputFilenames, TargetInfo};

/// Shared state for a single compiler invocation (`rustc_session::Session`).
pub struct Session {
    pub opts: Options,
    pub host: Target,
    pub target: Target,
    pub target_info: TargetInfo,
    pub psess: ParseSess,
    pub sysroot: PathBuf,
}

impl Session {
    pub fn new(opts: Options, psess: ParseSess) -> Result<Self, String> {
        let host_triple = Target::host_triple();
        let host = Target::from_triple(host_triple)?;
        let target_info = match &opts.target_triple {
            None => TargetInfo::host(),
            Some(t) => TargetInfo::from_triple(t).ok_or_else(|| format!("unknown target `{t}`"))?,
        };
        let target = target_info.to_target()?;
        let sysroot = resolve_sysroot(&opts);
        Ok(Self { opts, host, target, target_info, psess, sysroot })
    }

    #[inline]
    pub fn dcx(&self) -> &DiagCtxt {
        &self.psess.dcx
    }

    #[inline]
    pub fn source_map(&self) -> &SourceMap {
        &self.psess.source_map
    }

    #[inline]
    pub fn crate_types(&self) -> Vec<CrateType> {
        self.opts.effective_crate_types()
    }

    #[inline]
    pub fn panic_strategy(&self) -> PanicStrategy {
        self.opts.effective_panic_strategy(&self.target)
    }

    #[inline]
    pub fn sysroot(&self) -> &Path {
        &self.sysroot
    }

    #[inline]
    pub fn codegen_units(&self) -> usize {
        self.opts.cg.codegen_units.unwrap_or(1).max(1)
    }

    pub fn output_filenames(&self, crate_name: &str, temps_dir: Option<PathBuf>) -> OutputFilenames {
        let out_directory = self.opts.out_dir.clone().unwrap_or_else(|| {
            self.opts
                .out_file
                .as_ref()
                .and_then(OutFileName::as_path)
                .and_then(|p| p.parent())
                .filter(|p| !p.as_os_str().is_empty())
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        });
        OutputFilenames {
            out_directory,
            filestem: crate_name.to_string(),
            single_output_file: self.opts.out_file.clone(),
            temps_directory: temps_dir,
            outputs: self.opts.output_types.clone(),
            extra: self.opts.cg.extra_filename.clone(),
        }
    }
}

pub fn resolve_sysroot(opts: &Options) -> PathBuf {
    if let Some(ref sysroot) = opts.maybe_sysroot {
        return sysroot.clone();
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin_dir) = exe.parent()
        && let Some(prefix) = bin_dir.parent()
    {
        return prefix.to_path_buf();
    }
    PathBuf::from(".")
}
