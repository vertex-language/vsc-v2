//! Compilation pass orchestration and query wiring (`rustc_interface::passes`).

use std::any::Any;
use std::cell::OnceCell;
use std::fs;
use std::sync::{Arc, LazyLock};

use vsc_codegen_ssa::CrateInfo;
use vsc_codegen_ssa::back::link::output_path_for_crate_type;
use vsc_codegen_ssa::traits::CodegenBackend;
use vsc_const_eval::{Output as MiriOutput, RunResult};
use vsc_index::IndexVec;
use vsc_middle::arena::Arena;
use vsc_middle::query::Providers;
use vsc_middle::ty::{CtxtInputs, DefInfo, TyCtxt};
use vsc_resolve::ResolverOutputs;
use vsc_session::Session;
use vsc_session::config::{
    CrateType, OutFileName, OutputFilenames, OutputType, PrintRequest,
    default_configuration_with_panic,
};
use vsc_span::Symbol;

use crate::interface::{Compiler, Input};

/// Default front-end and middle-end query providers (`DEFAULT_QUERY_PROVIDERS`).
pub static DEFAULT_QUERY_PROVIDERS: LazyLock<Providers> = LazyLock::new(default_providers);

/// Returns a populated [`Providers`] table with all front-end and middle-end queries installed.
pub fn default_providers() -> Providers {
    let mut providers = Providers::default();
    vsc_passes::provide(&mut providers);
    vsc_hir_analysis::provide(&mut providers);
    vsc_hir_typeck::provide(&mut providers);
    vsc_mir_build::provide(&mut providers);
    vsc_mir_transform::provide(&mut providers);
    vsc_ty_utils::provide(&mut providers);
    vsc_borrowck::provide(&mut providers);
    vsc_symbol_mangling::provide(&mut providers);
    vsc_monomorphize::provide(&mut providers);
    providers
}

/// Summary returned by [`analysis`] indicating interpreter exit state and codegen eligibility.
#[derive(Clone, Copy, Debug, Default)]
pub struct AnalysisOutcome {
    pub panicked: bool,
    pub stop_before_codegen: bool,
}

/// Parses, configures (`@cfg`), expands built-ins, and assigns AST node IDs.
pub fn parse(sess: &Session, input: &Input) -> vsc_ast::Crate {
    let mut cfg = default_configuration_with_panic(
        &sess.target_info,
        sess.opts.debug_assertions,
        sess.opts.test,
        sess.panic_strategy(),
    );
    for arg in &sess.opts.cfgs {
        if let Err(msg) = cfg.insert_from_arg(arg) {
            sess.dcx().fatal(msg);
        }
    }

    let path = match input {
        Input::File(path) => path.as_path(),
    };
    let ecfg = vsc_expand::ExpansionConfig {
        crate_name: sess.opts.crate_name.as_deref(),
        cfg: &cfg,
    };
    let auto_core = find_core_library(sess);
    let mut externs: Vec<(String, std::path::PathBuf)> = Vec::new();
    for (name, entry) in &sess.opts.externs.0 {
        if let Some(loc) = &entry.location {
            let is_source = loc.is_dir() || loc.extension().is_some_and(|e| e == "vs");
            if is_source {
                externs.push((name.clone(), loc.clone()));
            }
        } else if name == "core"
            && let Some(ref core_path) = auto_core
        {
            externs.push((name.clone(), core_path.clone()));
        }
    }
    match vsc_expand::expand_crate_with_externs(
        &sess.psess,
        path,
        &ecfg,
        &externs,
        auto_core.as_deref(),
    ) {
        Ok(krate) => krate,
        Err(err) => sess.dcx().fatal(format!("couldn't read `{}`: {err}", path.display())),
    }
}

