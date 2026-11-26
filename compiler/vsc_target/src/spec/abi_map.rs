//! Mapping from `ExternAbi` to `CanonAbi` (`rustc_target::spec::abi_map`).

use vsc_abi::{ArmCall, CanonAbi, ExternAbi, X86Call};

use super::{Arch, Target};

#[derive(Clone, Debug)]
pub struct AbiMap {
    arch: Arch,
    is_windows: bool,
}

#[derive(Copy, Clone, Debug)]
pub enum AbiMapping {
    Direct(CanonAbi),
    Deprecated(CanonAbi),
    Invalid,
}

impl AbiMapping {
    #[inline]
    pub fn into_option(self) -> Option<CanonAbi> {
        match self {
            Self::Direct(abi) | Self::Deprecated(abi) => Some(abi),
            Self::Invalid => None,
        }
    }

    #[track_caller]
    #[inline]
    pub fn unwrap(self) -> CanonAbi {
        self.into_option().unwrap()
    }
}

impl AbiMap {
    pub fn from_target(target: &Target) -> Self {
        Self { arch: target.arch.clone(), is_windows: target.is_like_windows }
    }

    pub fn canonize_abi(&self, extern_abi: ExternAbi, _has_c_varargs: bool) -> AbiMapping {
        let canon = match extern_abi {
            ExternAbi::Rust => CanonAbi::Rust,
            ExternAbi::RustCall => CanonAbi::Rust,
            ExternAbi::RustCold => CanonAbi::RustCold,
            ExternAbi::RustInvalid => return AbiMapping::Invalid,
            ExternAbi::C { .. } | ExternAbi::System { .. } | ExternAbi::Intrinsic => CanonAbi::C,
            ExternAbi::EfiApi => match self.arch {
                Arch::X86_64 | Arch::AArch64 | Arch::X86 => CanonAbi::C,
                _ => return AbiMapping::Invalid,
            },
            ExternAbi::Aapcs { .. } => match self.arch {
                Arch::Arm => CanonAbi::Arm(ArmCall::Aapcs),
                _ => return AbiMapping::Invalid,
            },
            ExternAbi::SysV64 { .. } => match self.arch {
                Arch::X86_64 => CanonAbi::X86(X86Call::SysV64),
                _ => return AbiMapping::Invalid,
            },
            ExternAbi::Win64 { .. } => match self.arch {
                Arch::X86_64 => CanonAbi::X86(X86Call::Win64),
                _ => return AbiMapping::Invalid,
            },
            ExternAbi::Cdecl { .. } => CanonAbi::C,
            ExternAbi::Stdcall { .. } => {
                if self.arch == Arch::X86 && self.is_windows {
                    CanonAbi::X86(X86Call::Stdcall)
                } else {
                    return AbiMapping::Deprecated(CanonAbi::C);
                }
            }
            ExternAbi::Fastcall { .. } => {
                if self.arch == Arch::X86 {
                    CanonAbi::X86(X86Call::Fastcall)
                } else {
                    return AbiMapping::Deprecated(CanonAbi::C);
                }
            }
            ExternAbi::Vectorcall { .. } => {
                if matches!(self.arch, Arch::X86 | Arch::X86_64) {
                    CanonAbi::X86(X86Call::Vectorcall)
                } else {
                    return AbiMapping::Invalid;
                }
            }
        };
        AbiMapping::Direct(canon)
    }
}
