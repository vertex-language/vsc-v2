//! External calling convention identifiers (rustc's `rustc_abi::extern_abi`).

use std::fmt;

use crate::AbiFromStrErr;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum CVariadicStatus {
    NotSupported,
    Supported,
}

/// ABI expected within `extern "{abi}"`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ExternAbi {
    C { unwind: bool },
    System { unwind: bool },
    Rust,
    RustCall,
    RustCold,
    RustInvalid,
    Intrinsic,
    EfiApi,
    Aapcs { unwind: bool },
    Cdecl { unwind: bool },
    Stdcall { unwind: bool },
    Fastcall { unwind: bool },
    Vectorcall { unwind: bool },
    SysV64 { unwind: bool },
    Win64 { unwind: bool },
}

#[derive(Copy, Clone, Debug)]
pub struct AbiData {
    pub abi: ExternAbi,
    pub name: &'static str,
    pub generic_c_variadic: bool,
}

const ABI_DATAS: &[AbiData] = &[
    AbiData { abi: ExternAbi::Rust, name: "Rust", generic_c_variadic: false },
    AbiData { abi: ExternAbi::C { unwind: false }, name: "C", generic_c_variadic: true },
    AbiData { abi: ExternAbi::C { unwind: true }, name: "C-unwind", generic_c_variadic: true },
    AbiData { abi: ExternAbi::Cdecl { unwind: false }, name: "cdecl", generic_c_variadic: true },
    AbiData { abi: ExternAbi::Cdecl { unwind: true }, name: "cdecl-unwind", generic_c_variadic: true },
    AbiData { abi: ExternAbi::Stdcall { unwind: false }, name: "stdcall", generic_c_variadic: false },
    AbiData {
        abi: ExternAbi::Stdcall { unwind: true },
        name: "stdcall-unwind",
        generic_c_variadic: false,
    },
    AbiData {
        abi: ExternAbi::Fastcall { unwind: false },
        name: "fastcall",
        generic_c_variadic: false,
    },
    AbiData {
        abi: ExternAbi::Fastcall { unwind: true },
        name: "fastcall-unwind",
        generic_c_variadic: false,
    },
    AbiData {
        abi: ExternAbi::Vectorcall { unwind: false },
        name: "vectorcall",
        generic_c_variadic: false,
    },
    AbiData {
        abi: ExternAbi::Vectorcall { unwind: true },
        name: "vectorcall-unwind",
        generic_c_variadic: false,
    },
    AbiData { abi: ExternAbi::Aapcs { unwind: false }, name: "aapcs", generic_c_variadic: false },
    AbiData {
        abi: ExternAbi::Aapcs { unwind: true },
        name: "aapcs-unwind",
        generic_c_variadic: false,
    },
    AbiData { abi: ExternAbi::Win64 { unwind: false }, name: "win64", generic_c_variadic: true },
    AbiData { abi: ExternAbi::Win64 { unwind: true }, name: "win64-unwind", generic_c_variadic: true },
    AbiData { abi: ExternAbi::SysV64 { unwind: false }, name: "sysv64", generic_c_variadic: true },
    AbiData {
        abi: ExternAbi::SysV64 { unwind: true },
        name: "sysv64-unwind",
        generic_c_variadic: true,
    },
    AbiData { abi: ExternAbi::System { unwind: false }, name: "system", generic_c_variadic: true },
    AbiData {
        abi: ExternAbi::System { unwind: true },
        name: "system-unwind",
        generic_c_variadic: true,
    },
    AbiData { abi: ExternAbi::RustCall, name: "rust-call", generic_c_variadic: false },
    AbiData { abi: ExternAbi::RustCold, name: "rust-cold", generic_c_variadic: false },
    AbiData { abi: ExternAbi::RustInvalid, name: "rust-invalid", generic_c_variadic: false },
    AbiData { abi: ExternAbi::EfiApi, name: "efiapi", generic_c_variadic: true },
    AbiData { abi: ExternAbi::Intrinsic, name: "intrinsic", generic_c_variadic: false },
];

pub fn all_names() -> Vec<&'static str> {
    ABI_DATAS.iter().map(|d| d.name).collect()
}

impl ExternAbi {
    pub fn name(self) -> &'static str {
        ABI_DATAS.iter().find(|d| d.abi == self).map_or("unknown", |d| d.name)
    }

    pub fn from_name(s: &str) -> Result<Self, AbiFromStrErr> {
        ABI_DATAS.iter().find(|d| d.name == s).map(|d| d.abi).ok_or(AbiFromStrErr::Unknown)
    }

    pub fn is_rustic_abi(self) -> bool {
        matches!(self, ExternAbi::Rust | ExternAbi::RustCall | ExternAbi::RustCold)
    }

    pub fn supports_c_variadic(self) -> CVariadicStatus {
        let supported = ABI_DATAS.iter().find(|d| d.abi == self).is_some_and(|d| d.generic_c_variadic);
        if supported { CVariadicStatus::Supported } else { CVariadicStatus::NotSupported }
    }
}

impl std::str::FromStr for ExternAbi {
    type Err = AbiFromStrErr;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_name(s)
    }
}

impl fmt::Display for ExternAbi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
