//! The `vsc` command line (rustc's `rustc_driver_impl`).
//!
//! `vsc` takes rustc's command line, so cargo can drive it (`build.rustc =
//! "vsc"`). The pipeline so far: load the crate's packages and parse them,
//! apply `@cfg` and expand built-ins, resolve names, lower to HIR, check
//! item signatures, type check bodies, and stop.

mod args;

use std::process::ExitCode;
use std::sync::Arc;

use vsc_errors::{ColorConfig, DiagCtxt, FatalErrorMarker, HumanEmitter};
use vsc_session::ParseSess;
use vsc_session::config::{TargetInfo, default_configuration};
use vsc_span::SourceMap;

pub use args::{ErrorFormat, Options, Unpretty};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs `vsc` with the process's arguments and returns its exit code: 0 on
/// success, 1 after errors, 101 after an internal compiler error.
pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = std::panic::catch_unwind(|| run_compiler(&args));
    match result {
        Ok(code) => code,
        Err(payload) if payload.is::<FatalErrorMarker>() => ExitCode::from(1),
        Err(_) => {
            eprintln!("\nnote: this is a bug in vsc; please report it at https://github.com/vertex-language/vsc-v2/issues");
            ExitCode::from(101)
        }
    }
}

pub fn run_compiler(args: &[String]) -> ExitCode {
    let opts = match args::parse(args) {
        Ok(args::Parsed::Run(opts)) => opts,
        Ok(args::Parsed::Done(text)) => {
            print!("{text}");
            return ExitCode::SUCCESS;
        }
        Err(msg) => {
            let dcx = DiagCtxt::with_stderr(Arc::new(SourceMap::new()), ColorConfig::Auto);
            dcx.struct_err(msg).with_note("run `vsc --help` for the options").emit();
            return ExitCode::from(1);
        }
    };

    let sm = Arc::new(SourceMap::new());
    let emitter = HumanEmitter::stderr(opts.color).with_source_map(sm.clone()).short(opts.error_format == ErrorFormat::Short);
    let psess = ParseSess::new(sm, DiagCtxt::new(Box::new(emitter)));

    let target = match &opts.target {
        None => TargetInfo::host(),
        Some(triple) => match TargetInfo::from_triple(triple) {
            Some(t) => t,
            None => {
                psess.dcx.err(format!("unknown target `{triple}`"));
                return ExitCode::from(1);
            }
        },
    };
    let mut cfg = default_configuration(&target, true, opts.test);
    for arg in &opts.cfgs {
        if let Err(msg) = cfg.insert_from_arg(arg) {
            psess.dcx.err(msg);
            return ExitCode::from(1);
        }
    }

    let ecfg = vsc_expand::ExpansionConfig { crate_name: opts.crate_name.as_deref(), cfg: &cfg };
    let krate = match vsc_expand::expand_crate(&psess, &opts.input, &ecfg) {
        Ok(k) => k,
        Err(err) => {
            psess.dcx.err(format!("couldn't read `{}`: {err}", opts.input.display()));
            return ExitCode::from(1);
        }
    };

    if let Some(unpretty) = opts.unpretty.filter(|u| u.is_ast()) {
        print_crate(&psess, &krate, unpretty);
    }

    // Set when `-Z run-mir` ran the program and it panicked.
    let mut program_panicked = false;
    let stop_after_parse = opts.parse_only || opts.unpretty.is_some_and(Unpretty::is_ast);
    if !stop_after_parse {
        let outputs = vsc_resolve::resolve_crate(&psess, &krate);
        if opts.dump_resolutions {
            print!("{}", vsc_resolve::dump::dump_resolutions(&krate, &outputs, &psess.source_map));
        }
        let hir = vsc_ast_lowering::lower_to_hir(&psess, &krate, &outputs);
        match opts.unpretty {
            Some(Unpretty::Hir) => println!("{}", vsc_hir_pretty::crate_to_string(&hir)),
            Some(Unpretty::HirTree) => println!("{hir:#?}"),
            // Analysis starts from a crate without errors, so its errors
            // don't repeat earlier ones.
            None | Some(Unpretty::ThirTree) if psess.dcx.has_errors().is_none() => {
                program_panicked = analysis(&psess, &target, &hir, &outputs, &opts);
            }
            None | Some(Unpretty::Ast | Unpretty::AstTree | Unpretty::ThirTree) => {}
        }
    }

    if psess.dcx.has_errors().is_some() {
        psess.dcx.print_error_count();
        return ExitCode::from(1);
    }
    // A program that panicked exits as Rust's do.
    if program_panicked {
        return ExitCode::from(101);
    }
    if !stop_after_parse && !opts.check_only && !opts.dump_resolutions && !opts.dump_signatures && !opts.dump_types && !opts.dump_mir && !opts.run_mir && opts.unpretty.is_none() {
        psess
            .dcx
            .struct_note("vsc-v2 stops after building MIR for now")
            .with_note("borrow checking and code generation aren't written yet")
            .emit();
    }
    ExitCode::SUCCESS
}

