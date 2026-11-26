//! Symbol name mangling (`rustc_symbol_mangling`).
//!
//! Respects unmangled attributes (`@unsafe(exportName = "...")`, `@unsafe(noMangle)`,
//! `@linkName`, and foreign items) and generates deterministic, linker-safe
//! mangled names for Vertex functions, monomorphizations, closures, and drop glue.

use std::fmt::Write as _;

use vsc_hir::def::DefKind;
use vsc_middle::query::Providers;
use vsc_middle::ty::{
    GenericArg, Instance, InstanceKind, Mutability, SymbolName, Ty, TyCtxt, TyKind,
};
use vsc_span::def_id::{CRATE_DEF_ID, DefId};

pub fn provide(providers: &mut Providers) {
    providers.symbol_name = symbol_name;
}

fn symbol_name<'tcx>(tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> SymbolName<'tcx> {
    let attrs = tcx.codegen_instance_attrs(instance.def);
    if matches!(instance.def, InstanceKind::Item(_))
        && let Some(sym) = attrs.symbol_name
    {
        return SymbolName::new(tcx, sym.as_str());
    }

    let mut hash = Fnv64::new();
    let mut out = String::from("_V");

    hash.write_str(tcx.crate_name.as_str());
    push_ident_seg(&mut out, tcx.crate_name.as_str());

    match instance.def {
        InstanceKind::DropGlue(_, inner_ty) => {
            hash.write_u8(b'D');
            out.push_str("$drop_in_place");
            if let Some(ty) = inner_ty {
                hash_ty(tcx, ty, &mut hash);
                out.push('_');
                encode_ty_hint(tcx, ty, &mut out);
            } else {
                hash.write_str("noop");
                out.push_str("_noop");
            }
        }
        InstanceKind::ClosureOnceShim { closure, .. } => {
            hash.write_u8(b'C');
            append_def_path(tcx, closure, &mut out, &mut hash);
            out.push_str("$call_once");
        }
        InstanceKind::FnPtrShim(def_id, ty) => {
            hash.write_u8(b'F');
            append_def_path(tcx, def_id, &mut out, &mut hash);
            hash_ty(tcx, ty, &mut hash);
            out.push_str("$fn_ptr");
        }
        InstanceKind::ReifyShim(def_id) => {
            hash.write_u8(b'R');
            append_def_path(tcx, def_id, &mut out, &mut hash);
            out.push_str("$reify");
        }
        InstanceKind::VTableShim(def_id) => {
            hash.write_u8(b'V');
            append_def_path(tcx, def_id, &mut out, &mut hash);
            out.push_str("$vtable");
        }
        InstanceKind::Virtual(def_id, idx) => {
            hash.write_u8(b'T');
            append_def_path(tcx, def_id, &mut out, &mut hash);
            hash.write_u64(idx as u64);
            let _ = write!(out, "$virt{idx}");
        }
        InstanceKind::Intrinsic(def_id) => {
            hash.write_u8(b'I');
            append_def_path(tcx, def_id, &mut out, &mut hash);
        }
        InstanceKind::Item(def_id) => {
            hash.write_u8(b'N');
            append_def_path(tcx, def_id, &mut out, &mut hash);
        }
    }

    for arg in instance.args {
        match *arg {
            GenericArg::Type(ty) => {
                hash.write_u8(b't');
                hash_ty(tcx, ty, &mut hash);
            }
            GenericArg::Region(_) => {
                hash.write_u8(b'r');
            }
            GenericArg::Const(c) => {
                hash.write_u8(b'c');
                hash_const(c, &mut hash);
            }
        }
    }

    let _ = write!(out, "$h{:016x}", hash.finish());
    SymbolName::new(tcx, &out)
}

fn append_def_path(tcx: TyCtxt<'_>, def_id: DefId, out: &mut String, hash: &mut Fnv64) {
    let Some(local) = def_id.as_local() else {
        hash.write_u32(def_id.krate.as_u32());
        hash.write_u32(def_id.index.as_u32());
        let _ = write!(out, "_ext_{}_{}", def_id.krate.as_u32(), def_id.index.as_u32());
        return;
    };

    let mut chain = Vec::new();
    let mut cur = local;
    loop {
        if cur == CRATE_DEF_ID {
            break;
        }
        let info = &tcx.defs[cur];
        chain.push((cur, info.kind, info.name));
        match info.parent {
            Some(p) => cur = p,
            None => break,
        }
    }
    chain.reverse();

    for (id, kind, name) in chain {
        hash.write_u32(id.local_def_index.as_u32());
        let s = name.as_str();
        if !s.is_empty() {
            hash.write_str(s);
            push_ident_seg(out, s);
        } else {
            let tag = match kind {
                DefKind::Impl { .. } => "impl",
                DefKind::Closure => "closure",
                _ => "anon",
            };
            hash.write_str(tag);
            push_ident_seg(out, tag);
        }
    }
}

