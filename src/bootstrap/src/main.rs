//! The build system behind `./x` (rustc's `src/bootstrap`).
//!
//! There are two stages, because the compiler isn't self-hosted: stage0 is
//! the pinned Rust toolchain, and stage1 is `vsc` built by it. `library/`
//! will be built by cargo with stage1 `vsc` once there is a back end.
//!
//! ```text
//! ./x build                 build the compiler and tools
//! ./x test                  tidy, unit tests, then tests/ui and the pairs
//! ./x test tests/ui [FILTER] [--bless]
//! ./x test tests/ui pairs --bless   rerun tests/rust with rustc
//! ./x check                 cargo check
//! ./x fmt                   rustfmt the workspace
//! ```

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root();
    let Some(cmd) = args.first() else {
        eprintln!("usage: ./x build | test [SUITE] [FILTER] [--bless] | check | fmt");
        return ExitCode::from(2);
    };
    let rest = &args[1..];
    let ok = match cmd.as_str() {
        "build" | "b" => cargo(&root, &["build"]),
        "check" | "c" => cargo(&root, &["check", "--workspace", "--all-targets"]),
        "fmt" => cargo(&root, &["fmt", "--all"]),
        "tidy" => tidy(&root),
        "test" | "t" => test(&root, rest),
        other => {
            eprintln!("unknown command: `{other}`");
            false
        }
    };
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn repo_root() -> PathBuf {
    // src/bootstrap → the repository root.
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

fn cargo(root: &Path, args: &[&str]) -> bool {
    println!("$ cargo {}", args.join(" "));
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string()))
        .args(args)
        .current_dir(root)
        .status()
        .is_ok_and(|s| s.success())
}

fn tidy(root: &Path) -> bool {
    cargo(root, &["run", "-q", "-p", "tidy", "--", root.to_str().unwrap()])
}

fn test(root: &Path, args: &[String]) -> bool {
    let bless = args.iter().any(|a| a == "--bless");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--bless").collect();
    let only_ui = rest.first().is_some_and(|a| a.trim_end_matches('/') == "tests/ui");
    let filters: Vec<String> = rest.iter().skip(usize::from(only_ui)).map(|s| s.to_string()).collect();

    if !cargo(root, &["build", "-q", "-p", "vsc", "-p", "compiletest"]) {
        return false;
    }
    if !only_ui && !(tidy(root) && cargo(root, &["test", "-q", "--workspace"])) {
        return false;
    }
    let target = root.join("target/debug");
    let mut cmd = Command::new(target.join("compiletest"));
    cmd.arg("--vsc").arg(target.join("vsc")).arg("--root").arg(root);
    if bless {
        cmd.arg("--bless");
    }
    cmd.args(&filters);
    println!("$ compiletest tests/ui");
    cmd.status().is_ok_and(|s| s.success())
}
