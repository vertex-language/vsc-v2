//! The dependency allowlist (rustc's `src/tools/tidy/src/deps.rs`).
//!
//! The foundation, front end and middle use Rust's std only. Outside crates
//! are allowed only at the edges, where vsc meets LLVM or the operating
//! system, and each one is listed here with the crates that may use it and
//! why.

use std::path::Path;

/// (crate, version prefix, the vsc crates that may depend on it, why)
const PERMITTED: &[(&str, &str, &[&str], &str)] = &[
    // ("llvm-sys", "231", &["vsc_llvm"], "the LLVM C API; vsc_codegen_llvm builds on it"),
];

pub fn check(root: &Path, bad: &mut Vec<String>) {
    let lock = root.join("Cargo.lock");
    let Ok(text) = std::fs::read_to_string(&lock) else {
        bad.push("Cargo.lock is missing; run `cargo build` first".to_string());
        return;
    };
    for package in text.split("[[package]]").skip(1) {
        let field = |key: &str| {
            package
                .lines()
                .find_map(|l| l.strip_prefix(&format!("{key} = \"")))
                .map(|v| v.trim_end_matches('"').to_string())
        };
        let Some(name) = field("name") else { continue };
        let Some(source) = field("source") else {
            // No source: a path dependency inside the workspace.
            continue;
        };
        let version = field("version").unwrap_or_default();
        match PERMITTED.iter().find(|(n, ..)| *n == name) {
            Some((_, prefix, _, _)) if version.starts_with(prefix) => {}
            Some((_, prefix, _, _)) => bad.push(format!(
                "outside crate `{name}` {version} is allowed only at version {prefix}* (src/tools/tidy/src/deps.rs)"
            )),
            None => bad.push(format!(
                "outside crate `{name}` {version} from {source} isn't on the allowlist in src/tools/tidy/src/deps.rs"
            )),
        }
    }
}
