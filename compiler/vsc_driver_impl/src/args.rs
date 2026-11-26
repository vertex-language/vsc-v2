//! rustc-compatible command-line parsing (`rustc_driver_impl`).

use std::path::PathBuf;

use vsc_errors::ColorConfig;
use vsc_session::config::{
    CodeModel, CrateType, DebugInfo, ErrorFormat as SessErrorFormat, ExternEntry, NativeLib,
    NativeLibKind, OptLevel, Options as SessOptions, OutFileName, OutputType, OutputTypes,
    PanicStrategy, PrintRequest, RelocModel, SearchPath, SearchPathKind, Strip, TargetInfo,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorFormat {
    Human,
    Short,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unpretty {
    /// `-Z unpretty=ast`: the parsed file printed as Vertex.
    Ast,
    /// `-Z unpretty=ast-tree`: the AST's debug form.
    AstTree,
    /// `-Z unpretty=hir`: the crate after lowering, printed in Rust's syntax.
    Hir,
    /// `-Z unpretty=hir-tree`: the HIR's debug form.
    HirTree,
    /// `-Z unpretty=thir-tree`: every body's THIR, after type checking.
    ThirTree,
}

impl Unpretty {
    /// Printed straight after parsing, before name resolution.
    pub fn is_ast(self) -> bool {
        matches!(self, Unpretty::Ast | Unpretty::AstTree)
    }
}

#[derive(Debug)]
pub struct Options {
    pub input: PathBuf,
    pub crate_name: Option<String>,
    pub error_format: ErrorFormat,
    pub color: ColorConfig,
    pub parse_only: bool,
    pub unpretty: Option<Unpretty>,
    /// `-Z dump-resolutions`
    pub dump_resolutions: bool,
    /// `-Z dump-signatures`
    pub dump_signatures: bool,
    /// `-Z dump-types`
    pub dump_types: bool,
    /// `-Z dump-mir`
    pub dump_mir: bool,
    /// `-Z validate-mir`
    pub validate_mir: bool,
    /// `-Z run-mir`
    pub run_mir: bool,
    /// `--emit=metadata`: check only, as `cargo check` asks.
    pub check_only: bool,
    /// `--cfg` arguments.
    pub cfgs: Vec<String>,
    /// `--test`
    pub test: bool,
    pub target: Option<String>,
    /// Structured session options mirroring `rustc_session::config::Options`.
    pub sess: SessOptions,
}

pub enum Parsed {
    Run(Options),
    /// Print this and exit successfully (`--help`, `--version`, `--print`).
    Done(String),
}

const USAGE: &str = "\
Usage: vsc [OPTIONS] INPUT

INPUT is the crate root: its folder's .vs files form the root package.

Options:
    -h, --help              Print this help
    -V, --version           Print the version
    -vV                     Print verbose version information
        --crate-name NAME   Name of the crate
        --crate-type TYPE   bin, lib, rlib, staticlib, dylib, cdylib
        --edition 2024      Vertex has one edition; 2024 is accepted
        --error-format FMT  human or short
        --color WHEN        auto, always or never
        --cfg SPEC          Turn on a configuration option: name or name=\"value\"
        --test              Build the test harness (sets cfg test)
        --target TRIPLE     Target triple (default: the host)
        --emit KINDS        Comma-separated list of outputs (link, obj, asm, obs-ir, bc, mir, metadata, dep-info)
    -o FILENAME             Write output to <filename>
        --out-dir DIR       Write output to compiler-chosen filename in <dir>
    -L [KIND=]PATH          Add a directory to the library search path
    -l [KIND=]NAME          Link the generated crate(s) to the specified native library NAME
        --extern NAME[=PATH] Specify where an external library is located
        --sysroot PATH      Override the system root
    -C OPT[=VALUE]          Set a codegen option (opt-level, debuginfo, linker, link-arg, codegen-units, panic, ...)
    -O                      Equivalent to -C opt-level=2
    -g                      Equivalent to -C debuginfo=2
        --print INFO        Compiler information to print on stdout
    -Z parse-only           Stop after parsing
    -Z dump-resolutions     Print what every path resolved to
    -Z dump-signatures      Print every item's type or signature
    -Z dump-types           Print the types inferred in every body
    -Z dump-mir             Print every body's MIR
    -Z validate-mir         Check every body's MIR is well formed
    -Z run-mir              Run `main` in the MIR interpreter
    -Z unpretty=ast         Print the parsed source as Vertex
    -Z unpretty=ast-tree    Print the parsed AST
    -Z unpretty=hir         Print the crate after lowering to HIR
    -Z unpretty=hir-tree    Print the HIR
    -Z unpretty=thir-tree   Print every body's THIR
";

pub fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut input = None;
    let mut unpretty = None;
    let mut error_format = ErrorFormat::Human;
    let mut sess = SessOptions::default();
    let mut explicit_emits: Option<OutputTypes> = None;

    let mut it = args.iter().peekable();
    let value = |flag: &str,
                 arg: &str,
                 it: &mut std::iter::Peekable<std::slice::Iter<'_, String>>|
     -> Result<String, String> {
        if let Some(v) = arg.strip_prefix(flag).and_then(|r| r.strip_prefix('=')) {
            return Ok(v.to_string());
        }
        it.next().cloned().ok_or_else(|| format!("`{flag}` needs a value"))
    };

    let short_value = |prefix: &str,
                       arg: &str,
                       it: &mut std::iter::Peekable<std::slice::Iter<'_, String>>|
     -> Result<String, String> {
        let rest = &arg[prefix.len()..];
        if let Some(stripped) = rest.strip_prefix('=') {
            Ok(stripped.to_string())
        } else if !rest.is_empty() {
            Ok(rest.to_string())
        } else {
            it.next().cloned().ok_or_else(|| format!("`{prefix}` needs a value"))
        }
    };

    while let Some(arg) = it.next() {
        let a = arg.as_str();
        match a {
            "-h" | "--help" => return Ok(Parsed::Done(USAGE.to_string())),
            "-V" | "--version" => return Ok(Parsed::Done(format!("vsc {}\n", crate::VERSION))),
            "-vV" => {
                return Ok(Parsed::Done(format!(
                    "vsc {v}\nbinary: vsc\nrelease: {v}\nhost: {host}\n",
                    v = crate::VERSION,
                    host = host_triple()
                )));
            }
            "-O" => {
                sess.cg.opt_level = OptLevel::Default;
                sess.debug_assertions = false;
            }
            "-g" => sess.cg.debuginfo = DebugInfo::Full,
            _ if a == "--crate-name" || a.starts_with("--crate-name=") => {
                sess.crate_name = Some(value("--crate-name", a, &mut it)?);
            }
            _ if a == "--crate-type" || a.starts_with("--crate-type=") => {
                for ty in value("--crate-type", a, &mut it)?.split(',') {
                    sess.crate_types.push(CrateType::parse(ty)?);
                }
            }
            _ if a == "--edition" || a.starts_with("--edition=") => {
                let e = value("--edition", a, &mut it)?;
                if e != "2024" {
                    return Err(format!("Vertex has one edition, 2024; `--edition {e}` isn't supported"));
                }
                sess.edition = 2024;
            }
            _ if a == "--error-format" || a.starts_with("--error-format=") => {
                let fmt_str = value("--error-format", a, &mut it)?;
                error_format = match fmt_str.as_str() {
                    "human" => {
                        sess.error_format = SessErrorFormat::Human;
                        ErrorFormat::Human
                    }
                    "short" => {
                        sess.error_format = SessErrorFormat::Short;
                        ErrorFormat::Short
                    }
                    other => return Err(format!("unknown error format: `{other}`")),
                };
            }
            _ if a == "--color" || a.starts_with("--color=") => {
                sess.color = match value("--color", a, &mut it)?.as_str() {
                    "auto" => ColorConfig::Auto,
                    "always" => ColorConfig::Always,
                    "never" => ColorConfig::Never,
                    other => return Err(format!("unknown color setting: `{other}`")),
                };
            }
            "--test" => sess.test = true,
            _ if a == "--cfg" || a.starts_with("--cfg=") => {
                sess.cfgs.push(value("--cfg", a, &mut it)?);
            }
            _ if a == "--target" || a.starts_with("--target=") => {
                sess.target_triple = Some(value("--target", a, &mut it)?);
            }
            _ if a == "--sysroot" || a.starts_with("--sysroot=") => {
                sess.maybe_sysroot = Some(PathBuf::from(value("--sysroot", a, &mut it)?));
            }
            _ if a == "--out-dir" || a.starts_with("--out-dir=") => {
                sess.out_dir = Some(PathBuf::from(value("--out-dir", a, &mut it)?));
            }
            _ if a == "-o" || a.starts_with("-o=") || (a.starts_with("-o") && a.len() > 2) => {
                let out = short_value("-o", a, &mut it)?;
                sess.out_file = Some(if out == "-" {
                    OutFileName::Stdout
                } else {
                    OutFileName::Real(PathBuf::from(out))
                });
            }
            _ if a == "-L" || a.starts_with("-L") => {
                let raw = short_value("-L", a, &mut it)?;
                sess.search_paths.push(parse_search_path(&raw)?);
            }
            _ if a == "-l" || a.starts_with("-l") => {
                let raw = short_value("-l", a, &mut it)?;
                sess.libs.push(parse_native_lib(&raw)?);
            }
            _ if a == "--extern" || a.starts_with("--extern=") => {
                let raw = value("--extern", a, &mut it)?;
                parse_extern(&mut sess, &raw)?;
            }
            _ if a == "--print" || a.starts_with("--print=") => {
                let raw = value("--print", a, &mut it)?;
                sess.prints.push(parse_print_request(&raw)?);
            }
            _ if a == "--cap-lints" || a.starts_with("--cap-lints=") => {
                let _ = value("--cap-lints", a, &mut it)?;
            }
            _ if a == "--emit" || a.starts_with("--emit=") => {
                let emits = explicit_emits.get_or_insert_with(OutputTypes::default);
                for item in value("--emit", a, &mut it)?.split(',') {
                    let (kind_str, path) = match item.split_once('=') {
                        Some((k, p)) => (k, Some(if p == "-" { OutFileName::Stdout } else { OutFileName::Real(PathBuf::from(p)) })),
                        None => (item, None),
                    };
                    let kind = OutputType::parse_emit(kind_str)?;
                    emits.0.insert(kind, path);
                }
            }
            _ if a == "-C" || a.starts_with("-C") => {
                let cg = short_value("-C", a, &mut it)?;
                parse_codegen_opt(&mut sess, &cg)?;
            }
            _ if a == "-Z" || a.starts_with("-Z") => {
                let z = short_value("-Z", a, &mut it)?;
                match z.as_str() {
                    "parse-only" => sess.unstable_opts.parse_only = true,
                    "dump-resolutions" => sess.unstable_opts.dump_resolutions = true,
                    "dump-signatures" => sess.unstable_opts.dump_signatures = true,
                    "dump-types" => sess.unstable_opts.dump_types = true,
                    "dump-mir" => sess.unstable_opts.dump_mir = true,
                    "validate-mir" => sess.unstable_opts.validate_mir = true,
                    "run-mir" => sess.unstable_opts.run_mir = true,
                    "print-type-sizes" => sess.unstable_opts.print_type_sizes = true,
                    "save-temps" => {
                        sess.unstable_opts.save_temps = true;
                        sess.cg.save_temps = true;
                    }
                    "unpretty=ast" => unpretty = Some(Unpretty::Ast),
                    "unpretty=ast-tree" => unpretty = Some(Unpretty::AstTree),
                    "unpretty=hir" => unpretty = Some(Unpretty::Hir),
                    "unpretty=hir-tree" => unpretty = Some(Unpretty::HirTree),
                    "unpretty=thir-tree" => unpretty = Some(Unpretty::ThirTree),
                    other => return Err(format!("unknown `-Z` option: `{other}`")),
                }
            }
            _ if a.starts_with('-') && a != "-" => return Err(format!("unknown option: `{a}`")),
            _ => {
                if input.replace(PathBuf::from(a)).is_some() {
                    return Err("more than one input file given".to_string());
                }
            }
        }
    }

    if let Some(emits) = explicit_emits {
        sess.output_types = emits;
    }

    if let Some(early) = handle_early_prints(&sess)? {
        return Ok(Parsed::Done(early));
    }

    let Some(input) = input else {
        return Ok(Parsed::Done(USAGE.to_string()));
    };

    let check_only = !sess.output_types.should_codegen()
        && sess.output_types.contains_key(OutputType::Metadata);

    Ok(Parsed::Run(Options {
        input,
        crate_name: sess.crate_name.clone(),
        error_format,
        color: sess.color,
        parse_only: sess.unstable_opts.parse_only,
        unpretty,
        dump_resolutions: sess.unstable_opts.dump_resolutions,
        dump_signatures: sess.unstable_opts.dump_signatures,
        dump_types: sess.unstable_opts.dump_types,
        dump_mir: sess.unstable_opts.dump_mir,
        validate_mir: sess.unstable_opts.validate_mir,
        run_mir: sess.unstable_opts.run_mir,
        check_only,
        cfgs: sess.cfgs.clone(),
        test: sess.test,
        target: sess.target_triple.clone(),
        sess,
    }))
}

fn parse_search_path(raw: &str) -> Result<SearchPath, String> {
    let (kind, dir) = match raw.split_once('=') {
        Some(("native", p)) => (SearchPathKind::Native, p),
        Some(("crate", p)) => (SearchPathKind::Crate, p),
        Some(("dependency", p)) => (SearchPathKind::Dependency, p),
        Some(("framework", p)) => (SearchPathKind::Framework, p),
        Some(("all", p)) => (SearchPathKind::All, p),
        Some((other, _)) => return Err(format!("unknown search path kind: `{other}`")),
        None => (SearchPathKind::All, raw),
    };
    Ok(SearchPath { kind, dir: PathBuf::from(dir) })
}

fn parse_native_lib(raw: &str) -> Result<NativeLib, String> {
    let (kind, name) = match raw.split_once('=') {
        Some(("dylib", n)) => (NativeLibKind::Dylib, n),
        Some(("framework", n)) => (NativeLibKind::Framework, n),
        Some(("static", n)) => {
            (NativeLibKind::Static { bundle: None, whole_archive: None }, n)
        }
        Some((other, _)) => return Err(format!("unknown native library kind: `{other}`")),
        None => (NativeLibKind::Unspecified, raw),
    };
    Ok(NativeLib { kind, name: name.to_string() })
}

fn parse_extern(sess: &mut SessOptions, raw: &str) -> Result<(), String> {
    let (name, loc) = match raw.split_once('=') {
        Some((n, p)) => (n, Some(PathBuf::from(p))),
        None => (raw, None),
    };
    if name.is_empty() {
        return Err("`--extern` crate name cannot be empty".to_string());
    }
    sess.externs.0.insert(name.to_string(), ExternEntry { location: loc, force_private_dep: false });
    Ok(())
}

fn parse_print_request(raw: &str) -> Result<PrintRequest, String> {
    match raw {
        "target-list" => Ok(PrintRequest::TargetList),
        "target-spec-json" => Ok(PrintRequest::TargetSpecs),
        "target-cpus" => Ok(PrintRequest::TargetCpus),
        "target-features" => Ok(PrintRequest::TargetFeatures),
        "cfg" => Ok(PrintRequest::Cfg),
        "sysroot" => Ok(PrintRequest::Sysroot),
        "target-libdir" => Ok(PrintRequest::TargetLibdir),
        "file-names" => Ok(PrintRequest::FileNames),
        "crate-name" => Ok(PrintRequest::CrateName),
        "native-static-libs" => Ok(PrintRequest::NativeStaticLibs),
        other => Err(format!("unknown print request: `{other}`")),
    }
}

fn parse_codegen_opt(sess: &mut SessOptions, raw: &str) -> Result<(), String> {
    let (key, val) = match raw.split_once('=') {
        Some((k, v)) => (k, Some(v)),
        None => (raw, None),
    };
    match key {
        "opt-level" => {
            let v = val.ok_or("`-C opt-level` requires a value")?;
            sess.cg.opt_level = OptLevel::parse(v)?;
        }
        "debuginfo" => {
            let v = val.ok_or("`-C debuginfo` requires a value")?;
            sess.cg.debuginfo = DebugInfo::parse(v)?;
        }
        "linker" => {
            let v = val.ok_or("`-C linker` requires a value")?;
            sess.cg.linker = Some(PathBuf::from(v));
        }
        "link-arg" => {
            let v = val.ok_or("`-C link-arg` requires a value")?;
            sess.cg.link_args.push(v.to_string());
        }
        "link-args" => {
            let v = val.ok_or("`-C link-args` requires a value")?;
            sess.cg.link_args.extend(v.split_whitespace().map(str::to_string));
        }
        "codegen-units" => {
            let v = val.ok_or("`-C codegen-units` requires a value")?;
            let n: usize = v
                .parse()
                .map_err(|_| format!("invalid `-C codegen-units` value `{v}`"))?;
            if n == 0 {
                return Err("`-C codegen-units` must be at least 1".to_string());
            }
            sess.cg.codegen_units = Some(n);
        }
        "panic" => {
            let v = val.ok_or("`-C panic` requires a value")?;
            sess.cg.panic = Some(match v {
                "unwind" => PanicStrategy::Unwind,
                "abort" => PanicStrategy::Abort,
                "immediate-abort" => PanicStrategy::ImmediateAbort,
                other => return Err(format!("unknown `-C panic` strategy `{other}`")),
            });
        }
        "relocation-model" => {
            let v = val.ok_or("`-C relocation-model` requires a value")?;
            sess.cg.relocation_model = Some(match v {
                "static" => RelocModel::Static,
                "pic" => RelocModel::Pic,
                "pie" => RelocModel::Pie,
                "dynamic-no-pic" => RelocModel::DynamicNoPic,
                other => return Err(format!("unknown relocation model `{other}`")),
            });
        }
        "code-model" => {
            let v = val.ok_or("`-C code-model` requires a value")?;
            sess.cg.code_model = Some(match v {
                "tiny" => CodeModel::Tiny,
                "small" => CodeModel::Small,
                "kernel" => CodeModel::Kernel,
                "medium" => CodeModel::Medium,
                "large" => CodeModel::Large,
                other => return Err(format!("unknown code model `{other}`")),
            });
        }
        "target-cpu" => {
            let v = val.ok_or("`-C target-cpu` requires a value")?;
            sess.cg.target_cpu = Some(v.to_string());
        }
        "target-feature" => {
            let v = val.ok_or("`-C target-feature` requires a value")?;
            if !sess.cg.target_feature.is_empty() {
                sess.cg.target_feature.push(',');
            }
            sess.cg.target_feature.push_str(v);
        }
        "debug-assertions" => {
            sess.debug_assertions = match val {
                None | Some("y") | Some("yes") | Some("on") | Some("true") => true,
                Some("n") | Some("no") | Some("off") | Some("false") => false,
                Some(other) => return Err(format!("unknown bool value for `-C debug-assertions`: `{other}`")),
            };
        }
        "embed-bitcode" => {
            sess.cg.embed_bitcode = match val {
                None | Some("y") | Some("yes") | Some("on") | Some("true") => true,
                Some("n") | Some("no") | Some("off") | Some("false") => false,
                Some(other) => return Err(format!("unknown bool value for `-C embed-bitcode`: `{other}`")),
            };
        }
        "lto" => {
            sess.cg.lto = match val {
                None | Some("y") | Some("yes") | Some("thin") | Some("fat") => true,
                Some("n") | Some("no") | Some("off") => false,
                Some(other) => return Err(format!("unknown `-C lto` value `{other}`")),
            };
        }
        "overflow-checks" => {
            sess.cg.overflow_checks = Some(match val {
                None | Some("y") | Some("yes") | Some("on") | Some("true") => true,
                Some("n") | Some("no") | Some("off") | Some("false") => false,
                Some(other) => {
                    return Err(format!("unknown bool value for `-C overflow-checks`: `{other}`"));
                }
            });
        }
        "save-temps" => sess.cg.save_temps = true,
        "rpath" => sess.cg.rpath = true,
        "no-redzone" => sess.cg.no_redzone = Some(true),
        "strip" => {
            let v = val.ok_or("`-C strip` requires a value")?;
            sess.cg.strip = match v {
                "none" => Strip::None,
                "debuginfo" => Strip::Debuginfo,
                "symbols" => Strip::Symbols,
                other => return Err(format!("unknown `-C strip` value `{other}`")),
            };
        }
        "backend-args" => {
            let v = val.ok_or("`-C backend-args` requires a value")?;
            sess.cg.backend_args.extend(v.split_whitespace().map(str::to_string));
        }
        "passes" => {
            let v = val.ok_or("`-C passes` requires a value")?;
            sess.cg.passes.extend(v.split_whitespace().map(str::to_string));
        }
        "extra-filename" => {
            let v = val.ok_or("`-C extra-filename` requires a value")?;
            sess.cg.extra_filename = v.to_string();
        }
        "metadata" => {
            let v = val.ok_or("`-C metadata` requires a value")?;
            sess.cg.metadata.push(v.to_string());
        }
        other => return Err(format!("unknown codegen option: `{other}`")),
    }
    Ok(())
}

fn handle_early_prints(sess: &SessOptions) -> Result<Option<String>, String> {
    if sess.prints.is_empty() {
        return Ok(None);
    }
    let target_info = match &sess.target_triple {
        None => TargetInfo::host(),
        Some(t) => TargetInfo::from_triple(t).ok_or_else(|| format!("unknown target `{t}`"))?,
    };
    let mut out = String::new();
    for req in &sess.prints {
        match req {
            PrintRequest::TargetList => {
                for t in [
                    "aarch64-apple-darwin",
                    "x86_64-apple-darwin",
                    "aarch64-linux-android",
                    "x86_64-unknown-linux-gnu",
                    "aarch64-unknown-linux-gnu",
                    "x86_64-pc-windows-msvc",
                    "aarch64-pc-windows-msvc",
                ] {
                    out.push_str(t);
                    out.push('\n');
                }
            }
            PrintRequest::Cfg => {
                let panic = vsc_session::config::PanicStrategy::Unwind;
                let panic = sess.cg.panic.unwrap_or(panic);
                let cfg = vsc_session::config::default_configuration_with_panic(
                    &target_info,
                    sess.debug_assertions,
                    sess.test,
                    panic,
                );
                let mut lines: Vec<String> = cfg
                    .iter()
                    .map(|(k, v)| match v {
                        Some(val) => format!("{k}=\"{val}\""),
                        None => format!("{k}"),
                    })
                    .collect();
                lines.sort();
                for l in lines {
                    out.push_str(&l);
                    out.push('\n');
                }
            }
            PrintRequest::Sysroot => {
                let sysroot = vsc_session::session::resolve_sysroot(sess);
                out.push_str(&sysroot.display().to_string());
                out.push('\n');
            }
            PrintRequest::TargetLibdir => {
                let sysroot = vsc_session::session::resolve_sysroot(sess);
                let libdir =
                    sysroot.join("lib").join("rustlib").join(&target_info.triple).join("lib");
                out.push_str(&libdir.display().to_string());
                out.push('\n');
            }
            _ => return Ok(None),
        }
    }
    if out.is_empty() { Ok(None) } else { Ok(Some(out)) }
}

fn host_triple() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        ("aarch64", "windows") => "aarch64-pc-windows-msvc",
        ("aarch64", "android") => "aarch64-linux-android",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_native_codegen_flags() {
        let args: Vec<String> = [
            "main.vs",
            "-o",
            "build/app",
            "--crate-type=bin",
            "--emit=link,obs-ir",
            "-C",
            "opt-level=3",
            "-C",
            "panic=abort",
            "-C",
            "link-arg=-Wl,-dead_strip",
            "-L",
            "native=/opt/local/lib",
            "-l",
            "dylib=System",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        let Parsed::Run(opts) = parse(&args).unwrap() else {
            panic!("expected Run");
        };
        assert_eq!(opts.sess.crate_types, vec![CrateType::Executable]);
        assert!(opts.sess.output_types.contains_key(OutputType::Exe));
        assert!(opts.sess.output_types.contains_key(OutputType::ObsIr));
        assert_eq!(opts.sess.cg.opt_level, OptLevel::Aggressive);
        assert_eq!(opts.sess.cg.panic, Some(PanicStrategy::Abort));
        assert_eq!(opts.sess.cg.link_args, vec!["-Wl,-dead_strip".to_string()]);
        assert_eq!(opts.sess.search_paths.len(), 1);
        assert_eq!(opts.sess.libs.len(), 1);
        assert!(!opts.check_only);
    }
}
