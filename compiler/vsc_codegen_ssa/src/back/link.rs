//! Native binary and library linking (`rustc_codegen_ssa::back::link`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use vsc_session::Session;
use vsc_session::config::{
    CrateType, NativeLibKind, OutputFilenames, OutputType, SearchPathKind, Strip,
};
use vsc_target::spec::apple::{OsVersion, deployment_target_env_var};
use vsc_target::spec::{Arch, Cc, LinkerFlavor};

use crate::back::archive::create_static_library;
use crate::{CompiledModules, CrateInfo};

pub fn remove_temp_file(path: &Path) {
    if let Err(err) = fs::remove_file(path)
        && err.kind() != io::ErrorKind::NotFound
    {
        let _ = err;
    }
}

/// Links the compiled CGU object files into the target executable(s) or libraries.
pub fn link_binary(
    sess: &Session,
    compiled_modules: CompiledModules,
    crate_info: CrateInfo,
    outputs: &OutputFilenames,
) {
    let keep_objects = outputs.outputs.contains_key(OutputType::Object)
        || sess.opts.cg.save_temps
        || sess.opts.unstable_opts.save_temps;

    let objects = compiled_modules.all_objects();

    if outputs.outputs.should_link() {
        for &crate_type in &crate_info.crate_types {
            let out_path = output_path_for_crate_type(sess, outputs, crate_type);
            if let Some(parent) = out_path.parent()
                && !parent.as_os_str().is_empty()
                && let Err(e) = fs::create_dir_all(parent)
            {
                sess.dcx().fatal(format!(
                    "failed to create output directory `{}`: {e}",
                    parent.display()
                ));
            }
            match crate_type {
                CrateType::Staticlib | CrateType::Rlib => {
                    create_static_library(sess, &objects, &out_path);
                }
                CrateType::Executable
                | CrateType::Dylib
                | CrateType::Cdylib
                | CrateType::ProcMacro => {
                    invoke_linker(sess, crate_type, &objects, &crate_info, &out_path);
                }
            }
        }
    }

    if !keep_objects {
        for obj in objects {
            remove_temp_file(&obj);
        }
    }
}

pub fn output_path_for_crate_type(
    sess: &Session,
    outputs: &OutputFilenames,
    crate_type: CrateType,
) -> PathBuf {
    match crate_type {
        CrateType::Executable => outputs.exe_path(&sess.target),
        CrateType::Staticlib => {
            if let Some(ref p) = outputs.single_output_file
                && let Some(path) = p.as_path()
            {
                return path.to_path_buf();
            }
            let name = format!(
                "{}{}{}{}",
                sess.target.staticlib_prefix,
                outputs.filestem,
                outputs.extra,
                sess.target.staticlib_suffix
            );
            outputs.out_directory.join(name)
        }
        CrateType::Rlib => {
            if let Some(ref p) = outputs.single_output_file
                && let Some(path) = p.as_path()
            {
                return path.to_path_buf();
            }
            let name = format!("lib{}{}.rlib", outputs.filestem, outputs.extra);
            outputs.out_directory.join(name)
        }
        CrateType::Dylib | CrateType::Cdylib | CrateType::ProcMacro => {
            if let Some(ref p) = outputs.single_output_file
                && let Some(path) = p.as_path()
            {
                return path.to_path_buf();
            }
            let name = format!(
                "{}{}{}{}",
                sess.target.dll_prefix,
                outputs.filestem,
                outputs.extra,
                sess.target.dll_suffix
            );
            outputs.out_directory.join(name)
        }
    }
}

fn resolve_linker(sess: &Session) -> PathBuf {
    if let Some(ref custom) = sess.opts.cg.linker {
        return custom.clone();
    }
    if let Some(ref target_linker) = sess.target.linker {
        return PathBuf::from(target_linker);
    }
    match sess.target.linker_flavor {
        LinkerFlavor::Darwin(Cc::Yes, _)
        | LinkerFlavor::Gnu(Cc::Yes, _)
        | LinkerFlavor::WasmLld(Cc::Yes) => PathBuf::from("cc"),
        LinkerFlavor::Darwin(Cc::No, _) | LinkerFlavor::Gnu(Cc::No, _) => PathBuf::from("ld"),
        LinkerFlavor::WasmLld(Cc::No) => PathBuf::from("wasm-ld"),
        LinkerFlavor::Msvc(_) => PathBuf::from("link.exe"),
    }
}

