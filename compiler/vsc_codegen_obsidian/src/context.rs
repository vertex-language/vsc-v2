//! Per-CGU Obsidian codegen context and caches (`vsc_codegen_obsidian::context`).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use obsidian::ir;
use vsc_middle::middle::codegen_fn_attrs::{CodegenFnAttrFlags, Linkage};
use vsc_middle::mono::Visibility;
use vsc_middle::ty::layout::{LayoutCx, TyAndLayout};
use vsc_middle::ty::{GenericArgsRef, Instance, PolyFnSig, Ty, TyCtxt};
use vsc_span::def_id::DefId;
use vsc_target::callconv::FnAbi;

use crate::common::obsidian_target_for_spec;
use crate::value::{ObsFnSig, ObsType, ObsVal};

/// Owned `.obs` module produced for a single `CodegenUnit`.
pub struct OwnedObsModule {
    pub module: ir::Module,
}

/// Obsidian backend state scoped to one `CodegenUnit`.
pub struct CodegenCx<'tcx> {
    pub module: RefCell<ir::Module>,
    pub target: ir::Target,
    pub tcx: TyCtxt<'tcx>,
    pub layout_cx: LayoutCx<'tcx>,
    pub fn_sigs: RefCell<Vec<ObsFnSig>>,
    pub sym_fn_sigs: RefCell<HashMap<ir::Sym, u32>>,
    pub entry_block_claimed: RefCell<HashSet<ir::Sym>>,
    pub type_lowering: RefCell<HashMap<Ty<'tcx>, ObsType>>,
    pub scalar_type_lowering: RefCell<HashMap<Ty<'tcx>, ObsType>>,
    pub instances: RefCell<HashMap<Instance<'tcx>, ObsVal>>,
    pub statics: RefCell<HashMap<DefId, ObsVal>>,
    pub const_str_globals: RefCell<HashMap<String, ObsVal>>,
    pub fn_def_trampolines: RefCell<HashMap<(DefId, GenericArgsRef<'tcx>), ObsVal>>,
    pub fn_ptr_trampolines: RefCell<HashMap<Ty<'tcx>, ObsVal>>,
    pub zst_env_global: Cell<Option<ObsVal>>,
    pub panic_fn: Cell<Option<ObsVal>>,
    pub udivrem128_fn: Cell<Option<(ObsType, ObsVal)>>,
    pub next_const_id: Cell<usize>,
}

impl<'tcx> CodegenCx<'tcx> {
    /// Initializes an `.obs` module configured for `tcx.target_spec`.
    pub fn new(tcx: TyCtxt<'tcx>, cgu_name: &str) -> Self {
        let target = obsidian_target_for_spec(&tcx.target_spec);
        let module = ir::Module::new(cgu_name, target);
        Self {
            module: RefCell::new(module),
            target,
            tcx,
            layout_cx: LayoutCx::new(tcx),
            fn_sigs: RefCell::new(Vec::new()),
            sym_fn_sigs: RefCell::new(HashMap::new()),
            entry_block_claimed: RefCell::new(HashSet::new()),
            type_lowering: RefCell::new(HashMap::new()),
            scalar_type_lowering: RefCell::new(HashMap::new()),
            instances: RefCell::new(HashMap::new()),
            statics: RefCell::new(HashMap::new()),
            const_str_globals: RefCell::new(HashMap::new()),
            fn_def_trampolines: RefCell::new(HashMap::new()),
            fn_ptr_trampolines: RefCell::new(HashMap::new()),
            zst_env_global: Cell::new(None),
            panic_fn: Cell::new(None),
            udivrem128_fn: Cell::new(None),
            next_const_id: Cell::new(0),
        }
    }

    /// Consumes the context, returning [`OwnedObsModule`].
    pub fn into_module(self) -> OwnedObsModule {
        OwnedObsModule {
            module: self.module.into_inner(),
        }
    }

    /// Interns an [`ObsFnSig`] and returns its `u32` ID.
    pub fn intern_fn_sig(&self, sig: ObsFnSig) -> u32 {
        let mut sigs = self.fn_sigs.borrow_mut();
        if let Some(pos) = sigs.iter().position(|s| *s == sig) {
            return pos as u32;
        }
        let id = sigs.len() as u32;
        sigs.push(sig);
        id
    }