fn push_ident_seg(out: &mut String, seg: &str) {
    out.push('_');
    for ch in seg.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
}

fn encode_ty_hint(tcx: TyCtxt<'_>, ty: Ty<'_>, out: &mut String) {
    match *ty.kind() {
        TyKind::Bool => out.push_str("bool"),
        TyKind::Char => out.push_str("char"),
        TyKind::Int(i) => push_ident_seg(out, &format!("{i:?}")),
        TyKind::Uint(u) => push_ident_seg(out, &format!("{u:?}")),
        TyKind::Float(f) => push_ident_seg(out, &format!("{f:?}")),
        TyKind::Adt(adt, _) => {
            if let Some(l) = adt.did().as_local() {
                push_ident_seg(out, tcx.defs[l].name.as_str());
            }
        }
        TyKind::Tuple(elems) => {
            let _ = write!(out, "tup{}", elems.len());
        }
        _ => out.push_str("ty"),
    }
}

fn hash_const(c: vsc_middle::ty::Const<'_>, hash: &mut Fnv64) {
    match c.kind() {
        vsc_middle::ty::ConstKind::Value(ty, v) => {
            hash.write_u8(1);
            hash.write_u128(*v);
            let _ = ty;
        }
        vsc_middle::ty::ConstKind::Param(param) => {
            hash.write_u8(2);
            hash.write_u32(param.index);
        }
        vsc_middle::ty::ConstKind::Unevaluated(d, _) => {
            hash.write_u8(3);
            hash.write_u32(d.index.as_u32());
        }
        vsc_middle::ty::ConstKind::Error => hash.write_u8(5),
    }
}

fn hash_ty<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>, hash: &mut Fnv64) {
    match *ty.kind() {
        TyKind::Bool => hash.write_u8(1),
        TyKind::Char => hash.write_u8(2),
        TyKind::Int(i) => {
            hash.write_u8(3);
            hash.write_u8(i as u8);
        }
        TyKind::Uint(u) => {
            hash.write_u8(4);
            hash.write_u8(u as u8);
        }
        TyKind::Float(f) => {
            hash.write_u8(5);
            hash.write_u8(f as u8);
        }
        TyKind::Str => hash.write_u8(6),
        TyKind::Never => hash.write_u8(7),
        TyKind::RawPtr(inner, m) => {
            hash.write_u8(if m == Mutability::Mut { 8 } else { 9 });
            hash_ty(tcx, inner, hash);
        }
        TyKind::Ref(_, inner, m) => {
            hash.write_u8(if m == Mutability::Mut { 10 } else { 11 });
            hash_ty(tcx, inner, hash);
        }
        TyKind::Tuple(elems) => {
            hash.write_u8(12);
            hash.write_u64(elems.len() as u64);
            for &e in elems {
                hash_ty(tcx, e, hash);
            }
        }
        TyKind::Array(elem, len) => {
            hash.write_u8(13);
            hash_ty(tcx, elem, hash);
            hash_const(len, hash);
        }
        TyKind::Slice(elem) => {
            hash.write_u8(14);
            hash_ty(tcx, elem, hash);
        }
        TyKind::Adt(adt, args) => {
            hash.write_u8(15);
            hash.write_u32(adt.did().krate.as_u32());
            hash.write_u32(adt.did().index.as_u32());
            for arg in args {
                match *arg {
                    GenericArg::Type(t) => {
                        hash.write_u8(b'T');
                        hash_ty(tcx, t, hash);
                    }
                    GenericArg::Const(c) => {
                        hash.write_u8(b'C');
                        hash_const(c, hash);
                    }
                    GenericArg::Region(_) => hash.write_u8(b'R'),
                }
            }
        }
        TyKind::FnDef(d, args) | TyKind::Closure(d, args) => {
            hash.write_u8(16);
            hash.write_u32(d.krate.as_u32());
            hash.write_u32(d.index.as_u32());
            for arg in args {
                if let GenericArg::Type(t) = *arg {
                    hash_ty(tcx, t, hash);
                }
            }
        }
        TyKind::FnPtr(sig) => {
            hash.write_u8(17);
            let s = sig.skip_binder();
            hash.write_u64(s.inputs_and_output.len() as u64);
            for &t in s.inputs_and_output {
                hash_ty(tcx, t, hash);
            }
        }
        TyKind::Param(p) => {
            hash.write_u8(18);
            hash.write_u32(p.index);
        }
        TyKind::Foreign(d) => {
            hash.write_u8(19);
            hash.write_u32(d.krate.as_u32());
            hash.write_u32(d.index.as_u32());
        }
        TyKind::Coroutine(d, args) => {
            hash.write_u8(21);
            hash.write_u32(d.krate.as_u32());
            hash.write_u32(d.index.as_u32());
            for a in args {
                if let GenericArg::Type(t) = a {
                    hash_ty(tcx, *t, hash);
                }
            }
        }
        TyKind::Opaque(d, args) => {
            hash.write_u8(22);
            hash.write_u32(d.krate.as_u32());
            hash.write_u32(d.index.as_u32());
            for a in args {
                if let GenericArg::Type(t) = a {
                    hash_ty(tcx, *t, hash);
                }
            }
        }
        TyKind::Alias(alias) => {
            hash.write_u8(23);
            hash.write_u32(alias.def_id.krate.as_u32());
            hash.write_u32(alias.def_id.index.as_u32());
            for a in alias.args {
                match *a {
                    GenericArg::Type(t) => {
                        hash.write_u8(b'T');
                        hash_ty(tcx, t, hash);
                    }
                    GenericArg::Const(c) => {
                        hash.write_u8(b'C');
                        hash_const(c, hash);
                    }
                    GenericArg::Region(_) => hash.write_u8(b'R'),
                }
            }
        }
        TyKind::Dynamic(..) | TyKind::Infer(_) | TyKind::Error => {
            hash.write_u8(20);
        }
    }
}