fn invoke_linker(
    sess: &Session,
    crate_type: CrateType,
    objects: &[PathBuf],
    crate_info: &CrateInfo,
    out_path: &Path,
) {
    let linker = resolve_linker(sess);
    let mut cmd = Command::new(&linker);
    let uses_cc = sess.target.linker_flavor.uses_cc();

    if sess.target.is_like_darwin && uses_cc {
        let arch_flag = match sess.target.arch {
            Arch::AArch64 => Some("arm64"),
            Arch::X86_64 => Some("x86_64"),
            _ => None,
        };
        if let Some(arch) = arch_flag {
            cmd.arg("-arch").arg(arch);
        }
        let env_var = deployment_target_env_var(&sess.target.os);
        if std::env::var_os(env_var).is_none() {
            let min = OsVersion::minimum_deployment_target(&sess.target);
            cmd.env(env_var, min.to_string());
        }
    }

    for (flavor, args) in &sess.target.pre_link_args {
        if *flavor == sess.target.linker_flavor {
            cmd.args(args);
        }
    }

    match crate_type {
        CrateType::Dylib | CrateType::Cdylib | CrateType::ProcMacro => {
            if sess.target.is_like_darwin {
                if uses_cc {
                    cmd.arg("-dynamiclib");
                } else {
                    cmd.arg("-dylib");
                }
            } else {
                cmd.arg("-shared");
            }
        }
        CrateType::Executable => {
            if !uses_cc
                && sess.target.position_independent_executables
                && !sess.target.is_like_darwin
            {
                cmd.arg("-pie");
            }
        }
        CrateType::Staticlib | CrateType::Rlib => unreachable!(),
    }

    for obj in objects {
        cmd.arg(obj);
    }

    cmd.arg("-o").arg(out_path);

    for sp in &sess.opts.search_paths {
        match sp.kind {
            SearchPathKind::Framework => {
                if sess.target.is_like_darwin {
                    cmd.arg(format!("-F{}", sp.dir.display()));
                }
            }
            SearchPathKind::Native | SearchPathKind::All | SearchPathKind::Crate | SearchPathKind::Dependency => {
                cmd.arg(format!("-L{}", sp.dir.display()));
            }
        }
    }

    for lib in &crate_info.used_libraries {
        match lib.kind {
            NativeLibKind::Framework => {
                if sess.target.is_like_darwin {
                    cmd.arg("-framework").arg(&lib.name);
                }
            }
            NativeLibKind::Dylib | NativeLibKind::Static { .. } | NativeLibKind::Unspecified => {
                cmd.arg(format!("-l{}", lib.name));
            }
        }
    }

    if sess.target.is_like_darwin && uses_cc {
        cmd.arg("-lSystem");
    }

    match sess.opts.cg.strip {
        Strip::None => {}
        Strip::Debuginfo => {
            if uses_cc && !sess.target.is_like_darwin {
                cmd.arg("-Wl,-S");
            } else if sess.target.is_like_darwin {
                cmd.arg("-Wl,-S");
            }
        }
        Strip::Symbols => {
            if sess.target.is_like_darwin {
                cmd.arg("-Wl,-x");
            } else {
                cmd.arg("-s");
            }
        }
    }

    for (flavor, args) in &sess.target.post_link_args {
        if *flavor == sess.target.linker_flavor {
            cmd.args(args);
        }
    }

    for arg in &sess.opts.cg.link_args {
        cmd.arg(arg);
    }

    for key in &sess.target.link_env_remove {
        cmd.env_remove(key);
    }
    for (k, v) in &sess.target.link_env {
        cmd.env(k, v);
    }

    match cmd.output() {
        Ok(res) if res.status.success() => {}
        Ok(res) => {
            let stdout = String::from_utf8_lossy(&res.stdout);
            let stderr = String::from_utf8_lossy(&res.stderr);
            sess.dcx().fatal(format!(
                "linking with `{}` failed: exit status: {}\nstdout: {}\nstderr: {}",
                linker.display(),
                res.status,
                stdout.trim(),
                stderr.trim()
            ));
        }
        Err(err) => {
            sess.dcx().fatal(format!(
                "failed to invoke linker `{}`: {err}",
                linker.display()
            ));
        }
    }
}
