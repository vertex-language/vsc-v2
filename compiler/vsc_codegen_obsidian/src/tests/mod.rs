//! Integration tests verifying `vsc_codegen_obsidian` lowers MIR into `.obs` IR, emits native object files, and links executables.

mod runtime_and_async;

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::process::Command;

use vsc_ast_lowering::lower_to_hir;
use vsc_codegen_ssa::traits::CodegenBackend;
use vsc_codegen_ssa::{CompiledModules, CrateInfo};
use vsc_errors::SharedBuffer;
use vsc_expand::{ExpansionConfig, expand_crate_with_externs};
use vsc_index::IndexVec;
use vsc_middle::arena::Arena;
use vsc_middle::query::Providers;
use vsc_middle::ty::{CtxtInputs, DefInfo, TyCtxt};
use vsc_resolve::resolve_crate;
use vsc_session::config::{
    OptLevel, Options, OutFileName, OutputFilenames, OutputType, OutputTypes, default_configuration,
};
use vsc_session::{ParseSess, Session};
use vsc_span::kw;

use crate::ObsidianCodegenBackend;

fn make_providers() -> Providers {
    let mut p = Providers::default();
    vsc_passes::provide(&mut p);
    vsc_hir_analysis::provide(&mut p);
    vsc_hir_typeck::provide(&mut p);
    vsc_mir_build::provide(&mut p);
    vsc_mir_transform::provide(&mut p);
    vsc_ty_utils::provide(&mut p);
    vsc_borrowck::provide(&mut p);
    vsc_symbol_mangling::provide(&mut p);
    vsc_monomorphize::provide(&mut p);
    p
}

fn compile_and_run_binary(src: &str, name: &str) -> (i32, String, String, String) {
    compile_and_run_binary_with_opt(src, name, OptLevel::Default)
}

