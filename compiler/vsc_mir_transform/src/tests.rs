use std::cell::OnceCell;
use std::sync::atomic::{AtomicUsize, Ordering};

use vsc_ast_lowering::lower_to_hir;
use vsc_errors::SharedBuffer;
use vsc_index::IndexVec;
use vsc_middle::arena::Arena;
use vsc_middle::mir::TerminatorKind;
use vsc_middle::query::Providers;
use vsc_middle::ty::{CtxtInputs, DefInfo, TyCtxt};
use vsc_mir_build::validate::validate;
use vsc_resolve::resolve_crate;
use vsc_session::config::{TargetInfo, default_configuration};
use vsc_session::parse::ParseSess;
use vsc_span::{Symbol, kw};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn check_optimized(src: &str, f: impl for<'tcx> FnOnce(TyCtxt<'tcx>)) {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("vsc_mir_xform_test_{}_{}", std::process::id(), n));
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
    vsc_ty_utils::provide(&mut providers);
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
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(psess.dcx.err_count(), 0, "unexpected errors:\n{}", buf.contents());
    f(tcx);
}

#[test]
fn drop_elaboration_and_cfg_simplification_stay_valid() {
    let src = r#"
@lang("drop")
trait Drop {
    func drop(&mut self)
}

struct Bomb { id: int32 }
implement Drop for Bomb {
    func drop(&mut self) {}
}

func make(flag: bool) -> int32 {
    let b = Bomb { id: 7 }
    if flag {
        return b.id
    }
    0
}

func main() -> int32 {
    make(true) + make(false)
}
"#;
    check_optimized(src, |tcx| {
        let make_def = tcx
            .defs
            .iter_enumerated()
            .find(|(_, d)| d.name.as_str() == "make")
            .unwrap()
            .0
            .to_def_id();
        let opt = tcx.optimized_mir(make_def);
        let errors = validate(tcx, opt);
        assert!(errors.is_empty(), "optimized MIR invalid: {errors:?}");
        let has_drop = opt
            .basic_blocks
            .iter()
            .any(|b| matches!(b.terminator().kind, TerminatorKind::Drop { .. }));
        assert!(has_drop, "expected Drop terminator in optimized MIR:\n{}", tcx.mir_to_string(opt));
    });
}

#[test]
fn conditional_move_generates_dynamic_drop_flag() {
    let src = r#"
@lang("drop")
trait Drop {
    func drop(&mut self)
}

struct Bomb { id: int32 }
implement Drop for Bomb {
    func drop(&mut self) {}
}

func take(_b: Bomb) {}

func maybe_move(cond: bool) {
    let b = Bomb { id: 9 }
    if cond {
        take(b)
    }
}
"#;
    check_optimized(src, |tcx| {
        let def = tcx
            .defs
            .iter_enumerated()
            .find(|(_, d)| d.name.as_str() == "maybe_move")
            .unwrap()
            .0
            .to_def_id();
        let opt = tcx.optimized_mir(def);
        let errors = validate(tcx, opt);
        assert!(errors.is_empty(), "optimized MIR invalid: {errors:?}");
        let drop_blocks = opt
            .basic_blocks
            .iter()
            .filter(|b| matches!(b.terminator().kind, TerminatorKind::Drop { .. }))
            .count();
        assert!(
            drop_blocks >= 1,
            "expected conditional Drop block in optimized MIR:\n{}",
            tcx.mir_to_string(opt)
        );
    });
}