/// Signatures and the checks on them (rustc's `analysis` query).
/// Returns whether `-Z run-mir` ran the program and it panicked.
fn analysis(psess: &ParseSess, target: &TargetInfo, hir: &vsc_hir::Crate, resolved: &vsc_resolve::ResolverOutputs, opts: &Options) -> bool {
    use vsc_middle::ty::{CtxtInputs, DefInfo, TyCtxt};
    let defs = vsc_index::IndexVec::from_raw(
        resolved
            .defs
            .iter()
            .map(|d| DefInfo { kind: d.kind, parent: d.parent, name: d.name.unwrap_or(vsc_span::kw::Empty), span: d.span })
            .collect(),
    );
    let mut providers = vsc_middle::query::Providers::default();
    vsc_passes::provide(&mut providers);
    vsc_hir_analysis::provide(&mut providers);
    vsc_hir_typeck::provide(&mut providers);
    vsc_mir_build::provide(&mut providers);
    let arena = vsc_middle::arena::Arena::default();
    let gcx = std::cell::OnceCell::new();
    let tcx = TyCtxt::create(
        &arena,
        &gcx,
        CtxtInputs { sess: psess, target, hir, defs, crate_name: resolved.crate_name, providers },
    );
    tcx.lang_items();
    vsc_passes::loops::check_crate(tcx);
    vsc_hir_analysis::check_crate(tcx);
    if opts.dump_signatures {
        print!("{}", vsc_hir_analysis::dump::dump_signatures(tcx));
    }
    // Bodies are checked against signatures without errors, as in rustc.
    if psess.dcx.has_errors().is_none() {
        vsc_hir_typeck::check_crate(tcx);
        if opts.dump_types {
            print!("{}", vsc_hir_typeck::dump::dump_types(tcx));
        }
        if psess.dcx.has_errors().is_none() {
            if opts.unpretty == Some(Unpretty::ThirTree) {
                print!("{}", vsc_mir_build::thir_tree(tcx));
            }
            vsc_mir_build::check_crate(tcx);
        }
        if psess.dcx.has_errors().is_none() {
            vsc_mir_build::build_mir(tcx);
            if opts.validate_mir {
                vsc_mir_build::validate_all(tcx);
            }
            if opts.run_mir && psess.dcx.has_errors().is_none() {
                struct StdOut;
                impl vsc_const_eval::Output for StdOut {
                    fn stdout(&mut self, s: &str) {
                        use std::io::Write;
                        let _ = std::io::stdout().write_all(s.as_bytes());
                        let _ = std::io::stdout().flush();
                    }
                    fn stderr(&mut self, s: &str) {
                        use std::io::Write;
                        let _ = std::io::stderr().write_all(s.as_bytes());
                    }
                }
                return matches!(vsc_const_eval::run_main(tcx, &mut StdOut), vsc_const_eval::RunResult::Panicked);
            }
            if opts.dump_mir {
                // The core stubs' bodies (`//@ add-core-stubs`) aren't the
                // test's.
                print!("{}", vsc_mir_build::dump_mir(tcx, |f| f.ends_with("minicore.vs")));
            }
        }
    }
    false
}

/// Every file of every package, in package order.
fn print_crate(psess: &ParseSess, krate: &vsc_ast::Crate, unpretty: Unpretty) {
    let mut files = Vec::new();
    collect_files(&krate.root, &mut files);
    for (i, file) in files.iter().enumerate() {
        if files.len() > 1 {
            if i > 0 {
                println!();
            }
            let name = psess.source_map.lookup_source_file(file.span.lo()).map(|f| f.name.to_string()).unwrap_or_default();
            println!("// {name}");
        }
        match unpretty {
            Unpretty::Ast => print!("{}", vsc_ast_pretty::source_file_to_string(file)),
            Unpretty::AstTree => println!("{file:#?}"),
            Unpretty::Hir | Unpretty::HirTree | Unpretty::ThirTree => unreachable!("printed after lowering"),
        }
    }
}

fn collect_files<'a>(pkg: &'a vsc_ast::Package, out: &mut Vec<&'a vsc_ast::SourceFile>) {
    out.extend(&pkg.files);
    for child in &pkg.children {
        collect_files(child, out);
    }
}
