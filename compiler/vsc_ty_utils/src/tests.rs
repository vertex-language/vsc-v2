use std::cell::OnceCell;
use std::sync::atomic::{AtomicUsize, Ordering};

use vsc_abi::{BackendRepr, ExternAbi};
use vsc_ast_lowering::lower_to_hir;
use vsc_errors::SharedBuffer;
use vsc_hir::LangItem;
use vsc_index::IndexVec;
use vsc_middle::arena::Arena;
use vsc_middle::query::Providers;
use vsc_middle::ty::{CtxtInputs, DefInfo, TyCtxt};
use vsc_passes::entry::has_no_main_attr;
use vsc_resolve::resolve_crate;
use vsc_session::config::{TargetInfo, default_configuration};
use vsc_session::parse::ParseSess;
use vsc_span::{Symbol, kw};
use vsc_target::callconv::PassMode;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn check_layout(src: &str, f: impl for<'tcx> FnOnce(TyCtxt<'tcx>)) {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("vsc_ty_utils_test_{}_{}", std::process::id(), n));
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
    let _ = std::fs::remove_dir_all(&dir);
    f(tcx);
    assert_eq!(psess.dcx.err_count(), 0, "unexpected compile errors: {}", buf.contents());
    let _ = (ExternAbi::C { unwind: false }, PassMode::Ignore, has_no_main_attr(&[]));
}

#[test]
fn primitive_and_c_struct_layouts() {
    let src = r#"
@repr(c)
struct Point {
    x: int32
    y: int64
}

func add_pts(a: Point) -> int64 { a.y }
func main() {}
"#;
    check_layout(src, |tcx| {
        assert_eq!(tcx.layout_of(tcx.types.bool).unwrap().size.bytes(), 1);
        assert_eq!(tcx.layout_of(tcx.types.i32).unwrap().size.bytes(), 4);
        assert_eq!(tcx.layout_of(tcx.types.i64).unwrap().size.bytes(), 8);

        let point_def = tcx
            .defs
            .iter_enumerated()
            .find(|(_, d)| d.name.as_str() == "Point")
            .unwrap()
            .0
            .to_def_id();
        let point_ty = tcx.type_of(point_def);
        let point_layout = tcx.layout_of(point_ty).unwrap();
        assert_eq!(point_layout.size.bytes(), 16);
        assert_eq!(point_layout.align.bytes(), 8);
        assert!(point_layout.is_repr_c());

        let main_fn = tcx.entry_fn(()).expect("main should be discovered");
        assert_eq!(tcx.item_name(main_fn.0).as_str(), "main");

        let add_pts = tcx
            .defs
            .iter_enumerated()
            .find(|(_, d)| d.name.as_str() == "add_pts")
            .unwrap()
            .0
            .to_def_id();
        let abi = tcx
            .fn_abi_of_instance((vsc_middle::ty::Instance::mono(tcx, add_pts), &[]))
            .unwrap();
        assert_eq!(abi.args.len(), 1);
        assert!(matches!(abi.ret.layout.backend_repr, BackendRepr::Scalar(_)));
    });
}

#[test]
fn class_and_export_attrs() {
    let src = r#"
@lang("Class")
struct Class<T> { inner: T }

class Node {
    val: int32
}

@unsafe(exportName = "vx_export_fn")
@inline(always)
func exported(n: Node) -> int32 { n.val }
"#;
    check_layout(src, |tcx| {
        let exported = tcx
            .defs
            .iter_enumerated()
            .find(|(_, d)| d.name.as_str() == "exported")
            .unwrap()
            .0
            .to_def_id();
        let attrs = tcx.codegen_fn_attrs(exported);
        assert_eq!(attrs.symbol_name.unwrap().as_str(), "vx_export_fn");

        let sig = tcx.fn_sig(exported).value;
        let class_ty = sig.inputs()[0];
        let class_layout = tcx.layout_of(class_ty).unwrap();
        assert_eq!(
            class_layout.size.bytes(),
            tcx.data_layout.pointer_size().bytes(),
            "Class<T> must lay out as a single pointer, not recursive value"
        );
        let _ = LangItem::Class;
    });
}
