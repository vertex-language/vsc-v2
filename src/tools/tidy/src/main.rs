//! Repository checks (rustc's `src/tools/tidy`).
//!
//! - deps: every package in `Cargo.lock` is a workspace member, or an outside
//!   crate on the allowlist in `deps.rs`.
//! - style: source files end with a line feed, have no `\r`, no tabs and no
//!   trailing whitespace.
//!
//! Usage: `tidy ROOT`

mod deps;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".to_string()));
    let mut bad = Vec::new();
    deps::check(&root, &mut bad);
    for dir in ["compiler", "src", "tests"] {
        style(&root.join(dir), &mut bad);
    }
    if bad.is_empty() {
        println!("tidy: ok");
        ExitCode::SUCCESS
    } else {
        for b in &bad {
            eprintln!("tidy error: {b}");
        }
        eprintln!("tidy: {} problem(s)", bad.len());
        ExitCode::FAILURE
    }
}

fn style(dir: &Path, bad: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            style(&path, bad);
            continue;
        }
        let checked = path.extension().is_some_and(|e| e == "rs" || e == "vs" || e == "toml");
        if !checked {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let shown = path.display();
        if !text.is_empty() && !text.ends_with('\n') {
            bad.push(format!("{shown}: missing a line feed at the end of the file"));
        }
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            if line.contains('\r') {
                bad.push(format!("{shown}:{n}: CR character"));
            }
            if line.contains('\t') {
                bad.push(format!("{shown}:{n}: tab character"));
            }
            if line.ends_with(' ') {
                bad.push(format!("{shown}:{n}: trailing whitespace"));
            }
        }
    }
}
