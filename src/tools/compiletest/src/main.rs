//! Runs the test suites in `tests/` (rustc's `src/tools/compiletest`).
//!
//! Only the `ui` suite exists so far. A ui test is a `.vs` file compiled with
//! `vsc`; its stderr must match the `.stderr` file next to it, its stdout
//! the `.stdout` file (for `-Z unpretty` tests), and every `//~ ERROR
//! message` annotation must match a diagnostic on that line.
//!
//! Headers, as in rustc:
//!
//! - `//@ check-pass`: must compile without errors.
//! - `//@ compile-flags: …`: extra flags for `vsc`.
//! - `//@ add-core-stubs`: put `tests/auxiliary/minicore.vs` (the lang
//!   items, as stubs) in the test's package.
//!
//! Annotations: `//~ ERROR text` (this line), `//~^ ERROR text` (one line up,
//! one more per `^`), `//~| ERROR text` (same line as the previous one).
//! The kind is `ERROR`, `WARN`, `NOTE` or `HELP`.
//!
//! Usage: `compiletest --vsc PATH --root PATH [--bless] [FILTER…]`

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

struct Config {
    vsc: PathBuf,
    root: PathBuf,
    bless: bool,
    filters: Vec<String>,
}

fn main() -> ExitCode {
    let config = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("compiletest: {e}");
            return ExitCode::from(2);
        }
    };
    let suite = config.root.join("tests/ui");
    let mut tests = Vec::new();
    collect(&suite, &mut tests);
    tests.sort();
    tests.retain(|t| {
        let rel = t.strip_prefix(&config.root).unwrap_or(t).to_string_lossy().into_owned();
        config.filters.is_empty() || config.filters.iter().any(|f| rel.contains(f.as_str()))
    });

    let mut failed = Vec::new();
    for test in &tests {
        let rel = test.strip_prefix(&config.root).unwrap_or(test);
        match run_ui_test(&config, test) {
            Ok(()) => println!("test {} ... ok", rel.display()),
            Err(why) => {
                println!("test {} ... FAILED", rel.display());
                failed.push((rel.to_path_buf(), why));
            }
        }
    }
    for (path, why) in &failed {
        println!("\n---- {} ----\n{why}", path.display());
    }
    println!(
        "\ntest result: {}. {} passed; {} failed",
        if failed.is_empty() { "ok" } else { "FAILED" },
        tests.len() - failed.len(),
        failed.len()
    );
    if failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

