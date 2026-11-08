use std::cell::OnceCell;
use std::path::PathBuf;

use vsc_errors::SharedBuffer;
use vsc_middle::arena::Arena;
use vsc_middle::query::Providers;
use vsc_middle::ty::*;
use vsc_session::ParseSess;
use vsc_session::config::{TargetInfo, default_configuration};

use crate::interpret::{Output, RunResult, run_main};

const MINICORE: &str = include_str!("../../../tests/auxiliary/minicore.vs");

#[derive(Default)]
struct Captured {
    out: String,
    err: String,
}

impl Output for Captured {
    fn stdout(&mut self, s: &str) {
        self.out.push_str(s);
    }
    fn stderr(&mut self, s: &str) {
        self.err.push_str(s);
    }
}

/// Compiles `src` (with the core stubs) and runs its `main`: the result,
/// its output, and the compiler's diagnostics.
fn run(src: &str, step_limit: Option<u64>) -> (RunResult, Captured, String) {
    use std::hash::{Hash, Hasher};
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir: PathBuf = std::env::temp_dir().join(format!("vsc-interp-{}-{:x}-{n}", std::process::id(), h.finish()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.vs"), format!("package main\n{src}")).unwrap();
    std::fs::write(dir.join("minicore.vs"), MINICORE).unwrap();
    let buf = SharedBuffer::new();
    let psess = ParseSess::with_buffer(buf.clone());
    let target = TargetInfo::from_triple("x86_64-unknown-linux-gnu").unwrap();
    let cfg = default_configuration(&target, true, false);
    let ecfg = vsc_expand::ExpansionConfig { crate_name: Some("app"), cfg: &cfg };
    let krate = vsc_expand::expand_crate(&psess, &dir.join("main.vs"), &ecfg).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let outputs = vsc_resolve::resolve_crate(&psess, &krate);
    let hir = vsc_ast_lowering::lower_to_hir(&psess, &krate, &outputs);
    let defs = vsc_index::IndexVec::from_raw(
        outputs.defs.iter().map(|d| DefInfo { kind: d.kind, parent: d.parent, name: d.name.unwrap_or(vsc_span::kw::Empty), span: d.span }).collect(),
    );
    let mut providers = Providers::default();
    vsc_passes::provide(&mut providers);
    vsc_hir_analysis::provide(&mut providers);
    vsc_hir_typeck::provide(&mut providers);
    vsc_mir_build::provide(&mut providers);
    let arena = Arena::default();
    let gcx = OnceCell::new();
    let tcx = TyCtxt::create(&arena, &gcx, CtxtInputs { sess: &psess, target: &target, hir: &hir, defs, crate_name: outputs.crate_name, providers });
    vsc_hir_analysis::check_crate(tcx);
    vsc_hir_typeck::check_crate(tcx);
    vsc_mir_build::check_crate(tcx);
    assert!(psess.dcx.has_errors().is_none(), "errors before running:\n{}", buf.contents());
    let mut out = Captured::default();
    let result = match step_limit {
        None => run_main(tcx, &mut out),
        Some(limit) => crate::interpret::run_main_with_limit(tcx, &mut out, limit),
    };
    (result, out, buf.contents())
}

#[test]
fn prints_and_exits() {
    let (r, out, diags) = run("func main() { println(\"{} {}\", 1 + 2, \"three\")\n eprint(\"e\") }", None);
    assert!(matches!(r, RunResult::Exited), "{diags}");
    assert_eq!(out.out, "3 three\n");
    assert_eq!(out.err, "e");
}

#[test]
fn panics_report_where_and_why() {
    let (r, out, _) = run("func main() {\n    let x: int32 = 2147483647\n    let y = x + 1\n}", None);
    assert!(matches!(r, RunResult::Panicked));
    assert!(out.err.contains("main.vs:4:13:\nattempt to add with overflow"), "{}", out.err);
}

#[test]
fn infinite_loops_hit_the_step_limit() {
    let (r, _, diags) = run("func main() { loop {} }", Some(10_000));
    assert!(matches!(r, RunResult::Failed));
    assert!(diags.contains("unsupported: the program ran for more than 10000 steps"), "{diags}");
}

#[test]
fn generic_and_trait_calls_resolve_to_the_impl() {
    let src = "trait Speak { func speak(&self) -> int32 }\nstruct A {}\nstruct B {}\nimplement Speak for A { func speak(&self) -> int32 { 1 } }\nimplement Speak for B { func speak(&self) -> int32 { 2 } }\nfunc twice<T: Speak>(t: &T) -> int32 { t.speak() * 2 }\nfunc main() { println(\"{} {}\", twice(&A {}), twice(&B {})) }";
    let (r, out, diags) = run(src, None);
    assert!(matches!(r, RunResult::Exited), "{diags}");
    assert_eq!(out.out, "2 4\n");
}

#[test]
fn class_values_are_shared() {
    let src = "class Box { v: int32 }\nfunc bump(b: Box) { b.v += 1 }\nfunc main() { let b = Box { v: 1 }\n bump(b)\n bump(b)\n println(\"{}\", b.v) }";
    let (r, out, diags) = run(src, None);
    assert!(matches!(r, RunResult::Exited), "{diags}");
    assert_eq!(out.out, "3\n");
}