    /// Retrieves a cloned [`ObsFnSig`] by ID.
    pub fn get_fn_sig(&self, id: u32) -> ObsFnSig {
        self.fn_sigs.borrow()[id as usize].clone()
    }

    /// Mutably accesses an [`ir::Function`] inside `self.module` by its [`ir::Sym`].
    pub fn with_func_mut<R>(&self, func_sym: ir::Sym, f: impl FnOnce(&mut ir::Function) -> R) -> R {
        let mut module = self.module.borrow_mut();
        let ir::SymKind::Function(idx) = module.symbols[func_sym.index()].1 else {
            panic!("Sym {:?} is not a Function", func_sym);
        };
        f(&mut module.functions[idx])
    }

    /// Immutably accesses an [`ir::Function`] inside `self.module` by its [`ir::Sym`].
    pub fn with_func<R>(&self, func_sym: ir::Sym, f: impl FnOnce(&ir::Function) -> R) -> R {
        let module = self.module.borrow();
        let ir::SymKind::Function(idx) = module.symbols[func_sym.index()].1 else {
            panic!("Sym {:?} is not a Function", func_sym);
        };
        f(&module.functions[idx])
    }

    /// Returns parameter `idx` of function `llfn` (`Block::ENTRY` parameter `idx`).
    pub fn get_param(&self, llfn: ObsVal, idx: u32) -> ObsVal {
        let (func_sym, _, _) = llfn.as_sym().expect("get_param on non-function ObsVal");
        self.with_func(func_sym, |func| {
            let idx = idx as usize;
            assert!(
                idx < func.blocks[0].params.len(),
                "get_param({idx}) out of bounds for `{}` (len={})",
                func.name,
                func.blocks[0].params.len(),
            );
            let val = func.blocks[0].params[idx];
            let ty = func.param_types[idx];
            ObsVal::ssa(val, ty)
        })
    }

