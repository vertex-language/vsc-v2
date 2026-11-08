//! rustc's command line, as far as vsc implements it.

use std::path::PathBuf;

use vsc_errors::ColorConfig;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrorFormat {
    Human,
    Short,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unpretty {
    /// `-Z unpretty=ast`: the parsed file printed as Vertex.
    Ast,
    /// `-Z unpretty=ast-tree`: the AST's debug form.
    AstTree,
    /// `-Z unpretty=hir`: the crate after lowering, printed in Rust's syntax.
    Hir,
    /// `-Z unpretty=hir-tree`: the HIR's debug form.
    HirTree,
    /// `-Z unpretty=thir-tree`: every body's THIR, after type checking.
    ThirTree,
}

impl Unpretty {
    /// Printed straight after parsing, before name resolution.
    pub fn is_ast(self) -> bool {
        matches!(self, Unpretty::Ast | Unpretty::AstTree)
    }
}

#[derive(Debug)]
pub struct Options {
    pub input: PathBuf,
    pub crate_name: Option<String>,
    pub error_format: ErrorFormat,
    pub color: ColorConfig,
    pub parse_only: bool,
    pub unpretty: Option<Unpretty>,
    /// `-Z dump-resolutions`
    pub dump_resolutions: bool,
    /// `-Z dump-signatures`
    pub dump_signatures: bool,
    /// `-Z dump-types`
    pub dump_types: bool,
    /// `-Z dump-mir`
    pub dump_mir: bool,
    /// `-Z validate-mir`
    pub validate_mir: bool,
    /// `--emit=metadata`: check only, as `cargo check` asks.
    pub check_only: bool,
    /// `--cfg` arguments.
    pub cfgs: Vec<String>,
    /// `--test`
    pub test: bool,
    pub target: Option<String>,
}

pub enum Parsed {
    Run(Options),
    /// Print this and exit successfully (`--help`, `--version`).
    Done(String),
}

const USAGE: &str = "\
Usage: vsc [OPTIONS] INPUT

INPUT is the crate root: its folder's .vs files form the root package.

Options:
    -h, --help              Print this help
    -V, --version           Print the version
    -vV                     Print verbose version information
        --crate-name NAME   Name of the crate
        --crate-type TYPE   bin, lib, rlib, staticlib, dylib, cdylib
        --edition 2024      Vertex has one edition; 2024 is accepted
        --error-format FMT  human or short
        --color WHEN        auto, always or never
        --cfg SPEC          Turn on a configuration option: name or name=\"value\"
        --test              Build the test harness (sets cfg test)
        --target TRIPLE     Target triple (default: the host)
        --emit KINDS        metadata: check only; link: the default
    -Z parse-only           Stop after parsing
    -Z dump-resolutions     Print what every path resolved to
    -Z dump-signatures      Print every item's type or signature
    -Z dump-types           Print the types inferred in every body
    -Z dump-mir             Print every body's MIR
    -Z validate-mir         Check every body's MIR is well formed
    -Z unpretty=ast         Print the parsed source as Vertex
    -Z unpretty=ast-tree    Print the parsed AST
    -Z unpretty=hir         Print the crate after lowering to HIR
    -Z unpretty=hir-tree    Print the HIR
    -Z unpretty=thir-tree   Print every body's THIR
";

pub fn parse(args: &[String]) -> Result<Parsed, String> {
    let mut input = None;
    let mut crate_name = None;
    let mut error_format = ErrorFormat::Human;
    let mut color = ColorConfig::Auto;
    let mut parse_only = false;
    let mut unpretty = None;
    let mut dump_resolutions = false;
    let mut dump_signatures = false;
    let mut dump_types = false;
    let mut dump_mir = false;
    let mut validate_mir = false;
    let mut check_only = false;
    let mut cfgs = Vec::new();
    let mut test = false;
    let mut target = None;

    let mut it = args.iter().peekable();
    // `--flag value` or `--flag=value`.
    let value = |flag: &str, arg: &str, it: &mut std::iter::Peekable<std::slice::Iter<'_, String>>| -> Result<String, String> {
        if let Some(v) = arg.strip_prefix(flag).and_then(|r| r.strip_prefix('=')) {
            return Ok(v.to_string());
        }
        it.next().cloned().ok_or_else(|| format!("`{flag}` needs a value"))
    };

    while let Some(arg) = it.next() {
        let a = arg.as_str();
        match a {
            "-h" | "--help" => return Ok(Parsed::Done(USAGE.to_string())),
            "-V" | "--version" => return Ok(Parsed::Done(format!("vsc {}\n", crate::VERSION))),
            "-vV" => {
                return Ok(Parsed::Done(format!(
                    "vsc {v}\nbinary: vsc\nrelease: {v}\nhost: {host}\n",
                    v = crate::VERSION,
                    host = host_triple()
                )));
            }
            _ if a == "--crate-name" || a.starts_with("--crate-name=") => crate_name = Some(value("--crate-name", a, &mut it)?),
            _ if a == "--crate-type" || a.starts_with("--crate-type=") => {
                let t = value("--crate-type", a, &mut it)?;
                if !matches!(t.as_str(), "bin" | "lib" | "rlib" | "staticlib" | "dylib" | "cdylib") {
                    return Err(format!("unknown crate type: `{t}`"));
                }
            }
            _ if a == "--edition" || a.starts_with("--edition=") => {
                let e = value("--edition", a, &mut it)?;
                if e != "2024" {
                    return Err(format!("Vertex has one edition, 2024; `--edition {e}` isn't supported"));
                }
            }
            _ if a == "--error-format" || a.starts_with("--error-format=") => {
                error_format = match value("--error-format", a, &mut it)?.as_str() {
                    "human" => ErrorFormat::Human,
                    "short" => ErrorFormat::Short,
                    other => return Err(format!("unknown error format: `{other}`")),
                };
            }
            _ if a == "--color" || a.starts_with("--color=") => {
                color = match value("--color", a, &mut it)?.as_str() {
                    "auto" => ColorConfig::Auto,
                    "always" => ColorConfig::Always,
                    "never" => ColorConfig::Never,
                    other => return Err(format!("unknown color setting: `{other}`")),
                };
            }
            "--test" => test = true,
            _ if a == "--cfg" || a.starts_with("--cfg=") => cfgs.push(value("--cfg", a, &mut it)?),
            _ if a == "--target" || a.starts_with("--target=") => target = Some(value("--target", a, &mut it)?),
            _ if a == "--emit" || a.starts_with("--emit=") => {
                for kind in value("--emit", a, &mut it)?.split(',') {
                    match kind.split('=').next().unwrap() {
                        "metadata" => check_only = true,
                        "link" | "dep-info" => check_only = false,
                        other => return Err(format!("unknown emit kind: `{other}`")),
                    }
                }
            }
            _ if a == "-Z" || a.starts_with("-Z") => {
                let z = if a == "-Z" { it.next().cloned().ok_or("`-Z` needs an option")? } else { a[2..].to_string() };
                match z.as_str() {
                    "parse-only" => parse_only = true,
                    "dump-resolutions" => dump_resolutions = true,
                    "dump-signatures" => dump_signatures = true,
                    "dump-types" => dump_types = true,
                    "dump-mir" => dump_mir = true,
                    "validate-mir" => validate_mir = true,
                    "unpretty=ast" => unpretty = Some(Unpretty::Ast),
                    "unpretty=ast-tree" => unpretty = Some(Unpretty::AstTree),
                    "unpretty=hir" => unpretty = Some(Unpretty::Hir),
                    "unpretty=hir-tree" => unpretty = Some(Unpretty::HirTree),
                    "unpretty=thir-tree" => unpretty = Some(Unpretty::ThirTree),
                    other => return Err(format!("unknown `-Z` option: `{other}`")),
                }
            }
            _ if a.starts_with('-') && a != "-" => return Err(format!("unknown option: `{a}`")),
            _ => {
                if input.replace(PathBuf::from(a)).is_some() {
                    return Err("more than one input file given".to_string());
                }
            }
        }
    }

    let Some(input) = input else {
        return Ok(Parsed::Done(USAGE.to_string()));
    };
    Ok(Parsed::Run(Options {
        input,
        crate_name,
        error_format,
        color,
        parse_only,
        unpretty,
        dump_resolutions,
        dump_signatures,
        dump_types,
        dump_mir,
        validate_mir,
        check_only,
        cfgs,
        test,
        target,
    }))
}

fn host_triple() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        ("aarch64", "android") => "aarch64-linux-android",
        _ => "unknown",
    }
}
