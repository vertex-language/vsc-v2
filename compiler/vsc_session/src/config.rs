//! Configuration: target info, `cfg` set, crate types, output types, codegen & session options
//! (`rustc_session::config`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use vsc_data_structures::FxHashSet;
use vsc_errors::ColorConfig;
use vsc_span::{Symbol, sym};
pub use vsc_target::spec::{CodeModel, PanicStrategy, RelocModel, Target};

/// What `@cfg` can test about the target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetInfo {
    /// Rust's triple: `aarch64-apple-darwin`.
    pub triple: String,
    pub arch: &'static str,
    pub os: &'static str,
    pub family: &'static str,
    pub env: &'static str,
    pub vendor: &'static str,
    pub pointer_width: u32,
    pub endian: &'static str,
}

impl TargetInfo {
    /// The machine vsc runs on.
    pub fn host() -> TargetInfo {
        let arch = std::env::consts::ARCH;
        let os = std::env::consts::OS;
        TargetInfo::from_parts(arch, os).unwrap_or(TargetInfo {
            triple: format!("{arch}-unknown-{os}"),
            arch: leak(arch),
            os: leak(os),
            family: std::env::consts::FAMILY,
            env: "",
            vendor: "unknown",
            pointer_width: usize::BITS,
            endian: if cfg!(target_endian = "little") { "little" } else { "big" },
        })
    }

    /// Supported targets by Rust triple.
    pub fn from_triple(triple: &str) -> Option<TargetInfo> {
        let (arch, os) = match triple {
            "aarch64-apple-darwin" => ("aarch64", "macos"),
            "x86_64-apple-darwin" => ("x86_64", "macos"),
            "aarch64-linux-android" => ("aarch64", "android"),
            "x86_64-unknown-linux-gnu" => ("x86_64", "linux"),
            "aarch64-unknown-linux-gnu" => ("aarch64", "linux"),
            "x86_64-pc-windows-msvc" => ("x86_64", "windows"),
            "aarch64-pc-windows-msvc" => ("aarch64", "windows"),
            _ => return None,
        };
        TargetInfo::from_parts(arch, os)
    }

    pub fn to_target(&self) -> Result<Target, String> {
        Target::from_triple(&self.triple)
    }

    fn from_parts(arch: &str, os: &str) -> Option<TargetInfo> {
        let (triple, arch, os, family, env, vendor) = match (arch, os) {
            ("aarch64", "macos") => ("aarch64-apple-darwin", "aarch64", "macos", "unix", "", "apple"),
            ("x86_64", "macos") => ("x86_64-apple-darwin", "x86_64", "macos", "unix", "", "apple"),
            ("aarch64", "android") => ("aarch64-linux-android", "aarch64", "android", "unix", "", "unknown"),
            ("x86_64", "linux") => ("x86_64-unknown-linux-gnu", "x86_64", "linux", "unix", "gnu", "unknown"),
            ("aarch64", "linux") => ("aarch64-unknown-linux-gnu", "aarch64", "linux", "unix", "gnu", "unknown"),
            ("x86_64", "windows") => ("x86_64-pc-windows-msvc", "x86_64", "windows", "windows", "msvc", "pc"),
            ("aarch64", "windows") => ("aarch64-pc-windows-msvc", "aarch64", "windows", "windows", "msvc", "pc"),
            _ => return None,
        };
        Some(TargetInfo {
            triple: triple.to_string(),
            arch,
            os,
            family,
            env,
            vendor,
            pointer_width: 64,
            endian: "little",
        })
    }
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// The set of `name` and `name = "value"` options `@cfg` tests against.
#[derive(Clone, Debug, Default)]
pub struct Cfg {
    set: FxHashSet<(Symbol, Option<Symbol>)>,
}

impl Cfg {
    pub fn insert(&mut self, name: &str, value: Option<&str>) {
        self.set.insert((Symbol::intern(name), value.map(Symbol::intern)));
    }

    pub fn contains(&self, name: Symbol, value: Option<Symbol>) -> bool {
        self.set.contains(&(name, value))
    }

