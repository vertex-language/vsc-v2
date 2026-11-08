use std::cell::OnceCell;
use std::path::PathBuf;

use vsc_errors::SharedBuffer;
use vsc_hir::intravisit::{self, Visitor};
use vsc_hir::{self as hir};
use vsc_middle::arena::Arena;
use vsc_middle::query::Providers;
use vsc_middle::ty::*;
use vsc_session::ParseSess;
use vsc_session::config::{TargetInfo, default_configuration};
use vsc_span::def_id::DefId;

const MINICORE: &str = include_str!("../../../tests/auxiliary/minicore.vs");

/// Runs the whole front end on `src` (with the core stubs in its package)
/// and hands `f` the context and the diagnostics.
fn with_tcx<R>(src: &str, f: impl for<'tcx> FnOnce(TyCtxt<'tcx>, String) -> R) -> R {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    let dir: PathBuf = std::env::temp_dir().join(format!("vsc-typeck-{}-{:x}", std::process::id(), h.finish()));
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
    assert!(psess.dcx.has_errors().is_none(), "errors before type checking:\n{}", buf.contents());
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
    crate::provide(&mut providers);
    let arena = Arena::default();
    let gcx = OnceCell::new();
    let tcx = TyCtxt::create(&arena, &gcx, CtxtInputs { sess: &psess, target: &target, hir: &hir, defs, crate_name: outputs.crate_name, providers });
    vsc_hir_analysis::check_crate(tcx);
    crate::check_crate(tcx);
    f(tcx, buf.contents())
}

fn fn_named(tcx: TyCtxt<'_>, name: &str) -> DefId {
    tcx.defs
        .iter_enumerated()
        .find(|(_, d)| d.name.as_str() == name && matches!(d.kind, vsc_hir::def::DefKind::Fn | vsc_hir::def::DefKind::AssocFn))
        .map(|(id, _)| id.to_def_id())
        .unwrap_or_else(|| panic!("no function `{name}`"))
}

/// Every expression and pattern of every body in `main.vs` has a type,
/// with no variables or errors left.
fn assert_fully_typed(tcx: TyCtxt<'_>) {
    struct Check<'a, 'tcx> {
        tcx: TyCtxt<'tcx>,
        results: &'a TypeckResults<'tcx>,
        missing: Vec<String>,
    }
    impl<'tcx> Check<'_, 'tcx> {
        fn check(&mut self, id: hir::HirId, span: vsc_span::Span) {
            match self.results.node_type(id) {
                Some(t) if !has_error(t) => {}
                Some(t) => self.missing.push(format!("{} at {span:?}", self.tcx.ty_to_string(t))),
                None => self.missing.push(format!("no type at {span:?}")),
            }
        }
    }
    impl<'tcx> Visitor<'tcx> for Check<'_, 'tcx> {
        fn krate(&self) -> Option<&'tcx hir::Crate> {
            Some(self.tcx.hir_crate)
        }
        fn visit_nested_item(&mut self, _: hir::ItemId) {}
        fn visit_expr(&mut self, e: &'tcx hir::Expr) {
            self.check(e.hir_id, e.span);
            intravisit::walk_expr(self, e)
        }
        fn visit_pat(&mut self, p: &'tcx hir::Pat) {
            self.check(p.hir_id, p.span);
            intravisit::walk_pat(self, p)
        }
    }
    fn has_error(t: Ty<'_>) -> bool {
        let mut e = false;
        vsc_infer::walk_tys(t, &mut |t| e |= matches!(t.kind(), TyKind::Error | TyKind::Infer(_)));
        e
    }
    for (local, _) in tcx.defs.iter_enumerated() {
        let Some(body) = crate::body_of(tcx, local) else { continue };
        let file = tcx.sess.source_map.lookup_source_file(tcx.def_span(local.to_def_id()).lo()).unwrap();
        if !file.name.to_string().ends_with("main.vs") || tcx.def_kind(local.to_def_id()) == vsc_hir::def::DefKind::AnonConst {
            continue;
        }
        let results = tcx.typeck(local.to_def_id());
        assert!(!results.tainted_by_errors);
        let mut c = Check { tcx, results, missing: Vec::new() };
        c.visit_body(tcx.hir_crate.body(body));
        assert!(c.missing.is_empty(), "{}: {}", tcx.def_path_str(local.to_def_id()), c.missing.join("\n"));
    }
}