fn parse_args() -> Result<Config, String> {
    let mut vsc = None;
    let mut root = None;
    let mut bless = false;
    let mut filters = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--vsc" => vsc = Some(PathBuf::from(args.next().ok_or("--vsc needs a path")?)),
            "--root" => root = Some(PathBuf::from(args.next().ok_or("--root needs a path")?)),
            "--bless" => bless = true,
            _ => filters.push(arg),
        }
    }
    Ok(Config {
        vsc: vsc.ok_or("missing --vsc")?,
        root: root.ok_or("missing --root")?,
        bless,
        filters,
    })
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "vs") {
            out.push(path);
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
struct Annotation {
    line: usize,
    kind: String,
    msg: String,
}

fn run_ui_test(config: &Config, test: &Path) -> Result<(), String> {
    let src = std::fs::read_to_string(test).map_err(|e| e.to_string())?;
    let check_pass = src.lines().any(|l| l.trim() == "//@ check-pass");
    let flags: Vec<String> = src
        .lines()
        .filter_map(|l| l.trim().strip_prefix("//@ compile-flags:"))
        .flat_map(|f| f.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect();
    let expected = parse_annotations(&src)?;

    // Each test is its own package: run vsc on just this file by copying it
    // into a private folder, so sibling tests aren't read as one package.
    let rel = test.strip_prefix(&config.root).unwrap_or(test);
    let work = std::env::temp_dir().join(format!("vsc-ui-{}", std::process::id())).join(rel.with_extension(""));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let file_name = test.file_name().unwrap();
    let copy = work.join(file_name);
    std::fs::write(&copy, &src).map_err(|e| e.to_string())?;
    if src.lines().any(|l| l.trim() == "//@ add-core-stubs") {
        std::fs::copy(config.root.join("tests/auxiliary/minicore.vs"), work.join("minicore.vs")).map_err(|e| e.to_string())?;
    }

    let output = Command::new(&config.vsc)
        // Every test also checks that its MIR is well formed.
        .args(["--color=never", "--emit=metadata", "--crate-name=test", "-Z", "validate-mir"])
        .args(&flags)
        .arg(file_name)
        .current_dir(&work)
        .output()
        .map_err(|e| format!("couldn't run vsc: {e}"))?;
    let _ = std::fs::remove_dir_all(&work);

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let mut problems = Vec::new();

    match output.status.code() {
        Some(0) if !check_pass && !expected.iter().all(|a| a.kind != "ERROR") => {
            problems.push("expected compilation to fail, but it succeeded".to_string())
        }
        Some(0) => {}
        Some(1) if check_pass => problems.push("expected compilation to succeed, but it failed".to_string()),
        Some(1) => {}
        other => problems.push(format!("vsc exited with {other:?} (crash?)")),
    }

    // Annotations against actual diagnostics.
    let actual = parse_diagnostics(&stderr, &file_name.to_string_lossy());
    let mut unmatched_actual: Vec<Annotation> = actual.clone();
    for want in &expected {
        match unmatched_actual.iter().position(|got| got.line == want.line && got.kind == want.kind && got.msg.contains(&want.msg)) {
            Some(i) => {
                unmatched_actual.remove(i);
            }
            None => problems.push(format!("expected {} on line {}: {}", want.kind, want.line, want.msg)),
        }
    }
    for got in &unmatched_actual {
        if got.kind == "ERROR" {
            problems.push(format!("unexpected ERROR on line {}: {}", got.line, got.msg));
        }
    }

    // The .stderr and .stdout files.
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    for (kind, text) in [("stderr", &stderr), ("stdout", &stdout)] {
        let path = test.with_extension(kind);
        let normalized = text.replace(&*file_name.to_string_lossy(), &format!("$DIR/{}", file_name.to_string_lossy()));
        if config.bless {
            if normalized.is_empty() {
                let _ = std::fs::remove_file(&path);
            } else {
                std::fs::write(&path, &normalized).map_err(|e| e.to_string())?;
            }
        } else {
            let want = std::fs::read_to_string(&path).unwrap_or_default();
            if want != normalized {
                problems.push(format!(
                    "{kind} doesn't match {} (run with --bless to update)\n--- expected\n{want}\n--- actual\n{normalized}",
                    path.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }

    if problems.is_empty() { Ok(()) } else { Err(problems.join("\n")) }
}

fn parse_annotations(src: &str) -> Result<Vec<Annotation>, String> {
    let mut out: Vec<Annotation> = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let Some(pos) = line.find("//~") else { continue };
        let rest = &line[pos + 3..];
        let (target, rest) = if let Some(r) = rest.strip_prefix('|') {
            let prev = out.last().ok_or_else(|| format!("line {}: `//~|` with no annotation before it", i + 1))?;
            (prev.line, r)
        } else {
            let carets = rest.chars().take_while(|&c| c == '^').count();
            let line_no = (i + 1).checked_sub(carets).filter(|&n| n > 0).ok_or("`//~^` points above the file")?;
            (line_no, &rest[carets..])
        };
        let rest = rest.trim_start();
        let (kind, msg) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let kind = kind.trim_end_matches(':').to_string();
        if !["ERROR", "WARN", "NOTE", "HELP"].contains(&kind.as_str()) {
            return Err(format!("line {}: unknown annotation kind `{kind}`", i + 1));
        }
        out.push(Annotation { line: target, kind, msg: msg.trim().to_string() });
    }
    Ok(out)
}

/// The top-level diagnostics in human-format stderr, with their lines.
fn parse_diagnostics(stderr: &str, file: &str) -> Vec<Annotation> {
    let mut out = Vec::new();
    let lines: Vec<&str> = stderr.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let (kind, rest) = if let Some(r) = line.strip_prefix("error") {
            ("ERROR", r)
        } else if let Some(r) = line.strip_prefix("warning") {
            ("WARN", r)
        } else {
            continue;
        };
        // `error[E0382]: message` or `error: message`.
        let rest = if rest.starts_with('[') { rest.split_once(']').map(|x| x.1).unwrap_or(rest) } else { rest };
        let Some(msg) = rest.strip_prefix(": ") else { continue };
        let Some(next) = lines.get(i + 1) else { continue };
        let Some(loc) = next.trim_start().strip_prefix("--> ") else { continue };
        let Some(after) = loc.strip_prefix(file).and_then(|r| r.strip_prefix(':')) else { continue };
        let Some(line_no) = after.split(':').next().and_then(|n| n.parse().ok()) else { continue };
        out.push(Annotation { line: line_no, kind: kind.to_string(), msg: msg.to_string() });
    }
    out
}
