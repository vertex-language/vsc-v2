//! Target specifications (`rustc_target::spec`).

use std::fmt;
use std::ops::Deref;

use vsc_abi::{AddressSpace, Endian, HasDataLayout, TargetDataLayout};

pub mod abi_map;
pub mod apple;

pub use abi_map::{AbiMap, AbiMapping};
pub use apple::{OsVersion, deployment_target_env_var};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Arch {
    AArch64,
    Arm64EC,
    Arm,
    X86,
    X86_64,
    RiscV32,
    RiscV64,
    LoongArch32,
    LoongArch64,
    Wasm32,
    Wasm64,
    Other(String),
}

impl Arch {
    pub fn as_str(&self) -> &str {
        match self {
            Arch::AArch64 => "aarch64",
            Arch::Arm64EC => "arm64ec",
            Arch::Arm => "arm",
            Arch::X86 => "x86",
            Arch::X86_64 => "x86_64",
            Arch::RiscV32 => "riscv32",
            Arch::RiscV64 => "riscv64",
            Arch::LoongArch32 => "loongarch32",
            Arch::LoongArch64 => "loongarch64",
            Arch::Wasm32 => "wasm32",
            Arch::Wasm64 => "wasm64",
            Arch::Other(s) => s,
        }
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Os {
    MacOs,
    IOs,
    Linux,
    Android,
    Windows,
    None,
    Other(String),
}

impl Os {
    pub fn as_str(&self) -> &str {
        match self {
            Os::MacOs => "macos",
            Os::IOs => "ios",
            Os::Linux => "linux",
            Os::Android => "android",
            Os::Windows => "windows",
            Os::None => "none",
            Os::Other(s) => s,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Env {
    #[default]
    Unspecified,
    Gnu,
    Musl,
    Msvc,
    Android,
}

impl Env {
    pub fn as_str(self) -> &'static str {
        match self {
            Env::Unspecified => "",
            Env::Gnu => "gnu",
            Env::Musl => "musl",
            Env::Msvc => "msvc",
            Env::Android => "android",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum BinaryFormat {
    #[default]
    Elf,
    MachO,
    Coff,
    Wasm,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cc {
    Yes,
    No,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Lld {
    Yes,
    No,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LinkerFlavor {
    Darwin(Cc, Lld),
    Gnu(Cc, Lld),
    Msvc(Lld),
    WasmLld(Cc),
}

impl LinkerFlavor {
    pub fn uses_cc(self) -> bool {
        matches!(
            self,
            LinkerFlavor::Darwin(Cc::Yes, _)
                | LinkerFlavor::Gnu(Cc::Yes, _)
                | LinkerFlavor::WasmLld(Cc::Yes)
        )
    }

    pub fn uses_lld(self) -> bool {
        matches!(
            self,
            LinkerFlavor::Darwin(_, Lld::Yes)
                | LinkerFlavor::Gnu(_, Lld::Yes)
                | LinkerFlavor::Msvc(Lld::Yes)
                | LinkerFlavor::WasmLld(_)
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum PanicStrategy {
    #[default]
    Unwind,
    Abort,
    ImmediateAbort,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum RelocModel {
    Static,
    #[default]
    Pic,
    Pie,
    DynamicNoPic,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum CodeModel {
    Tiny,
    #[default]
    Small,
    Kernel,
    Medium,
    Large,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum FramePointer {
    Always,
    NonLeaf,
    #[default]
    MayOmit,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RustcAbi {
    Softfloat,
    X86Sse2,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum TlsModel {
    #[default]
    GeneralDynamic,
    LocalDynamic,
    InitialExec,
    LocalExec,
    Emulated,
}

#[derive(Clone, Debug, Default)]
pub struct X86AbiOpt {
    pub regparm: Option<u32>,
    pub reg_struct_return: bool,
}

pub trait HasX86AbiOpt {
    fn x86_abi_opt(&self) -> X86AbiOpt;
}

#[derive(Clone, Debug)]
pub struct TargetOptions {
    pub endian: Endian,
    pub c_int_width: u16,
    pub os: Os,
    pub env: Env,
    pub vendor: String,
    pub families: Vec<&'static str>,
    pub linker_flavor: LinkerFlavor,
    pub linker: Option<String>,
    pub pre_link_args: Vec<(LinkerFlavor, Vec<String>)>,
    pub post_link_args: Vec<(LinkerFlavor, Vec<String>)>,
    pub link_env: Vec<(&'static str, &'static str)>,
    pub link_env_remove: Vec<&'static str>,
    pub cpu: String,
    pub features: String,
    pub dynamic_linking: bool,
    pub executables: bool,
    pub relocation_model: RelocModel,
    pub position_independent_executables: bool,
    pub static_position_independent_executables: bool,
    pub function_sections: bool,
    pub dll_prefix: &'static str,
    pub dll_suffix: &'static str,
    pub exe_suffix: &'static str,
    pub staticlib_prefix: &'static str,
    pub staticlib_suffix: &'static str,
    pub has_rpath: bool,
    pub is_like_darwin: bool,
    pub is_like_android: bool,
    pub is_like_windows: bool,
    pub is_like_msvc: bool,
    pub binary_format: BinaryFormat,
    pub frame_pointer: FramePointer,
    pub default_dwarf_version: u32,
    pub panic_strategy: PanicStrategy,
    pub has_thread_local: bool,
    pub tls_model: TlsModel,
    pub main_needs_argc_argv: bool,
    pub max_atomic_width: Option<u64>,
    pub default_uwtable: bool,
    pub requires_uwtable: bool,
    pub abi_return_struct_as_int: bool,
    pub rustc_abi: Option<RustcAbi>,
}

impl Default for TargetOptions {
    fn default() -> Self {
        Self {
            endian: Endian::Little,
            c_int_width: 32,
            os: Os::Linux,
            env: Env::Unspecified,
            vendor: "unknown".to_string(),
            families: vec!["unix"],
            linker_flavor: LinkerFlavor::Gnu(Cc::Yes, Lld::No),
            linker: None,
            pre_link_args: Vec::new(),
            post_link_args: Vec::new(),
            link_env: Vec::new(),
            link_env_remove: Vec::new(),
            cpu: "generic".to_string(),
            features: String::new(),
            dynamic_linking: false,
            executables: true,
            relocation_model: RelocModel::Pic,
            position_independent_executables: false,
            static_position_independent_executables: false,
            function_sections: true,
            dll_prefix: "lib",
            dll_suffix: ".so",
            exe_suffix: "",
            staticlib_prefix: "lib",
            staticlib_suffix: ".a",
            has_rpath: false,
            is_like_darwin: false,
            is_like_android: false,
            is_like_windows: false,
            is_like_msvc: false,
            binary_format: BinaryFormat::Elf,
            frame_pointer: FramePointer::MayOmit,
            default_dwarf_version: 4,
            panic_strategy: PanicStrategy::Unwind,
            has_thread_local: false,
            tls_model: TlsModel::GeneralDynamic,
            main_needs_argc_argv: true,
            max_atomic_width: None,
            default_uwtable: false,
            requires_uwtable: false,
            abi_return_struct_as_int: false,
            rustc_abi: None,
        }
    }
}

/// Complete target description matching `rustc_target::spec::Target`.
#[derive(Clone, Debug)]
pub struct Target {
    pub triple: String,
    pub target_triple: String,
    pub pointer_width: u16,
    pub data_layout: String,
    pub arch: Arch,
    pub options: TargetOptions,
}

impl Deref for Target {
    type Target = TargetOptions;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.options
    }
}

pub trait HasTargetSpec {
    fn target_spec(&self) -> &Target;
}

impl HasTargetSpec for Target {
    #[inline]
    fn target_spec(&self) -> &Target {
        self
    }
}

impl Target {
    pub fn parse_data_layout(&self) -> Result<TargetDataLayout, String> {
        TargetDataLayout::parse_from_datalayout_string(&self.data_layout, AddressSpace::ZERO)
            .map_err(|e| e.to_string())
    }

    pub fn host_triple() -> &'static str {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "aarch64-apple-darwin"
        } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
            "x86_64-apple-darwin"
        } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            "aarch64-unknown-linux-gnu"
        } else if cfg!(all(target_os = "windows", target_arch = "aarch64")) {
            "aarch64-pc-windows-msvc"
        } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            "x86_64-pc-windows-msvc"
        } else {
            "x86_64-unknown-linux-gnu"
        }
    }

    pub fn from_triple(triple: &str) -> Result<Self, String> {
        match triple {
            "aarch64-apple-darwin" => Ok(aarch64_apple_darwin()),
            "x86_64-apple-darwin" => Ok(x86_64_apple_darwin()),
            "x86_64-unknown-linux-gnu" => Ok(x86_64_unknown_linux_gnu()),
            "aarch64-unknown-linux-gnu" => Ok(aarch64_unknown_linux_gnu()),
            "aarch64-linux-android" => Ok(aarch64_linux_android()),
            "x86_64-pc-windows-msvc" => Ok(x86_64_pc_windows_msvc()),
            "aarch64-pc-windows-msvc" => Ok(aarch64_pc_windows_msvc()),
            other => Err(format!("unsupported target triple `{other}`")),
        }
    }
}

/// Wrapper tying a `Target` and its parsed `TargetDataLayout` together for ABI queries.
pub struct TargetAndDataLayout<'a> {
    pub target: &'a Target,
    pub data_layout: TargetDataLayout,
}

impl<'a> TargetAndDataLayout<'a> {
    pub fn new(target: &'a Target) -> Self {
        let data_layout = target
            .parse_data_layout()
            .expect("built-in target data layout string must be valid");
        Self { target, data_layout }
    }
}

impl HasDataLayout for TargetAndDataLayout<'_> {
    fn data_layout(&self) -> &TargetDataLayout {
        &self.data_layout
    }
}

impl HasTargetSpec for TargetAndDataLayout<'_> {
    fn target_spec(&self) -> &Target {
        self.target
    }
}

impl HasX86AbiOpt for TargetAndDataLayout<'_> {
    fn x86_abi_opt(&self) -> X86AbiOpt {
        X86AbiOpt::default()
    }
}

fn apple_macos_base(arch: Arch, target_triple: &str, cpu: &str) -> TargetOptions {
    let frame_pointer = match arch {
        Arch::AArch64 => FramePointer::NonLeaf,
        _ => FramePointer::Always,
    };
    TargetOptions {
        os: Os::MacOs,
        env: Env::Unspecified,
        vendor: "apple".to_string(),
        families: vec!["unix"],
        linker_flavor: LinkerFlavor::Darwin(Cc::Yes, Lld::No),
        function_sections: false,
        dynamic_linking: true,
        is_like_darwin: true,
        binary_format: BinaryFormat::MachO,
        default_dwarf_version: 4,
        frame_pointer,
        has_rpath: true,
        dll_prefix: "lib",
        dll_suffix: ".dylib",
        exe_suffix: "",
        has_thread_local: true,
        abi_return_struct_as_int: true,
        max_atomic_width: Some(128),
        cpu: cpu.to_string(),
        link_env: vec![("ZERO_AR_DATE", "1")],
        link_env_remove: vec![
            "IPHONEOS_DEPLOYMENT_TARGET",
            "TVOS_DEPLOYMENT_TARGET",
            "XROS_DEPLOYMENT_TARGET",
        ],
        ..if target_triple.is_empty() {
            TargetOptions::default()
        } else {
            TargetOptions::default()
        }
    }
}

fn linux_base() -> TargetOptions {
    TargetOptions {
        os: Os::Linux,
        dynamic_linking: true,
        families: vec!["unix"],
        has_rpath: true,
        position_independent_executables: true,
        has_thread_local: true,
        default_uwtable: true,
        ..TargetOptions::default()
    }
}

fn aarch64_apple_darwin() -> Target {
    let target_triple = "arm64-apple-macosx";
    Target {
        triple: "aarch64-apple-darwin".to_string(),
        target_triple: target_triple.to_string(),
        pointer_width: 64,
        data_layout: "e-m:o-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-n32:64-S128-Fn32"
            .to_string(),
        arch: Arch::AArch64,
        options: apple_macos_base(Arch::AArch64, target_triple, "apple-m1"),
    }
}

fn x86_64_apple_darwin() -> Target {
    let target_triple = "x86_64-apple-macosx";
    Target {
        triple: "x86_64-apple-darwin".to_string(),
        target_triple: target_triple.to_string(),
        pointer_width: 64,
        data_layout:
            "e-m:o-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
                .to_string(),
        arch: Arch::X86_64,
        options: apple_macos_base(Arch::X86_64, target_triple, "penryn"),
    }
}

fn x86_64_unknown_linux_gnu() -> Target {
    let mut opts = linux_base();
    opts.env = Env::Gnu;
    opts.static_position_independent_executables = true;
    opts.cpu = "x86-64".to_string();
    opts.max_atomic_width = Some(64);
    opts.pre_link_args.push((LinkerFlavor::Gnu(Cc::Yes, Lld::No), vec!["-m64".to_string()]));
    Target {
        triple: "x86_64-unknown-linux-gnu".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        pointer_width: 64,
        data_layout:
            "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
                .to_string(),
        arch: Arch::X86_64,
        options: opts,
    }
}

fn aarch64_unknown_linux_gnu() -> Target {
    let mut opts = linux_base();
    opts.env = Env::Gnu;
    opts.static_position_independent_executables = true;
    opts.features = "+v8a,+outline-atomics".to_string();
    opts.frame_pointer = FramePointer::NonLeaf;
    opts.max_atomic_width = Some(128);
    Target {
        triple: "aarch64-unknown-linux-gnu".to_string(),
        target_triple: "aarch64-unknown-linux-gnu".to_string(),
        pointer_width: 64,
        data_layout:
            "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
                .to_string(),
        arch: Arch::AArch64,
        options: opts,
    }
}

fn aarch64_linux_android() -> Target {
    let mut opts = linux_base();
    opts.os = Os::Android;
    opts.env = Env::Android;
    opts.is_like_android = true;
    opts.default_dwarf_version = 2;
    opts.tls_model = TlsModel::Emulated;
    opts.has_thread_local = false;
    opts.features = "+v8a,+neon,+outline-atomics".to_string();
    opts.frame_pointer = FramePointer::NonLeaf;
    opts.max_atomic_width = Some(128);
    Target {
        triple: "aarch64-linux-android".to_string(),
        target_triple: "aarch64-linux-android".to_string(),
        pointer_width: 64,
        data_layout:
            "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
                .to_string(),
        arch: Arch::AArch64,
        options: opts,
    }
}

fn x86_64_pc_windows_msvc() -> Target {
    let opts = TargetOptions {
        os: Os::Windows,
        env: Env::Msvc,
        vendor: "pc".to_string(),
        families: vec!["windows"],
        linker_flavor: LinkerFlavor::Msvc(Lld::No),
        binary_format: BinaryFormat::Coff,
        dynamic_linking: true,
        is_like_windows: true,
        is_like_msvc: true,
        dll_prefix: "",
        dll_suffix: ".dll",
        exe_suffix: ".exe",
        staticlib_prefix: "",
        staticlib_suffix: ".lib",
        requires_uwtable: true,
        has_thread_local: true,
        cpu: "x86-64".to_string(),
        features: "+cmpxchg16b,+sse3,+lahfsahf".to_string(),
        max_atomic_width: Some(128),
        ..TargetOptions::default()
    };
    Target {
        triple: "x86_64-pc-windows-msvc".to_string(),
        target_triple: "x86_64-pc-windows-msvc".to_string(),
        pointer_width: 64,
        data_layout:
            "e-m:w-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
                .to_string(),
        arch: Arch::X86_64,
        options: opts,
    }
}

fn aarch64_pc_windows_msvc() -> Target {
    let opts = TargetOptions {
        os: Os::Windows,
        env: Env::Msvc,
        vendor: "pc".to_string(),
        families: vec!["windows"],
        linker_flavor: LinkerFlavor::Msvc(Lld::No),
        binary_format: BinaryFormat::Coff,
        dynamic_linking: true,
        is_like_windows: true,
        is_like_msvc: true,
        dll_prefix: "",
        dll_suffix: ".dll",
        exe_suffix: ".exe",
        staticlib_prefix: "",
        staticlib_suffix: ".lib",
        requires_uwtable: true,
        has_thread_local: true,
        frame_pointer: FramePointer::NonLeaf,
        features: "+v8a,+neon".to_string(),
        max_atomic_width: Some(128),
        ..TargetOptions::default()
    };
    Target {
        triple: "aarch64-pc-windows-msvc".to_string(),
        target_triple: "aarch64-pc-windows-msvc".to_string(),
        pointer_width: 64,
        data_layout:
            "e-m:w-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
                .to_string(),
        arch: Arch::AArch64,
        options: opts,
    }
}
