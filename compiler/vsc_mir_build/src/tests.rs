use std::cell::OnceCell;
use std::path::PathBuf;

use vsc_errors::SharedBuffer;
use vsc_middle::arena::Arena;
use vsc_middle::query::Providers;
use vsc_middle::thir::*;
use vsc_middle::ty::*;
use vsc_pattern_analysis::{Constructor, MatchCx};
use vsc_session::ParseSess;
use vsc_session::config::{TargetInfo, default_configuration};
use vsc_span::DUMMY_SP;

const MINICORE: &str = include_str!("../../../tests/auxiliary/minicore.vs");

fn with_tcx<R>(src: &str, f: impl for<'tcx> FnOnce(TyCtxt<'tcx>, String) -> R) -> R {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir: PathBuf = std::env::temp_dir().join(format!("vsc-mirbuild-{}-{:x}-{n}", std::process::id(), h.finish()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src: String = src.lines().filter(|l| !l.starts_with("package ")).collect::<Vec<_>>().join("\n");
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
        outputs
            .defs
            .iter()
            .map(|d| DefInfo { kind: d.kind, parent: d.parent, name: d.name.unwrap_or(vsc_span::kw::Empty), span: d.span })
            .collect(),
    );
    let mut providers = Providers::default();
    vsc_passes::provide(&mut providers);
    vsc_hir_analysis::provide(&mut providers);
    vsc_hir_typeck::provide(&mut providers);
    crate::provide(&mut providers);
    let arena = Arena::default();
    let gcx = OnceCell::new();
    let tcx = TyCtxt::create(&arena, &gcx, CtxtInputs { sess: &psess, target: &target, hir: &hir, defs, crate_name: outputs.crate_name, providers });
    vsc_hir_analysis::check_crate(tcx);
    vsc_hir_typeck::check_crate(tcx);
    assert!(psess.dcx.has_errors().is_none(), "errors before THIR:\n{}", buf.contents());
    crate::check_crate(tcx);
    f(tcx, buf.contents())
}

/// Every body's THIR, function literals' included.
fn all_thir<'tcx>(tcx: TyCtxt<'tcx>) -> Vec<(String, &'tcx Thir<'tcx>)> {
    let mut out = Vec::new();
    for (local, info) in tcx.defs.iter_enumerated() {
        let file = tcx.sess.source_map.lookup_source_file(info.span.lo());
        if !file.is_some_and(|f| f.name.to_string().ends_with("main.vs")) {
            continue;
        }
        let has_body = tcx.body_owned_by(local).is_some() || info.kind == vsc_hir::def::DefKind::Closure;
        if has_body && !matches!(info.kind, vsc_hir::def::DefKind::AnonConst | vsc_hir::def::DefKind::InlineConst) {
            out.push((tcx.def_path_str(local.to_def_id()), tcx.thir_body(local.to_def_id())));
        }
    }
    out
}

#[test]
fn ui_programs_build_complete_thir() {
    for src in [
        include_str!("../../../tests/ui/typeck/inference.vs"),
        include_str!("../../../tests/ui/typeck/blocks.vs"),
        include_str!("../../../tests/ui/typeck/classes-and-funcs.vs"),
        include_str!("../../../tests/ui/typeck/traits.vs"),
        include_str!("../../../tests/ui/typeck/patterns.vs"),
        include_str!("../../../tests/ui/patterns/exhaustive.vs"),
    ] {
        with_tcx(src, |tcx, diags| {
            assert_eq!(diags, "");
            for (name, thir) in all_thir(tcx) {
                for (i, e) in thir.exprs.iter_enumerated() {
                    assert!(!matches!(e.kind, ExprKind::Error), "{name}: error expression {i:?} at {:?}", e.span);
                    assert!(!matches!(e.ty.kind(), TyKind::Error | TyKind::Infer(_)), "{name}: {:?} has type {}", e.span, tcx.ty_to_string(e.ty));
                }
            }
        });
    }
}

