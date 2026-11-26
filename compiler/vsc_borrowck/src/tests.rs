use std::cell::OnceCell;
use std::sync::atomic::{AtomicUsize, Ordering};

use vsc_ast_lowering::lower_to_hir;
use vsc_errors::SharedBuffer;
use vsc_index::IndexVec;
use vsc_middle::arena::Arena;
use vsc_middle::query::Providers;
use vsc_middle::ty::{CtxtInputs, DefInfo, TyCtxt};
use vsc_resolve::resolve_crate;
use vsc_session::config::{TargetInfo, default_configuration};
use vsc_session::parse::ParseSess;
use vsc_span::{Symbol, kw};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn run_borrowck(src: &str) -> (usize, String) {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("vsc_borrowck_test_{}_{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.vs"), src).unwrap();

    let buf = SharedBuffer::new();
    let psess = ParseSess::with_buffer(buf.clone());
    let target = TargetInfo::host();
    let cfg = default_configuration(&target, true, false);
    let ecfg = vsc_expand::ExpansionConfig { crate_name: Some("test"), cfg: &cfg };
    let krate = vsc_expand::expand_crate(&psess, &dir.join("main.vs"), &ecfg).unwrap();
    let outputs = resolve_crate(&psess, &krate);
    let hir = lower_to_hir(&psess, &krate, &outputs);

    let mut providers = Providers::default();
    vsc_passes::provide(&mut providers);
    vsc_hir_analysis::provide(&mut providers);
    vsc_hir_typeck::provide(&mut providers);
    vsc_mir_build::provide(&mut providers);
    crate::provide(&mut providers);

    let defs = IndexVec::from_raw(
        outputs
            .defs
            .iter()
            .map(|d| DefInfo {
                kind: d.kind,
                span: d.span,
                parent: d.parent,
                name: d.name.unwrap_or(kw::Empty),
            })
            .collect(),
    );
    let arena = Arena::default();
    let gcx = OnceCell::new();
    let tcx = TyCtxt::create(
        &arena,
        &gcx,
        CtxtInputs {
            sess: &psess,
            target: &target,
            hir: &hir,
            defs,
            crate_name: Symbol::intern("test"),
            providers,
        },
    );
    vsc_hir_analysis::check_crate(tcx);
    vsc_hir_typeck::check_crate(tcx);
    vsc_mir_build::check_crate(tcx);
    vsc_mir_build::build_mir(tcx);
    if psess.dcx.err_count() == 0 {
        crate::check_crate(tcx);
    }
    let _ = std::fs::remove_dir_all(&dir);
    (psess.dcx.err_count(), buf.contents())
}

#[test]
fn valid_nll_closure_and_class_sharing_pass() {
    let src = r#"
@lang("Class")
struct Class<T> { inner: T }

class Counter { value: int64 }

func test_nll_and_class() -> int64 {
    var x = 10
    let r = &mut x
    *r = 20
    let y = x

    var total = 0
    let add = func(n: int64) { total += n }
    add(5)

    let c = Counter { value: 1 }
    let d = c
    d.value += 10
    total + y + c.value
}
"#;
    let (errs, out) = run_borrowck(src);
    assert_eq!(errs, 0, "unexpected borrowck errors:\n{out}");
}

#[test]
fn detects_use_after_move_and_mut_conflicts() {
    let src = r#"
struct NonCopy { x: int32 }
func take(v: NonCopy) -> int32 { v.x }

func bad_move() -> int32 {
    let a = NonCopy { x: 1 }
    let _ = take(a)
    take(a)
}

func bad_alias() -> int32 {
    var x = 1
    let r = &mut x
    let y = x
    *r = 2
    y + *r
}

func bad_return_local(_arg: &int32) -> &int32 {
    let x = 42
    &x
}

func bad_immut_reassign() -> int32 {
    let x = 1
    x = 2
    x
}

func bad_double_mut() -> int32 {
    var x = 1
    let a = &mut x
    let b = &mut x
    *a + *b
}
"#;
    let (errs, out) = run_borrowck(src);
    assert!(errs >= 5, "expected >= 5 errors, got {errs}:\n{out}");
    assert!(out.contains("E0382"), "missing E0382:\n{out}");
    assert!(out.contains("E0503"), "missing E0503:\n{out}");
    assert!(out.contains("E0515"), "missing E0515:\n{out}");
    assert!(out.contains("E0384"), "missing E0384:\n{out}");
    assert!(out.contains("E0499"), "missing E0499:\n{out}");
}

#[test]
fn loop_borrows_and_scoped_immutable_bindings() {
    let valid_src = r#"
struct Counter { val: int64 }

implement Counter {
    func inc(&mut self) -> int64 {
        self.val += 1
        self.val
    }
}

func ok_loops(c: &mut Counter, cond: bool) -> int64 {
    var n = 3
    while n > 0 {
        let _ = c.inc()
        n -= 1
    }
    loop {
        let x
        if cond {
            x = 1
        } else {
            x = 2
        }
        if x > 0 {
            return x
        }
    }
}
"#;
    let (errs, out) = run_borrowck(valid_src);
    assert_eq!(errs, 0, "unexpected errors in valid loop borrow/init test:\n{out}");

    let bad_src = r#"
func bad_outer_immut_in_loop() -> int64 {
    let x
    loop {
        x = 1
    }
}

func bad_inner_double_init() -> int64 {
    loop {
        let y
        y = 1
        y = 2
        return y
    }
}
"#;
    let (bad_errs, bad_out) = run_borrowck(bad_src);
    assert!(bad_errs >= 2, "expected E0384 errors in bad loop init test, got {bad_errs}:\n{bad_out}");
    assert!(bad_out.contains("E0384"), "missing E0384:\n{bad_out}");
}
