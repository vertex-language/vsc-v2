//! Configuration: the target and the `cfg` set (rustc's `config.rs` and
//! `config/cfg.rs`).

use vsc_data_structures::FxHashSet;
use vsc_span::{Symbol, sym};

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

    /// The four first targets, by Rust triple.
    pub fn from_triple(triple: &str) -> Option<TargetInfo> {
        let (arch, os) = match triple {
            "aarch64-apple-darwin" => ("aarch64", "macos"),
            "x86_64-apple-darwin" => ("x86_64", "macos"),
            "aarch64-linux-android" => ("aarch64", "android"),
            "x86_64-unknown-linux-gnu" => ("x86_64", "linux"),
            "aarch64-unknown-linux-gnu" => ("aarch64", "linux"),
            "x86_64-pc-windows-msvc" => ("x86_64", "windows"),
            _ => return None,
        };
        TargetInfo::from_parts(arch, os)
    }

    fn from_parts(arch: &str, os: &str) -> Option<TargetInfo> {
        let (triple, arch, os, family, env, vendor) = match (arch, os) {
            ("aarch64", "macos") => ("aarch64-apple-darwin", "aarch64", "macos", "unix", "", "apple"),
            ("x86_64", "macos") => ("x86_64-apple-darwin", "x86_64", "macos", "unix", "", "apple"),
            ("aarch64", "android") => ("aarch64-linux-android", "aarch64", "android", "unix", "", "unknown"),
            ("x86_64", "linux") => ("x86_64-unknown-linux-gnu", "x86_64", "linux", "unix", "gnu", "unknown"),
            ("aarch64", "linux") => ("aarch64-unknown-linux-gnu", "aarch64", "linux", "unix", "gnu", "unknown"),
            ("x86_64", "windows") => ("x86_64-pc-windows-msvc", "x86_64", "windows", "windows", "msvc", "pc"),
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
    let mut cfg = Cfg::default();
    cfg.insert("target_arch", Some(target.arch));
    cfg.insert("target_os", Some(target.os));
    cfg.insert("target_family", Some(target.family));
    cfg.insert(target.family, None);
    cfg.insert("target_env", Some(target.env));
    cfg.insert("target_vendor", Some(target.vendor));
    cfg.insert("target_pointer_width", Some(&target.pointer_width.to_string()));
    cfg.insert("target_endian", Some(target.endian));
    cfg.insert("panic", Some("unwind"));
    if debug_assertions {
        cfg.insert("debug_assertions", None);
    }
    if test {
        cfg.insert(sym::test.as_str(), None);
    }
    cfg
}