#[test]
fn method_calls_become_calls_with_autoref() {
    let src = "struct S { v: int64 }\nimplement S { func Get(&self) -> int64 { self.v } }\nfunc f(s: S) -> int64 { s.Get() }";
    with_tcx(src, |tcx, _| {
        let thir = all_thir(tcx).into_iter().find(|(n, _)| n == "f").unwrap().1;
        let call = thir.exprs.iter().find_map(|e| match &e.kind {
            ExprKind::Call { fun, args, .. } => Some((*fun, args.clone())),
            _ => None,
        });
        let (fun, args) = call.expect("a call");
        assert!(matches!(thir.exprs[fun].kind, ExprKind::ZstLiteral));
        assert!(matches!(thir.exprs[fun].ty.kind(), TyKind::FnDef(..)));
        // The receiver is borrowed: `S.Get(&s)`.
        assert!(matches!(thir.exprs[args[0]].kind, ExprKind::Borrow { borrow_kind: BorrowKind::Shared, .. }));
    });
}

#[test]
fn constructor_calls_build_values() {
    with_tcx("struct P(int64, int64)\nfunc f() -> Option<P> { Option.Some(P(1, 2)) }", |tcx, _| {
        let thir = all_thir(tcx).into_iter().find(|(n, _)| n == "f").unwrap().1;
        let adts = thir.exprs.iter().filter(|e| matches!(e.kind, ExprKind::Adt(_))).count();
        assert_eq!(adts, 2);
        assert!(!thir.exprs.iter().any(|e| matches!(e.kind, ExprKind::Call { .. })));
    });
}

#[test]
fn default_binding_modes_become_deref_patterns() {
    with_tcx("func f(o: &Option<int64>) { match o { Option.Some(x) => {}\n Option.None => {} } }", |tcx, _| {
        let thir = all_thir(tcx).into_iter().find(|(n, _)| n == "f").unwrap().1;
        let arm = &thir.arms.iter().next().unwrap();
        let PatKind::Deref { subpattern } = &arm.pattern.kind else { panic!("{:?}", arm.pattern.kind) };
        let PatKind::Variant { subpatterns, .. } = &subpattern.kind else { panic!() };
        let PatKind::Binding { mode, ty, .. } = &subpatterns[0].pattern.kind else { panic!() };
        assert_eq!(*mode, ByRef::Yes(Mutability::Not));
        assert_eq!(tcx.ty_to_string(*ty), "&int64");
    });
}

#[test]
fn class_literals_build_class_values() {
    with_tcx("class C { v: int64 }\nfunc f() -> C { C { v: 1 } }", |tcx, _| {
        let thir = all_thir(tcx).into_iter().find(|(n, _)| n == "f").unwrap().1;
        assert!(thir.exprs.iter().any(|e| matches!(e.kind, ExprKind::ClassNew { .. })));
    });
}

// ---------------------------------------------------------------------------
// Exhaustiveness against brute force
// ---------------------------------------------------------------------------

/// A small deterministic random generator (xorshift), for repeatable tests.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn pat<'tcx>(ty: Ty<'tcx>, kind: PatKind<'tcx>) -> Pat<'tcx> {
    Pat { ty, span: DUMMY_SP, kind }
}

#[test]
fn integer_ranges_match_brute_force() {
    with_tcx("func f() {}", |tcx, _| {
        let cx = MatchCx::new(tcx);
        let u8_ = tcx.mk_ty(TyKind::Uint(UintTy::U8));
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..300 {
            let n = 1 + rng.below(5) as usize;
            let mut covered = [false; 256];
            let mut arms = Vec::new();
            for _ in 0..n {
                let a = rng.below(256) as u128;
                let b = (a + rng.below(120) as u128).min(255);
                for v in a..=b {
                    covered[v as usize] = true;
                }
                let kind = if a == b {
                    PatKind::Constant { value: PatConst::Int(a) }
                } else {
                    PatKind::Range { lo: Some(PatConst::Int(a)), hi: Some(PatConst::Int(b)), end: RangeEnd::Included }
                };
                arms.push((cx.deconstruct(&pat(u8_, kind)), false));
            }
            let report = cx.analyze(&arms, u8_);
            let exhaustive = covered.iter().all(|&c| c);
            assert_eq!(report.missing.is_empty(), exhaustive, "{arms:?}");
            // Every witness is a value no arm covers.
            for w in &report.missing {
                let Constructor::IntRange(r) = w.ctor else { panic!("{w:?}") };
                assert!((r.lo..=r.hi).all(|v| !covered[v as usize]), "witness {r:?} is covered");
            }
        }
    });
}