struct Fnv64(u64);

impl Fnv64 {
    fn new() -> Self {
        Fnv64(0xcbf2_9ce4_8422_2325)
    }

    fn write_u8(&mut self, b: u8) {
        self.0 ^= b as u64;
        self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
    }

    fn write_u32(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.write_u8(b);
        }
    }

    fn write_u64(&mut self, v: u64) {
        for b in v.to_le_bytes() {
            self.write_u8(b);
        }
    }

    fn write_u128(&mut self, v: u128) {
        for b in v.to_le_bytes() {
            self.write_u8(b);
        }
    }

    fn write_str(&mut self, s: &str) {
        self.write_u64(s.len() as u64);
        for &b in s.as_bytes() {
            self.write_u8(b);
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use std::cell::OnceCell;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use vsc_ast_lowering::lower_to_hir;
    use vsc_errors::SharedBuffer;
    use vsc_index::IndexVec;
    use vsc_middle::arena::Arena;
    use vsc_middle::query::Providers;
    use vsc_middle::ty::{CtxtInputs, DefInfo, GenericArg, Instance, TyCtxt};
    use vsc_resolve::resolve_crate;
    use vsc_session::config::{TargetInfo, default_configuration};
    use vsc_session::parse::ParseSess;
    use vsc_span::{Symbol, kw};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn mangles_distinct_monomorphizations_and_preserves_exports() {
        let src = r#"
@unsafe(exportName = "c_add")
func exported_add(a: int32, b: int32) -> int32 { a + b }

func id<T>(x: T) -> T { x }
func main() {}
"#;
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("vsc_sym_test_{}_{}", std::process::id(), n));
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
        let _ = std::fs::remove_dir_all(&dir);

        let exp_def = tcx
            .defs
            .iter_enumerated()
            .find(|(_, d)| d.name.as_str() == "exported_add")
            .unwrap()
            .0
            .to_def_id();
        assert_eq!(tcx.symbol_name(Instance::mono(tcx, exp_def)).name, "c_add");

        let id_def = tcx
            .defs
            .iter_enumerated()
            .find(|(_, d)| d.name.as_str() == "id")
            .unwrap()
            .0
            .to_def_id();
        let s_i32 = tcx.symbol_name(Instance::new_raw(
            id_def,
            tcx.mk_args(&[GenericArg::Type(tcx.types.i32)]),
        ));
        let s_bool = tcx.symbol_name(Instance::new_raw(
            id_def,
            tcx.mk_args(&[GenericArg::Type(tcx.types.bool)]),
        ));
        assert_ne!(s_i32.name, s_bool.name);
        assert!(s_i32.name.starts_with("_V"));
    }
}
