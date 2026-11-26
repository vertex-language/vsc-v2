//! The `vsc` command line (rustc's `rustc_driver_impl`).
//!
//! `vsc` takes rustc's command line, so cargo can drive it (`build.rustc =
//! "vsc"`). Parses CLI arguments, builds a [`vsc_interface::Config`], and
//! runs the compiler pipeline through parsing, expansion, resolution, HIR
//! lowering, type/borrow checking, MIR, Obsidian codegen, and native linking.

mod args;

use std::process::ExitCode;
use std::sync::Arc;

use vsc_codegen_obsidian::ObsidianCodegenBackend;
use vsc_errors::{ColorConfig, DiagCtxt, FatalErrorMarker};
use vsc_interface::{
    Config, Input, Linker, create_and_enter_global_ctxt, handle_session_prints, passes,
    resolve_and_lower,
};
use vsc_session::{ParseSess, Session};
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
            eprintln!(
                "\nnote: this is a bug in vsc; please report it at https://github.com/vertex-language/vsc-v2/issues"
            );
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

    let unpretty = opts.unpretty;
    let parse_only = opts.parse_only;
    let input = Input::File(opts.input);
    let config = Config {
        opts: opts.sess,
        input: input.clone(),
        override_queries: None,
        make_codegen_backend: Some(Box::new(|_: &Session| Box::new(ObsidianCodegenBackend::new()))),
    };

    vsc_interface::run_compiler(config, |compiler| {
        let sess = &compiler.sess;
        let krate = passes::parse(sess, &input);

        if let Some(unpretty_ast) = unpretty.filter(|u| u.is_ast()) {
            print_crate(&sess.psess, &krate, unpretty_ast);
        }

        // Set when `-Z run-mir` ran the program and it panicked.
        let mut program_panicked = false;
        let stop_after_parse = parse_only || unpretty.is_some_and(Unpretty::is_ast);
        if !stop_after_parse {
            let (resolved, hir) = resolve_and_lower(sess, &krate);
            if handle_session_prints(sess, resolved.crate_name) {
                return if compiler.dcx().has_errors().is_some() {
                    compiler.dcx().print_error_count();
                    ExitCode::from(1)
                } else {
                    ExitCode::SUCCESS
                };
            }

            match unpretty {
                Some(Unpretty::Hir) => println!("{}", vsc_hir_pretty::crate_to_string(&hir)),
                Some(Unpretty::HirTree) => println!("{hir:#?}"),
                // Analysis starts from a crate without errors, so its errors
                // don't repeat earlier ones.
                None | Some(Unpretty::ThirTree) if compiler.dcx().has_errors().is_none() => {
                    let unpretty_thir = unpretty == Some(Unpretty::ThirTree);
                    let (outcome, linker) =
                        create_and_enter_global_ctxt(compiler, &hir, &resolved, |tcx| {
                            let outcome = passes::analysis(tcx, sess, unpretty_thir);
                            let linker = (!outcome.stop_before_codegen && !outcome.panicked)
                                .then(|| {
                                    Linker::codegen_and_build_linker(
                                        tcx,
                                        sess,
                                        compiler.codegen_backend.as_ref(),
                                    )
                                });
                            (outcome, linker)
                        });
                    program_panicked = outcome.panicked;
                    if let Some(linker) = linker {
                        linker.link(sess, compiler.codegen_backend.as_ref());
                    }
                }
                None | Some(Unpretty::Ast | Unpretty::AstTree | Unpretty::ThirTree) => {}
            }
        }

        if compiler.dcx().has_errors().is_some() {
            compiler.dcx().print_error_count();
            return ExitCode::from(1);
        }
        // A program that panicked exits as Rust's do.
        if program_panicked {
            return ExitCode::from(101);
        }
        ExitCode::SUCCESS
    })
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
            let name = psess
                .source_map
                .lookup_source_file(file.span.lo())
                .map(|f| f.name.to_string())
                .unwrap_or_default();
            println!("// {name}");
        }
        match unpretty {
            Unpretty::Ast => print!("{}", vsc_ast_pretty::source_file_to_string(file)),
            Unpretty::AstTree => println!("{file:#?}"),
            Unpretty::Hir | Unpretty::HirTree | Unpretty::ThirTree => {
                unreachable!("printed after lowering")
            }
        }
    }
}

fn collect_files<'a>(pkg: &'a vsc_ast::Package, out: &mut Vec<&'a vsc_ast::SourceFile>) {
    out.extend(&pkg.files);
    for child in &pkg.children {
        collect_files(child, out);
    }
}
