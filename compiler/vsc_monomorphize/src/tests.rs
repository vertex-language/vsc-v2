use std::cell::OnceCell;
use std::sync::atomic::{AtomicUsize, Ordering};

use vsc_ast_lowering::lower_to_hir;
use vsc_errors::SharedBuffer;
use vsc_index::IndexVec;
use vsc_middle::arena::Arena;
use vsc_middle::mono::MonoItem;
use vsc_middle::query::Providers;
use vsc_middle::ty::{CtxtInputs, DefInfo, TyCtxt};
use vsc_resolve::resolve_crate;
use vsc_session::config::{TargetInfo, default_configuration};
use vsc_session::parse::ParseSess;
use vsc_span::{Symbol, kw};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn collects_reachable_monomorphizations_and_drops_unreachable() {
    let src = r#"
func helper<T>(x: T) -> T { x }
func unused_dead() -> int32 { 999 }

func main() {
    let a = helper(42)
    let b = helper(true)
    let _ = (a, b)
}
"#;
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("vsc_mono_test_{}_{}", std::process::id(), n));
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
    vsc_mir_transform::provide(&mut providers);
    vsc_ty_utils::provide(&mut providers);
    vsc_symbol_mangling::provide(&mut providers);
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

    let cgus = tcx.collect_and_partition_mono_items(());
    assert_eq!(cgus.len(), 1);
    let names: Vec<&str> = cgus[0]
        .items()
        .keys()
        .map(|item| match item {
            MonoItem::Fn(inst) => tcx.symbol_name(*inst).name,
            _ => "",
        })
        .collect();
    // `main` + `helper::<int32>` + `helper::<bool>`, while `unused_dead` is omitted.
    assert_eq!(names.len(), 3, "collected items: {names:?}");
    assert!(!names.iter().any(|n| n.contains("unused_dead")));
}
