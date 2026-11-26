//! Apple deployment target and version helpers (`rustc_target::spec::base::apple`).

use std::fmt;
use std::str::FromStr;

use super::{Arch, Env, Os, Target};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OsVersion {
    pub major: u16,
    pub minor: u8,
    pub patch: u8,
}

impl OsVersion {
    pub const fn new(major: u16, minor: u8, patch: u8) -> Self {
        Self { major, minor, patch }
    }

    pub fn os_minimum_deployment_target(os: &Os) -> Self {
        let (major, minor, patch) = match os {
            Os::MacOs => (10, 12, 0),
            Os::IOs => (10, 0, 0),
            _ => (11, 0, 0),
        };
        Self { major, minor, patch }
    }

    pub fn minimum_deployment_target(target: &Target) -> Self {
        let (major, minor, patch) = match (&target.os, &target.arch, &target.env) {
            (Os::MacOs, Arch::AArch64, _) => (11, 0, 0),
            (Os::IOs, Arch::AArch64, Env::Unspecified) => (10, 0, 0),
            _ => return Self::os_minimum_deployment_target(&target.os),
        };
        Self { major, minor, patch }
    }
}

impl FromStr for OsVersion {
    type Err = std::num::ParseIntError;

    fn from_str(version: &str) -> Result<Self, Self::Err> {
        if let Some((major, minor)) = version.split_once('.') {
            let major = major.parse()?;
            if let Some((minor, patch)) = minor.split_once('.') {
                Ok(Self { major, minor: minor.parse()?, patch: patch.parse()? })
            } else {
                Ok(Self { major, minor: minor.parse()?, patch: 0 })
            }
        } else {
            Ok(Self { major: version.parse()?, minor: 0, patch: 0 })
        }
    }
}

impl fmt::Display for OsVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

pub fn deployment_target_env_var(os: &Os) -> &'static str {
    match os {
        Os::MacOs => "MACOSX_DEPLOYMENT_TARGET",
        Os::IOs => "IPHONEOS_DEPLOYMENT_TARGET",
        _ => "MACOSX_DEPLOYMENT_TARGET",
    }
}