#[test]
fn bool_tuples_match_brute_force() {
    with_tcx("func f() {}", |tcx, _| {
        let cx = MatchCx::new(tcx);
        let b = tcx.types.bool;
        let tup = tcx.mk_tup(&[b, b, b]);
        let mut rng = Rng(42);
        for _ in 0..300 {
            let n = 1 + rng.below(6) as usize;
            let mut covered = [false; 8];
            let mut arms = Vec::new();
            for _ in 0..n {
                // Each position: true, false or `_`.
                let choice: Vec<u64> = (0..3).map(|_| rng.below(3)).collect();
                for v in 0..8u32 {
                    if (0..3).all(|i| choice[i] == 2 || (choice[i] == 1) == ((v >> i) & 1 == 1)) {
                        covered[v as usize] = true;
                    }
                }
                let subpatterns = (0..3)
                    .map(|i| FieldPat {
                        field: i,
                        pattern: match choice[i] {
                            2 => pat(b, PatKind::Wild),
                            c => pat(b, PatKind::Constant { value: PatConst::Bool(c == 1) }),
                        },
                    })
                    .collect();
                arms.push((cx.deconstruct(&pat(tup, PatKind::Leaf { subpatterns })), false));
            }
            let report = cx.analyze(&arms, tup);
            assert_eq!(report.missing.is_empty(), covered.iter().all(|&c| c));
        }
    });
}

// ---------------------------------------------------------------------------
// MIR
// ---------------------------------------------------------------------------

/// Builds and validates the MIR of every body in `main.vs`.
fn assert_valid_mir(tcx: TyCtxt<'_>) {
    for (local, info) in tcx.defs.iter_enumerated() {
        let file = tcx.sess.source_map.lookup_source_file(info.span.lo());
        if !file.is_some_and(|f| f.name.to_string().ends_with("main.vs")) {
            continue;
        }
        let has_body = tcx.body_owned_by(local).is_some() || info.kind == vsc_hir::def::DefKind::Closure;
        if !has_body || matches!(info.kind, vsc_hir::def::DefKind::InlineConst) {
            continue;
        }
        let body = tcx.mir_built(local.to_def_id());
        let errors = crate::validate::validate(tcx, body);
        assert!(errors.is_empty(), "invalid MIR:\n{}\n\n{}", errors.join("\n"), tcx.mir_to_string(body));
    }
}

#[test]
fn ui_programs_build_valid_mir() {
    for src in [
        include_str!("../../../tests/ui/typeck/inference.vs"),
        include_str!("../../../tests/ui/typeck/blocks.vs"),
        include_str!("../../../tests/ui/typeck/classes-and-funcs.vs"),
        include_str!("../../../tests/ui/typeck/traits.vs"),
        include_str!("../../../tests/ui/typeck/patterns.vs"),
        include_str!("../../../tests/ui/patterns/exhaustive.vs"),
    ] {
        with_tcx(src, |tcx, diags| {
            assert_eq!(diags, "");
            assert_valid_mir(tcx);
        });
    }
}

#[test]
fn the_core_stubs_build_valid_mir() {
    with_tcx("func f() {}", |tcx, _| {
        for (local, info) in tcx.defs.iter_enumerated() {
            let has_body = tcx.body_owned_by(local).is_some() || info.kind == vsc_hir::def::DefKind::Closure;
            if has_body && info.kind != vsc_hir::def::DefKind::InlineConst {
                let body = tcx.mir_built(local.to_def_id());
                let errors = crate::validate::validate(tcx, body);
                assert!(errors.is_empty(), "{}: {}\n{}", tcx.def_path_str(local.to_def_id()), errors.join("\n"), tcx.mir_to_string(body));
            }
        }
    });
}
