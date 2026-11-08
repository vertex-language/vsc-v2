use std::cell::OnceCell;
use std::path::PathBuf;

use vsc_errors::SharedBuffer;
use vsc_hir::LangItem;
use vsc_hir::def::DefKind;
use vsc_middle::arena::Arena;
use vsc_middle::query::Providers;
use vsc_middle::ty::*;
use vsc_session::ParseSess;
use vsc_session::config::{TargetInfo, default_configuration};
use vsc_span::def_id::DefId;

/// Runs the pipeline up to `check_crate` on one file and hands `f` the
/// context and the diagnostics so far.
fn with_tcx<R>(src: &str, f: impl for<'tcx> FnOnce(TyCtxt<'tcx>, &dyn Fn() -> String) -> R) -> R {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    let dir: PathBuf = std::env::temp_dir().join(format!("vsc-analysis-{}-{:x}", std::process::id(), h.finish()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.vs"), src).unwrap();
    let buf = SharedBuffer::new();
    let psess = ParseSess::with_buffer(buf.clone());
    let target = TargetInfo::from_triple("x86_64-unknown-linux-gnu").unwrap();
    let cfg = default_configuration(&target, true, false);
    let ecfg = vsc_expand::ExpansionConfig { crate_name: Some("app"), cfg: &cfg };
    let krate = vsc_expand::expand_crate(&psess, &dir.join("main.vs"), &ecfg).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let outputs = vsc_resolve::resolve_crate(&psess, &krate);
    let hir = vsc_ast_lowering::lower_to_hir(&psess, &krate, &outputs);
    assert!(psess.dcx.has_errors().is_none(), "errors before analysis:\n{}", buf.contents());
    let defs = vsc_index::IndexVec::from_raw(
        outputs
            .defs
            .iter()
            .map(|d| DefInfo { kind: d.kind, parent: d.parent, name: d.name.unwrap_or(vsc_span::kw::Empty), span: d.span })
            .collect(),
    );
    let mut providers = Providers::default();
    vsc_passes::provide(&mut providers);
    crate::provide(&mut providers);
    let arena = Arena::default();
    let gcx = OnceCell::new();
    let tcx = TyCtxt::create(
        &arena,
        &gcx,
        CtxtInputs { sess: &psess, target: &target, hir: &hir, defs, crate_name: outputs.crate_name, providers },
    );
    crate::check_crate(tcx);
    f(tcx, &|| buf.contents())
}

fn def_named(tcx: TyCtxt<'_>, name: &str) -> DefId {
    tcx.defs
        .iter_enumerated()
        .find(|(_, d)| d.name.as_str() == name && !matches!(d.kind, DefKind::Ctor(..) | DefKind::Field))
        .map(|(id, _)| id.to_def_id())
        .unwrap_or_else(|| panic!("no definition named `{name}`"))
}

const PRELUDE: &str = r#"
@lang("String")
struct String { len: uint }
@lang("Class")
struct Class<T> { inner: T }
@lang("Func")
struct Func<F> { f: F }
"#;

#[test]
fn types_are_interned() {
    with_tcx("func f() {}", |tcx, _| {
        let a = tcx.mk_tup(&[tcx.types.bool, tcx.types.char]);
        let b = tcx.mk_tup(&[tcx.types.bool, tcx.types.char]);
        assert!(std::ptr::eq(a.kind(), b.kind()));
        assert_eq!(tcx.mk_ty(TyKind::Bool), tcx.types.bool);
        assert_ne!(a, tcx.mk_tup(&[tcx.types.char, tcx.types.bool]));
    });
}

#[test]
fn exception_2_string_forms() {
    let src = format!("{PRELUDE}\nfunc f(a: string, b: &string, c: &mut string) {{}}");
    with_tcx(&src, |tcx, diags| {
        assert_eq!(diags(), "");
        let sig = tcx.fn_sig(def_named(tcx, "f")).skip_binder();
        let string = tcx.lang_items().get(LangItem::String).unwrap();
        let [a, b, c] = sig.inputs() else { panic!() };
        assert!(matches!(*a.kind(), TyKind::Adt(d, _) if d.did() == string));
        assert!(matches!(*b.kind(), TyKind::Ref(_, t, Mutability::Not) if t == tcx.types.str_));
        assert!(matches!(*c.kind(), TyKind::Ref(_, t, Mutability::Mut) if t == *a));
    });
}

#[test]
fn exception_3_class_uses_are_wrapped() {
    let src = format!("{PRELUDE}\nclass Counter {{ value: int64 }}\nfunc f(c: Counter) {{}}");
    with_tcx(&src, |tcx, diags| {
        assert_eq!(diags(), "");
        let counter = def_named(tcx, "Counter");
        let class = tcx.lang_items().get(LangItem::Class).unwrap();
        let input = tcx.fn_sig(def_named(tcx, "f")).skip_binder().inputs()[0];
        let TyKind::Adt(outer, args) = *input.kind() else { panic!("{input:?}") };
        assert_eq!(outer.did(), class);
        assert!(matches!(*args[0].expect_ty().kind(), TyKind::Adt(inner, _) if inner.did() == counter && inner.is_class()));
        // The class's own type is the struct.
        assert!(matches!(*tcx.type_of(counter).kind(), TyKind::Adt(d, _) if d.did() == counter));
    });
}

#[test]
fn exception_13_several_origins_make_a_joined_region() {
    with_tcx("func longest(a: &int64, b: &int64) -> &[a, b] int64 { a }", |tcx, diags| {
        assert_eq!(diags(), "");
        let sig = tcx.fn_sig(def_named(tcx, "longest"));
        assert_eq!(sig.bound_vars.len(), 3);
        assert_eq!(sig.bound_vars[2].outlived_by, &[0, 1]);
        let TyKind::Ref(r, ..) = *sig.skip_binder().output().kind() else { panic!() };
        assert_eq!(*r.kind(), RegionKind::ReBound(DebruijnIndex(0), BoundRegion { var: 2 }));
    });
}

#[test]
fn exception_13_elision_and_naming_share_the_input_region() {
    with_tcx("func first(a: &int64) -> &int64 { a }\nfunc named(x: &int64, y: &int64) -> &[y] int64 { y }", |tcx, diags| {
        assert_eq!(diags(), "");
        for (name, input) in [("first", 0), ("named", 1)] {
            let sig = tcx.fn_sig(def_named(tcx, name)).skip_binder();
            fn region(t: Ty<'_>) -> Region<'_> {
                match *t.kind() {
                    TyKind::Ref(r, ..) => r,
                    _ => panic!(),
                }
            }
            assert_eq!(region(sig.output()), region(sig.inputs()[input]), "{name}");
        }
        assert_eq!(tcx.fn_sig_to_string(tcx.fn_sig(def_named(tcx, "named"))), "func(&int64, &[y] int64) -> &[y] int64");
    });
}

#[test]
fn exception_13_regions_shift_inside_function_types() {
    let src = format!("{PRELUDE}\nfunc g(a: &int64, f: func(&[a] int64)) {{}}");
    with_tcx(&src, |tcx, diags| {
        assert_eq!(diags(), "");
        let sig = tcx.fn_sig(def_named(tcx, "g")).skip_binder();
        let TyKind::Adt(_, args) = *sig.inputs()[1].kind() else { panic!() };
        let TyKind::FnPtr(inner) = *args[0].expect_ty().kind() else { panic!() };
        let TyKind::Ref(r, ..) = *inner.skip_binder().inputs()[0].kind() else { panic!() };
        // One binder out: the outer signature's variable 0.
        assert_eq!(*r.kind(), RegionKind::ReBound(DebruijnIndex(1), BoundRegion { var: 0 }));
    });
}

#[test]
fn aliases_are_instantiated() {
    with_tcx("type Pair<T> = (T, T)\nfunc f(p: Pair<int32>) {}", |tcx, diags| {
        assert_eq!(diags(), "");
        let input = tcx.fn_sig(def_named(tcx, "f")).skip_binder().inputs()[0];
        let i32_ = tcx.mk_ty(TyKind::Int(IntTy::I32));
        assert_eq!(input, tcx.mk_tup(&[i32_, i32_]));
    });
}

#[test]
fn generic_parameters_are_indexed_after_their_parents() {
    let src = "struct S<T> { t: T }\nimplement<T> S<T> { func m<U, V>(u: U, v: V, t: T) {} }";
    with_tcx(src, |tcx, diags| {
        assert_eq!(diags(), "");
        let m = def_named(tcx, "m");
        let g = tcx.generics_of(m);
        assert_eq!((g.parent_count, g.own_params.len()), (1, 2));
        let sig = tcx.fn_sig(m).skip_binder();
        let indices: Vec<u32> = sig
            .inputs()
            .iter()
            .map(|t| match *t.kind() {
                TyKind::Param(p) => p.index,
                _ => panic!(),
            })
            .collect();
        assert_eq!(indices, vec![1, 2, 0]);
    });
}

#[test]
fn associated_types_project_through_bounds() {
    let src = "trait It { type Item }\nfunc f<T: It>(x: T.Item) {}";
    with_tcx(src, |tcx, diags| {
        assert_eq!(diags(), "");
        let input = tcx.fn_sig(def_named(tcx, "f")).skip_binder().inputs()[0];
        let TyKind::Alias(a) = *input.kind() else { panic!("{input:?}") };
        assert_eq!(a.def_id, def_named(tcx, "Item"));
        assert!(matches!(*a.self_ty().kind(), TyKind::Param(p) if p.index == 0));
    });
}