/// Locates `library/core/src/lib.vs` from the sysroot, search paths, or source tree.
pub fn find_core_library(sess: &Session) -> Option<std::path::PathBuf> {
    let mut candidates = Vec::new();
    for sp in &sess.opts.search_paths {
        candidates.push(sp.dir.join("core/src/lib.vs"));
        candidates.push(sp.dir.join("library/core/src/lib.vs"));
    }
    let sysroot = sess.sysroot();
    candidates.push(sysroot.join("library/core/src/lib.vs"));
    candidates.push(sysroot.join("lib/vsc/library/core/src/lib.vs"));
    if let Some(parent) = sysroot.parent() {
        candidates.push(parent.join("library/core/src/lib.vs"));
    }
    if let Ok(exe) = std::env::current_exe() {
        for anc in exe.ancestors().take(5) {
            candidates.push(anc.join("library/core/src/lib.vs"));
        }
    }
    if let Some(repo_root) = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2) {
        candidates.push(repo_root.join("library/core/src/lib.vs"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Resolves names and lowers the expanded AST crate to HIR.
pub fn resolve_and_lower(sess: &Session, krate: &vsc_ast::Crate) -> (ResolverOutputs, vsc_hir::Crate) {
    let outputs = vsc_resolve::resolve_crate(&sess.psess, krate);
    if sess.opts.unstable_opts.dump_resolutions {
        print!("{}", vsc_resolve::dump::dump_resolutions(krate, &outputs, &sess.psess.source_map));
    }
    let hir = vsc_ast_lowering::lower_to_hir(&sess.psess, krate, &outputs);
    (outputs, hir)
}

/// Enters a freshly constructed [`TyCtxt`] for `hir` and executes `f`.
pub fn create_and_enter_global_ctxt<T, F>(
    compiler: &Compiler,
    hir: &vsc_hir::Crate,
    resolved: &ResolverOutputs,
    f: F,
) -> T
where
    F: for<'tcx> FnOnce(TyCtxt<'tcx>) -> T,
{
    let defs = IndexVec::from_raw(
        resolved
            .defs
            .iter()
            .map(|d| DefInfo {
                kind: d.kind,
                parent: d.parent,
                name: d.name.unwrap_or(vsc_span::kw::Empty),
                span: d.span,
            })
            .collect(),
    );
    let mut providers = *DEFAULT_QUERY_PROVIDERS;
    compiler.codegen_backend.provide(&mut providers);
    if let Some(callback) = compiler.override_queries {
        callback(&compiler.sess, &mut providers);
    }

    let arena = Arena::default();
    let gcx = OnceCell::new();
    let tcx = TyCtxt::create_with_options(
        &arena,
        &gcx,
        CtxtInputs {
            sess: &compiler.sess.psess,
            target: &compiler.sess.target_info,
            hir,
            defs,
            crate_name: resolved.crate_name,
            providers,
        },
        compiler.sess.opts.clone(),
    );
    f(tcx)
}

/// Handles crate-aware `--print` requests. Returns `true` when compilation should stop
/// cleanly right after printing.
pub fn handle_session_prints(sess: &Session, crate_name: Symbol) -> bool {
    if sess.opts.prints.is_empty() {
        return false;
    }
    let outputs = sess.output_filenames(crate_name.as_str(), None);
    let mut stop_after_print = false;
    for req in &sess.opts.prints {
        match req {
            PrintRequest::Sysroot => {
                println!("{}", sess.sysroot().display());
                stop_after_print = true;
            }
            PrintRequest::TargetLibdir => {
                let libdir = sess
                    .sysroot()
                    .join("lib")
                    .join("rustlib")
                    .join(&sess.target_info.triple)
                    .join("lib");
                println!("{}", libdir.display());
                stop_after_print = true;
            }
            PrintRequest::CrateName => {
                println!("{crate_name}");
                stop_after_print = true;
            }
            PrintRequest::FileNames => {
                for &crate_type in &sess.crate_types() {
                    let path = output_path_for_crate_type(sess, &outputs, crate_type);
                    if let Some(name) = path.file_name() {
                        println!("{}", name.to_string_lossy());
                    }
                }
                stop_after_print = true;
            }
            PrintRequest::TargetSpecs => {
                println!("{:#?}", sess.target);
                stop_after_print = true;
            }
            PrintRequest::TargetCpus => {
                println!("Available CPUs for this target:");
                println!("    {}", sess.target.cpu);
                stop_after_print = true;
            }
            PrintRequest::TargetFeatures => {
                println!("Features defined on this target:");
                println!("    {}", sess.target.features);
                stop_after_print = true;
            }
            PrintRequest::NativeStaticLibs | PrintRequest::TargetList | PrintRequest::Cfg => {}
        }
    }
    stop_after_print
}

/// Runs signature checking, type checking, THIR/MIR building, borrow checking,
/// and optional MIR interpretation or dumps.
pub fn analysis(tcx: TyCtxt<'_>, sess: &Session, unpretty_thir: bool) -> AnalysisOutcome {
    tcx.lang_items();
    vsc_passes::loops::check_crate(tcx);
    vsc_hir_analysis::check_crate(tcx);
    if sess.opts.unstable_opts.dump_signatures {
        print!("{}", vsc_hir_analysis::dump::dump_signatures(tcx));
    }
    if sess.dcx().has_errors().is_some() {
        return AnalysisOutcome { panicked: false, stop_before_codegen: true };
    }

    vsc_hir_typeck::check_crate(tcx);
    if sess.opts.unstable_opts.dump_types {
        print!("{}", vsc_hir_typeck::dump::dump_types(tcx));
    }
    if sess.dcx().has_errors().is_some() {
        return AnalysisOutcome { panicked: false, stop_before_codegen: true };
    }

    if unpretty_thir {
        print!("{}", vsc_mir_build::thir_tree(tcx));
    }
    vsc_mir_build::check_crate(tcx);
    if sess.dcx().has_errors().is_some() {
        return AnalysisOutcome { panicked: false, stop_before_codegen: true };
    }

    vsc_mir_build::build_mir(tcx);
    if sess.opts.unstable_opts.validate_mir {
        vsc_mir_build::validate_all(tcx);
    }
    if sess.dcx().has_errors().is_some() {
        return AnalysisOutcome { panicked: false, stop_before_codegen: true };
    }

    if sess.opts.unstable_opts.run_mir {
        struct StdOut;
        impl MiriOutput for StdOut {
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
        let panicked =
            matches!(vsc_const_eval::run_main(tcx, &mut StdOut), RunResult::Panicked);
        return AnalysisOutcome { panicked, stop_before_codegen: true };
    }

    let dump_or_unpretty = sess.opts.unstable_opts.dump_resolutions
        || sess.opts.unstable_opts.dump_signatures
        || sess.opts.unstable_opts.dump_types
        || sess.opts.unstable_opts.dump_mir
        || unpretty_thir;

    if sess.opts.unstable_opts.dump_mir {
        // Skip synthetic core stub bodies (`//@ add-core-stubs`) in MIR dump tests.
        print!("{}", vsc_mir_build::dump_mir(tcx, |f| f.ends_with("minicore.vs")));
    }

    if !dump_or_unpretty {
        vsc_borrowck::check_crate(tcx);
    }
    if sess.dcx().has_errors().is_some() {
        return AnalysisOutcome { panicked: false, stop_before_codegen: true };
    }

    if sess.opts.output_types.contains_key(OutputType::Mir) {
        write_mir(tcx, sess);
    }

    let stop_before_codegen = !sess.opts.output_types.should_codegen() || dump_or_unpretty;
    if !stop_before_codegen
        && sess.opts.output_types.should_link()
        && sess.crate_types().contains(&CrateType::Executable)
    {
        vsc_passes::entry::check_entry_fn(tcx);
    }

    AnalysisOutcome {
        panicked: false,
        stop_before_codegen: stop_before_codegen || sess.dcx().has_errors().is_some(),
    }
}

fn write_mir(tcx: TyCtxt<'_>, sess: &Session) {
    let outputs = sess.output_filenames(tcx.crate_name.as_str(), None);
    let mir_text = vsc_mir_build::dump_mir(tcx, |_| false);
    match outputs.path(OutputType::Mir) {
        OutFileName::Stdout => print!("{mir_text}"),
        OutFileName::Real(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
                && let Err(err) = fs::create_dir_all(parent)
            {
                sess.dcx().fatal(format!(
                    "failed to create output directory `{}`: {err}",
                    parent.display()
                ));
            }
            if let Err(err) = fs::write(&path, mir_text) {
                sess.dcx().fatal(format!("failed to write MIR `{}`: {err}", path.display()));
            }
        }
    }
}

pub(crate) fn start_codegen(
    codegen_backend: &dyn CodegenBackend,
    tcx: TyCtxt<'_>,
    sess: &Session,
) -> (Box<dyn Any>, CrateInfo, Arc<OutputFilenames>) {
    let crate_info = CrateInfo::new(tcx);
    let outputs = Arc::new(sess.output_filenames(tcx.crate_name.as_str(), None));
    let ongoing_codegen = codegen_backend.codegen_crate(tcx);
    (ongoing_codegen, crate_info, outputs)
}