fn compile_and_run_binary_with_opt(
    src: &str,
    name: &str,
    opt_level: OptLevel,
) -> (i32, String, String, String) {
    let dir =
        std::env::temp_dir().join(format!("vsc_obsidian_test_{}_{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join(format!("{name}.vs"));
    let exe_path = dir.join(name);
    let ir_path = dir.join(format!("{name}.obs"));
    std::fs::write(&src_path, src).unwrap();

    let buf = SharedBuffer::new();
    let psess = ParseSess::with_buffer(buf.clone());

    let mut opts = Options::default();
    opts.crate_name = Some(name.to_string());
    opts.cg.opt_level = opt_level;
    opts.output_types = OutputTypes(BTreeMap::from([
        (OutputType::Exe, None),
        (OutputType::ObsIr, Some(OutFileName::Real(ir_path.clone()))),
    ]));

    let sess = Session::new(opts, psess).expect("initialize Session");
    let cfg = default_configuration(&sess.target_info, true, false);
    let ecfg = ExpansionConfig {
        crate_name: Some(name),
        cfg: &cfg,
    };
    let core_lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("library/core/src/lib.vs");
    let krate = expand_crate_with_externs(&sess.psess, &src_path, &ecfg, &[], Some(&core_lib))
        .expect("expand crate");
    let outputs = resolve_crate(&sess.psess, &krate);
    let hir = lower_to_hir(&sess.psess, &krate, &outputs);
    assert!(
        sess.dcx().has_errors().is_none(),
        "frontend errors in {name}:\n{}",
        buf.contents()
    );

    let defs = IndexVec::from_raw(
        outputs
            .defs
            .iter()
            .map(|d| DefInfo {
                kind: d.kind,
                parent: d.parent,
                name: d.name.unwrap_or(kw::Empty),
                span: d.span,
            })
            .collect(),
    );

    let arena = Arena::default();
    let gcx = OnceCell::new();
    let tcx = TyCtxt::create_with_options(
        &arena,
        &gcx,
        CtxtInputs {
            sess: &sess.psess,
            target: &sess.target_info,
            hir: &hir,
            defs,
            crate_name: outputs.crate_name,
            providers: make_providers(),
        },
        sess.opts.clone(),
    );

    vsc_hir_analysis::check_crate(tcx);
    assert!(
        tcx.dcx().has_errors().is_none(),
        "typecheck errors in {name}:\n{}",
        buf.contents()
    );
    vsc_borrowck::check_crate(tcx);
    assert!(
        tcx.dcx().has_errors().is_none(),
        "borrowck errors in {name}:\n{}",
        buf.contents()
    );
    vsc_passes::entry::check_entry_fn(tcx);
    assert!(
        tcx.dcx().has_errors().is_none(),
        "entry check errors in {name}:\n{}",
        buf.contents()
    );

    let backend = ObsidianCodegenBackend::new();
    let ongoing = backend.codegen_crate(tcx);
    let crate_info = CrateInfo::new(tcx);

    let out_files = OutputFilenames {
        out_directory: dir.clone(),
        filestem: name.to_string(),
        single_output_file: Some(OutFileName::Real(exe_path.clone())),
        temps_directory: Some(dir.clone()),
        outputs: sess.opts.output_types.clone(),
        extra: String::new(),
    };
    let compiled: CompiledModules = backend.join_codegen(ongoing, &sess, &out_files, &crate_info);
    backend.link(&sess, compiled, crate_info, &out_files);

    let ir = std::fs::read_to_string(&ir_path).expect("Obsidian IR emitted");
    assert!(
        ir.contains("fn @"),
        "expected Obsidian function definitions in IR:\n{ir}"
    );

    let run = Command::new(&exe_path)
        .output()
        .expect("execute compiled binary");
    let code = run.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&dir);
    (code, stdout, stderr, ir)
}

#[test]
fn compiles_standalone_structs_enums_loops_and_c_ffi() {
    let src = r#"
package main

unsafe extern "C" {
    safe func write(fd: int32, buf: *const uint8, count: uint) -> int
}

struct Point { x: int64; y: int64 }
enum MaybeInt { None; Some(int64) }

static Magic: int64 = 5

func main() -> int32 {
    let p = Point { x: 10, y: 19 }
    var sum = p.x + p.y

    let m = MaybeInt.Some(8)
    let extra = match m {
        MaybeInt.Some(v) => v
        MaybeInt.None => 0
    }
    sum = sum + extra + Magic

    let msg: [uint8; 4] = [111, 107, 33, 10]
    write(1, &msg[0] as *const uint8, 4)

    if sum == 42 { 42 } else { 1 }
}
"#;
    let (code, stdout, stderr, _ir) = compile_and_run_binary(src, "standalone");
    assert_eq!(stdout, "ok!\n", "unexpected stdout, stderr: {stderr}");
    assert_eq!(code, 42, "stderr: {stderr}");
}

#[test]
fn compiles_closures_classes_and_func_values() {
    let src = r#"
package main

@lang("Class")
public struct Class<T> { inner: T }

@lang("Func")
public struct Func<F> { f: F }

@lang("fnOnce")
public trait FnOnce<Args> {
    type Output
    func callOnce(self, args: Args) -> Self.Output
}

@lang("fnMut")
public trait FnMut<Args>: FnOnce<Args> {
    func callMut(&mut self, args: Args) -> <Self as FnOnce<Args>>.Output
}

@lang("fn")
public trait Fn<Args>: FnMut<Args> {
    func call(&self, args: Args) -> <Self as FnOnce<Args>>.Output
}

unsafe extern "C" {
    safe func write(fd: int32, buf: *const uint8, count: uint) -> int
}

struct Point { x: int64; y: int64 }
enum MaybeInt { None; Some(int64) }
class Counter { value: int64 }

func apply_fn(f: func(int64) -> int64, v: int64) -> int64 {
    f(v)
}

func add5_fn(x: int64) -> int64 {
    x + 5
}

func make_adder() -> func(int64) -> int64 {
    func(x: int64) -> int64 { add5_fn(x) }
}

func main() -> int32 {
    let p = Point { x: 10, y: 32 }
    var sum = p.x + p.y

    let m = MaybeInt.Some(8)
    let extra = match m {
        MaybeInt.Some(v) => v
        MaybeInt.None => 0
    }
    sum = sum + extra

    let k: int64 = 1
    let add5 = make_adder()
    let total = apply_fn(add5, sum) + apply_fn(func(x: int64) -> int64 { x + k }, 0) - 1

    var c = Counter { value: total }
    c.value = c.value + 1

    let msg: [uint8; 4] = [111, 107, 33, 10]
    write(1, &msg[0] as *const uint8, 4)

    if c.value == 56 { 42 } else { 1 }
}
"#;
    let (code, stdout, stderr, _ir) = compile_and_run_binary(src, "closures_classes");
    assert_eq!(stdout, "ok!\n", "unexpected stdout, stderr: {stderr}");
    assert_eq!(code, 42, "stderr: {stderr}");
}

#[test]
fn compiles_byte_and_raw_string_literals() {
    let src = r#"
package main

unsafe extern "C" {
    func write(fd: int32, buf: *const uint8, len: uint) -> int64
}

func main() -> int32 {
    let msg: &[uint8; 10] = b"native-ok\n"
    let raw_bytes: &[uint8; 4] = br"a\nb"
    let raw_str: &string = r"x\ny"
    let hi: &[uint8; 1] = b"\xFF"
    let raw_str_ok = match raw_str {
        r"x\ny" => true
        _ => false
    }
    let raw_str_ne_esc = match raw_str {
        "x\ny" => false
        _ => true
    }
    unsafe {
        write(1, &msg[0] as *const uint8, 10)
        if raw_bytes[0] == 97 && raw_bytes[1] == 92 && raw_bytes[2] == 110 && raw_bytes[3] == 98 && raw_str_ok && raw_str_ne_esc && hi[0] == 255 {
            return 7
        }
    }
    1
}
"#;
    let (code, stdout, stderr, _ir) = compile_and_run_binary(src, "byte_and_raw_strings");
    assert_eq!(stdout, "native-ok\n", "unexpected stdout, stderr: {stderr}");
    assert_eq!(code, 7, "stderr: {stderr}");
}

#[test]
fn compiles_floats_divrem_shifts_and_switches() {
    let src = r#"
package main

func classify(x: int64) -> int64 {
    match x {
        1 => 10
        2 => 20
        3 => 30
        _ => 99
    }
}

func compute_fp(a: float64, b: float64) -> float64 {
    (a * b) + (a / 2.0) - 1.5
}

func main() -> int32 {
    let sw = classify(2) + classify(3)
    let sh = (1 << 4) + (64 >> 3)
    let dv = (100 / 7) + (100 % 7)
    let fp = compute_fp(6.0, 2.5)
    let fp_ok = if fp > 16.4 && fp < 16.6 { 1 } else { 0 }
    let fp_int = fp as int64
    let total = sw + sh + dv + fp_int + fp_ok
    if total == 107 { 42 } else { 1 }
}
"#;
    let (code, _stdout, stderr, _ir) = compile_and_run_binary(src, "floats_divrem_switches");
    assert_eq!(code, 42, "stderr: {stderr}");
}

#[test]
fn compiles_int128_and_uint128_arithmetic_shifts_divrem_and_calls() {
    let src = r#"
package main

func add128(a: uint128, b: uint128) -> uint128 {
    a + b
}

func signed_divrem(a: int128, b: int128) -> int128 {
    (a / b) + (a % b)
}

func main() -> int32 {
    let one: uint128 = 1
    let big: uint128 = one << 80
    let big2: uint128 = add128(big, 1000)
    let diff: uint128 = big2 - big
    let shifted_back: uint128 = big >> 80

    // 128-bit multiplication crossing the 64-bit boundary:
    let a64: uint128 = 1000000000000
    let b64: uint128 = 1000000000000
    let prod: uint128 = a64 * b64
    let quot: uint128 = prod / 1000000000000
    let rem: uint128 = (prod + 77) % 1000000000000

    // Signed 128-bit division/remainder & negation:
    let zero_i: int128 = 0
    let neg_big: int128 = -((1 as int128) << 70) - 9
    let pos_big: int128 = -neg_big
    let d: int128 = 10
    let sdr: int128 = signed_divrem(-100 as int128, 7 as int128) // -14 + (-2) = -16
    let neg_rem: int128 = neg_big % d
    let neg_ok = (neg_big < zero_i) && (pos_big > zero_i) && (neg_rem < zero_i)

    if diff == 1000
        && shifted_back == 1
        && quot == 1000000000000
        && rem == 77
        && sdr == (-16 as int128)
        && neg_ok
    {
        42
    } else {
        1
    }
}
"#;
    let (code, _stdout, stderr, _ir) = compile_and_run_binary(src, "int128_full");
    assert_eq!(code, 42, "stderr: {stderr}");
}

#[test]
fn compiles_variadic_c_ffi_snprintf() {
    let src = r#"
package main

unsafe extern "C" {
    func snprintf(buf: *mut uint8, cap: uint, fmt: *const uint8, ...) -> int32
    func puts(s: *const uint8) -> int32
}

func main() -> int32 {
    var buf: [uint8; 64] = [0; 64]
    let fmt: &[uint8; 17] = b"v=%d f=%.1f s=%s\0"
    let tag: &[uint8; 4] = b"vsc\0"
    let f: float64 = 2.5
    unsafe {
        let n = snprintf(
            &mut buf[0] as *mut uint8,
            64,
            &fmt[0] as *const uint8,
            42 as int32,
            f,
            &tag[0] as *const uint8,
        )
        puts(&buf[0] as *const uint8)
        n
    }
}
"#;
    let (code, stdout, stderr, _ir) = compile_and_run_binary(src, "variadic_snprintf");
    assert_eq!(stdout, "v=42 f=2.5 s=vsc\n", "unexpected stdout, stderr: {stderr}");
    assert_eq!(code, 16, "stderr: {stderr}");
}

#[test]
fn compiles_object_files_for_all_six_host_targets() {
    let src = r#"
package main

unsafe extern "C" {
    func snprintf(buf: *mut uint8, cap: uint, fmt: *const uint8, ...) -> int32
}

func wide_calc(a: uint128, b: float64) -> int64 {
    let c = (a * 3 + 7) / 2
    (c as int64) + (b as int64)
}

func main() -> int32 {
    var buf: [uint8; 32] = [0; 32]
    let fmt: &[uint8; 4] = b"%d\0\0"
    let w = wide_calc(100, 5.5)
    unsafe {
        snprintf(&mut buf[0] as *mut uint8, 32, &fmt[0] as *const uint8, w as int32)
    }
}
"#;
    let targets = [
        ("aarch64-apple-darwin", &b"\xCF\xFA\xED\xFE"[..]),
        ("x86_64-apple-darwin", &b"\xCF\xFA\xED\xFE"[..]),
        ("x86_64-unknown-linux-gnu", &b"\x7FELF"[..]),
        ("aarch64-unknown-linux-gnu", &b"\x7FELF"[..]),
        ("x86_64-pc-windows-msvc", &[0x64, 0x86][..]),
        ("aarch64-pc-windows-msvc", &[0x64, 0xAA][..]),
    ];

    for (triple, expected_magic) in targets {
        let dir = std::env::temp_dir().join(format!(
            "vsc_obsidian_target_{}_{}",
            triple.replace('-', "_"),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src_path = dir.join("cross.vs");
        let obj_path = dir.join("cross.o");
        std::fs::write(&src_path, src).unwrap();

        let buf = SharedBuffer::new();
        let psess = ParseSess::with_buffer(buf.clone());
        let mut opts = Options::default();
        opts.crate_name = Some("cross".to_string());
        opts.target_triple = Some(triple.to_string());
        opts.cg.opt_level = OptLevel::Default;
        opts.output_types =
            OutputTypes(BTreeMap::from([(OutputType::Object, Some(OutFileName::Real(obj_path.clone())))]));

        let sess = Session::new(opts, psess).expect("initialize Session");
        let cfg = default_configuration(&sess.target_info, true, false);
        let ecfg = ExpansionConfig {
            crate_name: Some("cross"),
            cfg: &cfg,
        };
        let core_lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("library/core/src/lib.vs");
    let krate = expand_crate_with_externs(&sess.psess, &src_path, &ecfg, &[], Some(&core_lib))
        .expect("expand crate");
        let outputs = resolve_crate(&sess.psess, &krate);
        let hir = lower_to_hir(&sess.psess, &krate, &outputs);
        assert!(sess.dcx().has_errors().is_none(), "frontend errors for {triple}:\n{}", buf.contents());

        let defs = IndexVec::from_raw(
            outputs
                .defs
                .iter()
                .map(|d| DefInfo {
                    kind: d.kind,
                    parent: d.parent,
                    name: d.name.unwrap_or(kw::Empty),
                    span: d.span,
                })
                .collect(),
        );
        let arena = Arena::default();
        let gcx = OnceCell::new();
        let tcx = TyCtxt::create_with_options(
            &arena,
            &gcx,
            CtxtInputs {
                sess: &sess.psess,
                target: &sess.target_info,
                hir: &hir,
                defs,
                crate_name: outputs.crate_name,
                providers: make_providers(),
            },
            sess.opts.clone(),
        );
        vsc_hir_analysis::check_crate(tcx);
        assert!(tcx.dcx().has_errors().is_none(), "typecheck errors for {triple}:\n{}", buf.contents());
        vsc_borrowck::check_crate(tcx);
        vsc_passes::entry::check_entry_fn(tcx);

        let backend = ObsidianCodegenBackend::new();
        let ongoing = backend.codegen_crate(tcx);
        let crate_info = CrateInfo::new(tcx);
        let out_files = OutputFilenames {
            out_directory: dir.clone(),
            filestem: "cross".to_string(),
            single_output_file: None,
            temps_directory: Some(dir.clone()),
            outputs: sess.opts.output_types.clone(),
            extra: String::new(),
        };
        let compiled = backend.join_codegen(ongoing, &sess, &out_files, &crate_info);
        let first_obj = compiled.modules[0].object.as_ref().expect("object emitted");
        let bytes = std::fs::read(first_obj).expect("read emitted object file");
        assert!(
            bytes.starts_with(expected_magic),
            "target {triple}: unexpected object magic {:02X?}",
            &bytes[..expected_magic.len().min(bytes.len())]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