    /// Computes target layout for `ty`, failing fatally on layout errors.
    pub fn layout_of(&self, ty: Ty<'tcx>) -> TyAndLayout<'tcx> {
        match self.tcx.layout_of(ty) {
            Ok(layout) => layout,
            Err(err) => self
                .tcx
                .dcx()
                .fatal(format!("failed to compute layout for `{ty}`: {err:?}")),
        }
    }

    /// Computes ABI for monomorphized `instance`, failing fatally on errors.
    pub fn fn_abi_of_instance(&self, instance: Instance<'tcx>) -> &'tcx FnAbi<'tcx, Ty<'tcx>> {
        match self.tcx.fn_abi_of_instance((instance, &[])) {
            Ok(abi) => abi,
            Err(err) => self
                .tcx
                .dcx()
                .fatal(format!("failed to compute FnAbi for `{instance:?}`: {err:?}")),
        }
    }

    /// Computes ABI for function pointer signature `sig`, failing fatally on errors.
    pub fn fn_abi_of_fn_ptr(&self, sig: PolyFnSig<'tcx>) -> &'tcx FnAbi<'tcx, Ty<'tcx>> {
        match self.tcx.fn_abi_of_fn_ptr((sig, &[])) {
            Ok(abi) => abi,
            Err(err) => self
                .tcx
                .dcx()
                .fatal(format!("failed to compute FnAbi for fn ptr `{sig:?}`: {err:?}")),
        }
    }

    /// Generates a unique internal symbol name within this module.
    pub fn fresh_const_name(&self, prefix: &str) -> String {
        let id = self.next_const_id.get();
        self.next_const_id.set(id + 1);
        format!("{prefix}.{id}")
    }

    /// Returns or creates a global symbol reference for a local or foreign `static`.
    pub fn get_static(&self, def_id: DefId) -> ObsVal {
        if let Some(&g) = self.statics.borrow().get(&def_id) {
            return g;
        }
        let ty = self.tcx.type_of(def_id);
        let layout = self.layout_of(ty);
        let attrs = self.tcx.codegen_fn_attrs(def_id);
        let sym_name = attrs
            .symbol_name
            .map(|s| s.as_str().to_string())
            .unwrap_or_else(|| {
                if self
                    .tcx
                    .opt_parent(def_id)
                    .is_some_and(|p| self.tcx.def_kind(p) == vsc_hir::def::DefKind::ForeignMod)
                {
                    self.tcx.item_name(def_id).as_str().to_string()
                } else {
                    self.tcx.def_path_str(def_id).replace("::", "__")
                }
            });
        let is_tls = attrs.flags.contains(CodegenFnAttrFlags::THREAD_LOCAL);
        let align = layout.align.bytes().max(1) as u32;
        let sym = if is_tls {
            self.module.borrow_mut().declare_tls(&sym_name, align)
        } else {
            self.module.borrow_mut().declare_static(&sym_name, align)
        };
        let g = ObsVal::sym_addr(sym, false);
        if let Some(linkage) = attrs.linkage {
            self.set_linkage_and_visibility(g, linkage, Visibility::Default);
        }
        self.statics.borrow_mut().insert(def_id, g);
        g
    }

    /// Returns a non-null 1-byte static dummy pointer for zero-sized `Func` closures.
    pub fn zst_env_ptr(&self) -> ObsVal {
        if let Some(g) = self.zst_env_global.get() {
            return g;
        }
        let sym = self.module.borrow_mut().add_const("__vsc_zst_env", &[0u8], 1);
        let g = ObsVal::sym_addr(sym, true);
        self.zst_env_global.set(Some(g));
        g
    }

    /// Emits or reuses an internal constant global holding `bytes`, returning `(ptr, bytes.len())`.
    pub fn const_bytes_global(&self, bytes: &[u8]) -> (ObsVal, usize) {
        let len = bytes.len();
        if let Ok(s) = std::str::from_utf8(bytes)
            && let Some(&g) = self.const_str_globals.borrow().get(s)
        {
            return (g, len);
        }
        let stored: &[u8] = if bytes.is_empty() { &[0u8] } else { bytes };
        let name = self.fresh_const_name("__vsc_str");
        let sym = self.module.borrow_mut().add_const(&name, stored, 1);
        let g = ObsVal::sym_addr(sym, true);
        if let Ok(s) = std::str::from_utf8(bytes) {
            self.const_str_globals.borrow_mut().insert(s.to_string(), g);
        }
        (g, len)
    }

    /// Declares C `malloc(size_t) -> *mut u8`.
    pub fn c_malloc(&self) -> ObsVal {
        let fn_ty = self.type_func(&[self.type_isize()], self.type_ptr(), false);
        self.declare_raw_fn("malloc", ir::CallConv::C, fn_ty)
    }

    /// Declares C `memcmp(const void*, const void*, size_t) -> i32`.
    pub fn c_memcmp(&self) -> ObsVal {
        let ptr = self.type_ptr();
        let fn_ty = self.type_func(&[ptr, ptr, self.type_isize()], self.type_i32(), false);
        self.declare_raw_fn("memcmp", ir::CallConv::C, fn_ty)
    }

    /// Declares C `fmod(f64, f64) -> f64` or `fmodf(f32, f32) -> f32`.
    pub fn c_fmod(&self, is_f32: bool) -> (ObsType, ObsVal) {
        let fty = if is_f32 { self.type_f32() } else { self.type_f64() };
        let fn_ty = self.type_func(&[fty, fty], fty, false);
        let name = if is_f32 { "fmodf" } else { "fmod" };
        let fn_val = self.declare_raw_fn(name, ir::CallConv::C, fn_ty);
        (fn_ty, fn_val)
    }

    /// Emits (or retrieves) an internal 128-bit unsigned restoring division helper:
    /// `local fn @__vsc_udivrem128(i64 %n_lo, i64 %n_hi, i64 %d_lo, i64 %d_hi, ptr %rem_out) -> (i64, i64)`
    /// returning `(q_lo, q_hi)` and storing `(r_lo, r_hi)` at `%rem_out[0..16]`.
    pub fn udivrem128_fn(&self) -> (ObsType, ObsVal) {
        if let Some(cached) = self.udivrem128_fn.get() {
            return cached;
        }
        let i64_ty = self.type_i64();
        let ptr_ty = self.type_ptr();
        let fn_ty = self.type_func(
            &[i64_ty, i64_ty, i64_ty, i64_ty, ptr_ty],
            self.type_i128(),
            false,
        );
        let fn_val = self.declare_raw_fn("__vsc_udivrem128", ir::CallConv::C, fn_ty);
        self.set_linkage_and_visibility(fn_val, Linkage::Internal, Visibility::Hidden);

        let (func_sym, _, _) = fn_val.as_sym().expect("udivrem128_fn must be SymAddr");
        self.entry_block_claimed.borrow_mut().insert(func_sym);
        self.with_func_mut(func_sym, |func| {
            let [p_n_lo, p_n_hi, p_d_lo, p_d_hi, p_rem_out] = func.params();
            let bb_head = func.new_block("loop_head", &[]);
            let bb_body = func.new_block("loop_body", &[]);
            let bb_exit = func.new_block("loop_exit", &[]);

            func.switch_to_block(ir::Block::ENTRY);
            // 80-byte frame state:
            // [0]=n_lo, [8]=n_hi, [16]=q_lo, [24]=q_hi, [32]=r_lo, [40]=r_hi,
            // [48]=cnt, [56]=d_lo, [64]=d_hi, [72]=rem_out
            let st = func.stack_slot(80, 8);
            let zero = func.i64_const(0);
            let c128 = func.i64_const(128);
            func.i64_store(st, p_n_lo, 0);
            func.i64_store(st, p_n_hi, 8);
            func.i64_store(st, zero, 16);
            func.i64_store(st, zero, 24);
            func.i64_store(st, zero, 32);
            func.i64_store(st, zero, 40);
            func.i64_store(st, c128, 48);
            func.i64_store(st, p_d_lo, 56);
            func.i64_store(st, p_d_hi, 64);
            func.store(ir::Type::Ptr, st, p_rem_out, 72);
            func.br(bb_head, &[]);

            func.switch_to_block(bb_head);
            let cnt = func.i64_load(st, 48);
            let z_head = func.i64_const(0);
            let is_done = func.icmp(ir::Type::I64, ir::IntCC::Eq, cnt, z_head);
            func.br_if(is_done, bb_exit, &[], bb_body, &[]);

            func.switch_to_block(bb_body);
            let n_lo = func.i64_load(st, 0);
            let n_hi = func.i64_load(st, 8);
            let q_lo = func.i64_load(st, 16);
            let q_hi = func.i64_load(st, 24);
            let r_lo = func.i64_load(st, 32);
            let r_hi = func.i64_load(st, 40);
            let cnt_cur = func.i64_load(st, 48);
            let d_lo = func.i64_load(st, 56);
            let d_hi = func.i64_load(st, 64);

            let c1 = func.i64_const(1);
            let c63 = func.i64_const(63);

            // Shift (r_hi:r_lo) left by 1, shifting in MSB of n_hi:
            let msb_n = func.shr_u(ir::Type::I64, n_hi, c63);
            let msb_r = func.shr_u(ir::Type::I64, r_lo, c63);
            let r_hi_sh = func.shl(ir::Type::I64, r_hi, c1);
            let r_hi1 = func.or(ir::Type::I64, r_hi_sh, msb_r);
            let r_lo_sh = func.shl(ir::Type::I64, r_lo, c1);
            let r_lo1 = func.or(ir::Type::I64, r_lo_sh, msb_n);

            // Shift (n_hi:n_lo) left by 1:
            let msb_nlo = func.shr_u(ir::Type::I64, n_lo, c63);
            let n_hi_sh = func.shl(ir::Type::I64, n_hi, c1);
            let new_n_hi = func.or(ir::Type::I64, n_hi_sh, msb_nlo);
            let new_n_lo = func.shl(ir::Type::I64, n_lo, c1);

            // Shift (q_hi:q_lo) left by 1:
            let msb_qlo = func.shr_u(ir::Type::I64, q_lo, c63);
            let q_hi_sh = func.shl(ir::Type::I64, q_hi, c1);
            let new_q_hi = func.or(ir::Type::I64, q_hi_sh, msb_qlo);
            let q_lo1 = func.shl(ir::Type::I64, q_lo, c1);

            // Compare unsigned 128-bit `r1 >= d`:
            let hi_gt = func.icmp(ir::Type::I64, ir::IntCC::Ugt, r_hi1, d_hi);
            let hi_eq = func.icmp(ir::Type::I64, ir::IntCC::Eq, r_hi1, d_hi);
            let lo_ge = func.icmp(ir::Type::I64, ir::IntCC::Uge, r_lo1, d_lo);
            let eq_and_ge = func.and(ir::Type::I1, hi_eq, lo_ge);
            let ge = func.or(ir::Type::I1, hi_gt, eq_and_ge);

            // Branchless 128-bit subtract `r1 - d`:
            let (sub_lo, borrow) = func.sub_ovf_u(ir::Type::I64, r_lo1, d_lo);
            let borrow64 = func.ext_u(ir::Type::I64, borrow);
            let sub_hi0 = func.sub(ir::Type::I64, r_hi1, d_hi);
            let sub_hi = func.sub(ir::Type::I64, sub_hi0, borrow64);

            let new_r_lo = func.select(ir::Type::I64, ge, sub_lo, r_lo1);
            let new_r_hi = func.select(ir::Type::I64, ge, sub_hi, r_hi1);
            let q_lo_one = func.or(ir::Type::I64, q_lo1, c1);
            let new_q_lo = func.select(ir::Type::I64, ge, q_lo_one, q_lo1);
            let next_cnt = func.sub(ir::Type::I64, cnt_cur, c1);

            func.i64_store(st, new_n_lo, 0);
            func.i64_store(st, new_n_hi, 8);
            func.i64_store(st, new_q_lo, 16);
            func.i64_store(st, new_q_hi, 24);
            func.i64_store(st, new_r_lo, 32);
            func.i64_store(st, new_r_hi, 40);
            func.i64_store(st, next_cnt, 48);
            func.br(bb_head, &[]);

            func.switch_to_block(bb_exit);
            let fin_q_lo = func.i64_load(st, 16);
            let fin_q_hi = func.i64_load(st, 24);
            let fin_r_lo = func.i64_load(st, 32);
            let fin_r_hi = func.i64_load(st, 40);
            let rem_ptr = func.load(ir::Type::Ptr, st, 72);
            func.i64_store(rem_ptr, fin_r_lo, 0);
            func.i64_store(rem_ptr, fin_r_hi, 8);
            func.ret(&[fin_q_lo, fin_q_hi]);
        });

        let entry = (fn_ty, fn_val);
        self.udivrem128_fn.set(Some(entry));
        entry
    }

    /// Emits (or retrieves) `local fn @__vsc_panic_abort(ptr %msg, i64 %len)` returning `!`.
    pub fn panic_abort_fn(&self) -> ObsVal {
        if let Some(f) = self.panic_fn.get() {
            return f;
        }
        let ptr = self.type_ptr();
        let fn_ty = self.type_func(&[ptr, self.type_i64()], self.type_void(), false);
        let llfn = self.declare_raw_fn("__vsc_panic_abort", ir::CallConv::C, fn_ty);
        self.set_linkage_and_visibility(llfn, Linkage::Internal, Visibility::Default);

        let write_ty = self.type_func(
            &[self.type_i32(), ptr, self.type_isize()],
            self.type_isize(),
            false,
        );
        let write_fn = self.declare_raw_fn("write", ir::CallConv::C, write_ty);
        let (write_sym, _, _) = write_fn.as_sym().unwrap();

        let exit_ty = self.type_func(&[self.type_i32()], self.type_void(), false);
        let exit_fn = self.declare_raw_fn("_exit", ir::CallConv::C, exit_ty);
        let (exit_sym, _, _) = exit_fn.as_sym().unwrap();

        let (nl_val, _) = self.const_bytes_global(b"\n");
        let (nl_sym, _, _) = nl_val.as_sym().unwrap();

        let (panic_sym, _, _) = llfn.as_sym().unwrap();
        self.entry_block_claimed.borrow_mut().insert(panic_sym);
        self.with_func_mut(panic_sym, |func| {
            func.attrs.cold = true;
            func.attrs.noreturn = true;
            let [msg, len] = func.params();
            let fd = func.i32_const(2);
            let _ = func.call(write_sym, &[fd, msg, len], ir::Type::I64);
            let nl_ptr = func.ptr_sym(nl_sym, 0);
            let one = func.i64_const(1);
            let _ = func.call(write_sym, &[fd, nl_ptr, one], ir::Type::I64);
            let code = func.i32_const(101);
            let _ = func.call(exit_sym, &[code], ir::Type::Void);
            func.ret(&[]);
        });

        self.panic_fn.set(Some(llfn));
        llfn
    }
}