    /// Parses a `--cfg` argument: `name` or `name="value"`.
    pub fn insert_from_arg(&mut self, arg: &str) -> Result<(), String> {
        match arg.split_once('=') {
            None => {
                self.insert(arg, None);
                Ok(())
            }
            Some((name, value)) => {
                let value = value
                    .strip_prefix('"')
                    .and_then(|v| v.strip_suffix('"'))
                    .ok_or_else(|| format!("invalid `--cfg` argument: `{arg}` (the value must be quoted)"))?;
                self.insert(name, Some(value));
                Ok(())
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (Symbol, Option<Symbol>)> + '_ {
        self.set.iter().copied()
    }
}

/// The options every compilation has, as in rustc: the target's, plus
/// `debug_assertions` and `test` when they apply.
pub fn default_configuration(target: &TargetInfo, debug_assertions: bool, test: bool) -> Cfg {
    default_configuration_with_panic(target, debug_assertions, test, PanicStrategy::Unwind)
}

pub fn default_configuration_with_panic(
    target: &TargetInfo,
    debug_assertions: bool,
    test: bool,
    panic: PanicStrategy,
) -> Cfg {
    let mut cfg = Cfg::default();
    cfg.insert("target_arch", Some(target.arch));
    cfg.insert("target_os", Some(target.os));
    cfg.insert("target_family", Some(target.family));
    cfg.insert(target.family, None);
    cfg.insert("target_env", Some(target.env));
    cfg.insert("target_vendor", Some(target.vendor));
    cfg.insert("target_pointer_width", Some(&target.pointer_width.to_string()));
    cfg.insert("target_endian", Some(target.endian));
    let panic_str = match panic {
        PanicStrategy::Unwind => "unwind",
        PanicStrategy::Abort | PanicStrategy::ImmediateAbort => "abort",
    };
    cfg.insert("panic", Some(panic_str));
    if debug_assertions {
        cfg.insert("debug_assertions", None);
    }
    if test {
        cfg.insert(sym::test.as_str(), None);
    }
    cfg
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum CrateType {
    Executable,
    Rlib,
    Dylib,
    Cdylib,
    Staticlib,
    ProcMacro,
}

impl CrateType {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "bin" => Ok(CrateType::Executable),
            "lib" | "rlib" => Ok(CrateType::Rlib),
            "dylib" => Ok(CrateType::Dylib),
            "cdylib" => Ok(CrateType::Cdylib),
            "staticlib" => Ok(CrateType::Staticlib),
            "proc-macro" => Ok(CrateType::ProcMacro),
            other => Err(format!("unknown crate type: `{other}`")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CrateType::Executable => "bin",
            CrateType::Rlib => "rlib",
            CrateType::Dylib => "dylib",
            CrateType::Cdylib => "cdylib",
            CrateType::Staticlib => "staticlib",
            CrateType::ProcMacro => "proc-macro",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum OutputType {
    Bitcode,
    Assembly,
    ObsIr,
    Mir,
    Metadata,
    Object,
    Exe,
    DepInfo,
}

impl OutputType {
    pub fn parse_emit(s: &str) -> Result<Self, String> {
        match s {
            "bc" | "obs-bc" => Ok(OutputType::Bitcode),
            "asm" => Ok(OutputType::Assembly),
            "ir" | "obs-ir" => Ok(OutputType::ObsIr),
            "mir" => Ok(OutputType::Mir),
            "metadata" => Ok(OutputType::Metadata),
            "obj" => Ok(OutputType::Object),
            "link" => Ok(OutputType::Exe),
            "dep-info" => Ok(OutputType::DepInfo),
            other => Err(format!("unknown emit kind: `{other}`")),
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            OutputType::Bitcode => "bc",
            OutputType::Assembly => "s",
            OutputType::ObsIr => "obs",
            OutputType::Mir => "mir",
            OutputType::Metadata => "rmeta",
            OutputType::Object => "o",
            OutputType::Exe => "",
            OutputType::DepInfo => "d",
        }
    }

    pub fn is_compatible_with_codegen_units_and_single_output_file(self) -> bool {
        !matches!(self, OutputType::Bitcode | OutputType::Assembly | OutputType::ObsIr | OutputType::Object)
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OutFileName {
    Real(PathBuf),
    Stdout,
}

impl OutFileName {
    pub fn as_path(&self) -> Option<&Path> {
        match self {
            OutFileName::Real(p) => Some(p),
            OutFileName::Stdout => None,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct OutputTypes(pub BTreeMap<OutputType, Option<OutFileName>>);

impl OutputTypes {
    pub fn contains_key(&self, k: OutputType) -> bool {
        self.0.contains_key(&k)
    }

    pub fn should_codegen(&self) -> bool {
        self.0.keys().any(|k| {
            matches!(
                k,
                OutputType::Bitcode
                    | OutputType::Assembly
                    | OutputType::ObsIr
                    | OutputType::Object
                    | OutputType::Exe
            )
        })
    }

    pub fn should_link(&self) -> bool {
        self.0.contains_key(&OutputType::Exe)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum OptLevel {
    #[default]
    No,
    Less,
    Default,
    Aggressive,
    Size,
    SizeMin,
}

impl OptLevel {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "0" => Ok(OptLevel::No),
            "1" => Ok(OptLevel::Less),
            "2" => Ok(OptLevel::Default),
            "3" => Ok(OptLevel::Aggressive),
            "s" => Ok(OptLevel::Size),
            "z" => Ok(OptLevel::SizeMin),
            other => Err(format!("optimization level needs to be between 0-3, s or z (instead was `{other}`)")),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum DebugInfo {
    #[default]
    None,
    LineDirectivesOnly,
    LineTablesOnly,
    Limited,
    Full,
}

impl DebugInfo {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "0" | "none" => Ok(DebugInfo::None),
            "line-directives-only" => Ok(DebugInfo::LineDirectivesOnly),
            "line-tables-only" => Ok(DebugInfo::LineTablesOnly),
            "1" | "limited" => Ok(DebugInfo::Limited),
            "2" | "full" => Ok(DebugInfo::Full),
            other => Err(format!("unknown debuginfo level `{other}`")),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Strip {
    #[default]
    None,
    Debuginfo,
    Symbols,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CodegenOptions {
    pub opt_level: OptLevel,
    pub debuginfo: DebugInfo,
    pub linker: Option<PathBuf>,
    pub link_args: Vec<String>,
    pub link_self_contained: Option<bool>,
    pub codegen_units: Option<usize>,
    pub panic: Option<PanicStrategy>,
    pub relocation_model: Option<RelocModel>,
    pub code_model: Option<CodeModel>,
    pub target_cpu: Option<String>,
    pub target_feature: String,
    pub no_redzone: Option<bool>,
    pub embed_bitcode: bool,
    pub lto: bool,
    pub overflow_checks: Option<bool>,
    pub backend_args: Vec<String>,
    pub passes: Vec<String>,
    pub save_temps: bool,
    pub rpath: bool,
    pub strip: Strip,
    pub metadata: Vec<String>,
    pub extra_filename: String,
}

impl CodegenOptions {
    /// Returns whether integer `+`, `-`, `*` emit runtime overflow checks.
    /// Matches rustc: enabled at `-O0`, disabled at `-O1`/`-O2`/`-O3`/`-Os`/`-Oz`
    /// unless explicitly overridden via `-C overflow-checks=yes|no`.
    pub fn overflow_checks(&self) -> bool {
        self.overflow_checks.unwrap_or(self.opt_level == OptLevel::No)
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct UnstableOptions {
    pub parse_only: bool,
    pub dump_resolutions: bool,
    pub dump_signatures: bool,
    pub dump_types: bool,
    pub dump_mir: bool,
    pub validate_mir: bool,
    pub run_mir: bool,
    pub print_type_sizes: bool,
    pub save_temps: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PrintRequest {
    TargetList,
    TargetSpecs,
    TargetCpus,
    TargetFeatures,
    Cfg,
    Sysroot,
    TargetLibdir,
    FileNames,
    CrateName,
    NativeStaticLibs,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SearchPathKind {
    Native,
    Crate,
    Dependency,
    Framework,
    All,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SearchPath {
    pub kind: SearchPathKind,
    pub dir: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NativeLibKind {
    Dylib,
    Framework,
    Static { bundle: Option<bool>, whole_archive: Option<bool> },
    Unspecified,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NativeLib {
    pub name: String,
    pub kind: NativeLibKind,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ExternEntry {
    pub location: Option<PathBuf>,
    pub force_private_dep: bool,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Externs(pub BTreeMap<String, ExternEntry>);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ErrorFormat {
    #[default]
    Human,
    Short,
}

/// Complete session configuration (`rustc_session::config::Options`).
#[derive(Clone, Debug)]
pub struct Options {
    pub crate_types: Vec<CrateType>,
    pub output_types: OutputTypes,
    pub crate_name: Option<String>,
    pub cg: CodegenOptions,
    pub unstable_opts: UnstableOptions,
    pub error_format: ErrorFormat,
    pub color: ColorConfig,
    pub cfgs: Vec<String>,
    pub test: bool,
    pub debug_assertions: bool,
    pub target_triple: Option<String>,
    pub maybe_sysroot: Option<PathBuf>,
    pub search_paths: Vec<SearchPath>,
    pub libs: Vec<NativeLib>,
    pub externs: Externs,
    pub prints: Vec<PrintRequest>,
    pub out_dir: Option<PathBuf>,
    pub out_file: Option<OutFileName>,
    pub edition: u32,
}

impl Default for Options {
    fn default() -> Self {
        let mut output_types = OutputTypes::default();
        output_types.0.insert(OutputType::Exe, None);
        Self {
            crate_types: Vec::new(),
            output_types,
            crate_name: None,
            cg: CodegenOptions { embed_bitcode: true, ..CodegenOptions::default() },
            unstable_opts: UnstableOptions::default(),
            error_format: ErrorFormat::Human,
            color: ColorConfig::Auto,
            cfgs: Vec::new(),
            test: false,
            debug_assertions: true,
            target_triple: None,
            maybe_sysroot: None,
            search_paths: Vec::new(),
            libs: Vec::new(),
            externs: Externs::default(),
            prints: Vec::new(),
            out_dir: None,
            out_file: None,
            edition: 2024,
        }
    }
}

impl Options {
    pub fn effective_crate_types(&self) -> Vec<CrateType> {
        if self.crate_types.is_empty() {
            vec![CrateType::Executable]
        } else {
            let set: BTreeSet<_> = self.crate_types.iter().copied().collect();
            set.into_iter().collect()
        }
    }

    pub fn effective_panic_strategy(&self, target: &Target) -> PanicStrategy {
        self.cg.panic.unwrap_or(target.panic_strategy)
    }
}

/// Output paths resolved for the current crate compilation (`rustc_session::config::OutputFilenames`).
#[derive(Clone, Debug)]
pub struct OutputFilenames {
    pub out_directory: PathBuf,
    pub filestem: String,
    pub single_output_file: Option<OutFileName>,
    pub temps_directory: Option<PathBuf>,
    pub outputs: OutputTypes,
    pub extra: String,
}

impl OutputFilenames {
    pub fn path(&self, flavor: OutputType) -> OutFileName {
        if let Some(Some(explicit)) = self.outputs.0.get(&flavor) {
            return explicit.clone();
        }
        if let Some(ref single) = self.single_output_file {
            if matches!(flavor, OutputType::Exe) || self.outputs.0.len() == 1 {
                return single.clone();
            }
        }
        let stem = if self.extra.is_empty() {
            self.filestem.clone()
        } else {
            format!("{}{}", self.filestem, self.extra)
        };
        let ext = flavor.extension();
        let filename = if ext.is_empty() { stem } else { format!("{stem}.{ext}") };
        OutFileName::Real(self.out_directory.join(filename))
    }

    pub fn temp_path(&self, flavor: OutputType, cgu_name: Option<&str>) -> PathBuf {
        let dir = self.temps_directory.as_ref().unwrap_or(&self.out_directory);
        let stem = if let Some(cgu) = cgu_name {
            format!("{}.{cgu}", self.filestem)
        } else {
            self.filestem.clone()
        };
        let ext = flavor.extension();
        let filename = if ext.is_empty() { stem } else { format!("{stem}.{ext}") };
        dir.join(filename)
    }

    pub fn exe_path(&self, target: &Target) -> PathBuf {
        if let Some(Some(OutFileName::Real(p))) = self.outputs.0.get(&OutputType::Exe) {
            return p.clone();
        }
        if let Some(OutFileName::Real(ref single)) = self.single_output_file {
            return single.clone();
        }
        let filename = format!("{}{}{}", self.filestem, self.extra, target.exe_suffix);
        self.out_directory.join(filename)
    }

    pub fn with_extension(&self, extension: &str) -> PathBuf {
        let filename = format!("{}{}.{extension}", self.filestem, self.extra);
        self.out_directory.join(filename)
    }
}