const PROGRAM: &str = include_str!("../../../tests/ui/typeck/inference.vs");

#[test]
fn ui_programs_are_fully_typed() {
    for src in [
        PROGRAM,
        include_str!("../../../tests/ui/typeck/blocks.vs"),
        include_str!("../../../tests/ui/typeck/classes-and-funcs.vs"),
        include_str!("../../../tests/ui/typeck/traits.vs"),
        include_str!("../../../tests/ui/typeck/patterns.vs"),
    ] {
        // Drop the test's own `package` line: the harness adds one.
        let src: String = src.lines().filter(|l| !l.starts_with("package ")).collect::<Vec<_>>().join("\n");
        with_tcx(&src, |tcx, diags| {
            assert_eq!(diags, "");
            assert_fully_typed(tcx);
        });
    }
}

#[test]
fn literals_default_to_int32_and_float64() {
    with_tcx("func f() { let a = 1\n let b = 2.0\n let c: uint8 = 3 }", |tcx, diags| {
        assert_eq!(diags, "");
        let results = tcx.typeck(fn_named(tcx, "f"));
        let mut tys: Vec<String> = results
            .node_types
            .iter()
            .filter(|(id, _)| matches!(tcx.hir_crate.owner(id.owner).node, hir::OwnerNode::Item(_)))
            .map(|(_, t)| tcx.ty_to_string(*t))
            .collect();
        tys.sort();
        tys.dedup();
        assert!(tys.contains(&"int32".to_string()) && tys.contains(&"float64".to_string()) && tys.contains(&"uint8".to_string()), "{tys:?}");
    });
}

#[test]
fn method_calls_record_their_autoref() {
    let src = "struct S { v: int64 }\nimplement S { func Get(&self) -> int64 { self.v }\nfunc Set(&mut self) {} }\nfunc f() { var s = S { v: 1 }\n s.Get()\n s.Set() }";
    with_tcx(src, |tcx, diags| {
        assert_eq!(diags, "");
        let results = tcx.typeck(fn_named(tcx, "f"));
        let mut borrows: Vec<Mutability> = results
            .adjustments
            .values()
            .flat_map(|a| a.iter())
            .filter_map(|a| match a.kind {
                Adjust::Borrow(m) => Some(m),
                _ => None,
            })
            .collect();
        borrows.sort_by_key(|m| m.is_mut());
        assert_eq!(borrows, vec![Mutability::Not, Mutability::Mut]);
        let resolved: Vec<String> = results.type_dependent_defs.values().map(|(_, d)| tcx.def_path_str(*d)).collect();
        assert!(resolved.contains(&"S.Get".to_string()) && resolved.contains(&"S.Set".to_string()), "{resolved:?}");
    });
}

#[test]
fn arrays_unsize_to_slices() {
    with_tcx("func takes(x: &[int64]) {}\nfunc f() { let a = [1, 2]\n takes(&a) }", |tcx, diags| {
        assert_eq!(diags, "");
        let results = tcx.typeck(fn_named(tcx, "f"));
        assert!(results.adjustments.values().flatten().any(|a| a.kind == Adjust::Unsize));
    });
}

#[test]
fn autoderef_reaches_fields_through_references() {
    with_tcx("struct P { x: int64 }\nfunc f(p: &&P) -> int64 { p.x }", |tcx, diags| {
        assert_eq!(diags, "");
        let results = tcx.typeck(fn_named(tcx, "f"));
        let derefs = results.adjustments.values().flatten().filter(|a| a.kind == Adjust::Deref).count();
        assert_eq!(derefs, 2);
    });
}

#[test]
fn errors_taint_the_results() {
    with_tcx("func f() -> int64 { true }", |tcx, diags| {
        assert!(diags.contains("mismatched types"), "{diags}");
        assert!(tcx.typeck(fn_named(tcx, "f")).tainted_by_errors);
    });
}
