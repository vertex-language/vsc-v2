//! Obsidian IR instruction builder and MIR statement/rvalue/terminator lowering (`vsc_codegen_obsidian::builder`).

mod mir_arith;
mod mir_place;
mod mir_rvalue;
mod mir_term;
mod ops;

use std::cell::Cell;

use obsidian::ir;
use vsc_abi::{Align, BackendRepr, FieldsShape};
use vsc_codegen_ssa::common::{IntPredicate, RealPredicate};
use vsc_codegen_ssa::mir::PlaceRef;
use vsc_middle::mir::BinOp;
use vsc_middle::ty::layout::TyAndLayout;
use vsc_middle::ty::{ConstKind, Instance, Ty, TyKind};

use crate::context::CodegenCx;
use crate::value::{ObsBlock, ObsScalar, ObsType, ObsVal};

/// Scoped instruction builder positioning inside a single [`ir::Function`].
pub struct Builder<'a, 'tcx> {
    pub cx: &'a CodegenCx<'tcx>,
    pub func_sym: ir::Sym,
    pub cur_block: Cell<ir::Block>,
}

impl<'a, 'tcx> Builder<'a, 'tcx> {
    /// Appends a new basic block to `llfn` (claiming `Block::ENTRY` on the first call for `llfn`).
    pub fn append_block(cx: &CodegenCx<'tcx>, llfn: ObsVal, name: &str) -> ObsBlock {
        let (func_sym, _, _) = llfn.as_sym().expect("append_block on non-function ObsVal");
        if cx.entry_block_claimed.borrow_mut().insert(func_sym) {
            cx.with_func_mut(func_sym, |func| {
                func.blocks[0].name = String::from(name);
            });
            ObsBlock {
                func_sym,
                block: ir::Block::ENTRY,
            }
        } else {
            let block = cx.with_func_mut(func_sym, |func| func.new_block(name, &[]));
            ObsBlock { func_sym, block }
        }
    }

    /// Creates a [`Builder`] positioned at the end of `bb`.
    pub fn build(cx: &'a CodegenCx<'tcx>, llfn: ObsVal, bb: ObsBlock) -> Self {
        let (func_sym, _, _) = llfn.as_sym().expect("Builder::build on non-function ObsVal");
        cx.with_func_mut(func_sym, |func| {
            func.current_block = bb.block;
        });
        Self {
            cx,
            func_sym,
            cur_block: Cell::new(bb.block),
        }
    }

    /// Returns the active function as an [`ObsVal`].
    #[inline]
    pub fn llfn(&self) -> ObsVal {
        ObsVal::sym_addr(self.func_sym, false)
    }

    /// Appends a new basic block to the active function.
    pub fn append_block_here(&self, name: &str) -> ObsBlock {
        Self::append_block(self.cx, self.llfn(), name)
    }

    /// Positions the builder at the end of `bb`.
    pub fn position_at_end(&self, bb: ObsBlock) {
        self.cur_block.set(bb.block);
        self.cx.with_func_mut(self.func_sym, |func| {
            func.current_block = bb.block;
        });
    }

    fn with_func<R>(&self, f: impl FnOnce(&mut ir::Function) -> R) -> R {
        let bb = self.cur_block.get();
        self.cx.with_func_mut(self.func_sym, |func| {
            func.current_block = bb;
            f(func)
        })
    }

    /// Returns `true` if the current basic block already ends with a terminator instruction.
    pub fn is_cur_block_terminated(&self) -> bool {
        let bb = self.cur_block.get();
        self.cx.with_func(self.func_sym, |func| {
            func.blocks[bb.index()]
                .insts
                .last()
                .is_some_and(|&v| func.insts[v.index()].op.is_terminator())
        })
    }

    /// Allocates a stack slot for `ty` with alignment `align`.
    pub fn alloca(&self, ty: ObsType, align: Align, _name: &str) -> ObsVal {
        let size = ty.byte_size().max(1) as u32;
        let align_bytes = align.bytes().max(1) as u32;
        let slot_val = self.with_func(|func| func.stack_slot(size, align_bytes));
        ObsVal::stack_slot(slot_val, 0)
    }

    /// Allocates a stack slot sized and aligned for `layout`.
    pub fn alloca_layout(&self, layout: TyAndLayout<'tcx>, name: &str) -> PlaceRef<'tcx, ObsVal> {
        let size = layout.size.bytes().max(1) as u32;
        let align_bytes = layout.align.bytes().max(1) as u32;
        let _ = name;
        let slot_val = self.with_func(|func| func.stack_slot(size, align_bytes));
        PlaceRef::new_sized(ObsVal::stack_slot(slot_val, 0), layout)
    }

    /// Loads a value of `ty` from `ptr` with alignment `align`.
    pub fn load(&self, ty: ObsType, ptr: ObsVal, align: Align) -> ObsVal {
        if let ObsType::Pair(t0, t1) = ty {
            let v0 = self.load(ObsType::Prim(t0), ptr, align);
            let off1 = ObsType::Prim(t0).byte_size();
            let p1 = self.ptradd(ptr, off1);
            let v1 = self.load(ObsType::Prim(t1), p1, align);
            return ObsVal::Pair(v0.as_scalar(), v1.as_scalar());
        }
        let ir_ty = ty.as_ir_type();
        if let ObsVal::Scalar(ObsScalar::StackSlot { slot_val, offset }) = ptr
            && let Ok(off16) = i16::try_from(offset)
        {
            let v = self.with_func(|func| func.load(ir_ty, slot_val, off16));
            return ObsVal::ssa(v, ir_ty);
        }
        let v = self.with_func(|func| {
            let p = materialize_scalar(func, ptr.as_scalar());
            func.load(ir_ty, p, 0)
        });
        ObsVal::ssa(v, ir_ty)
    }

    /// Stores `val` to `ptr` with alignment `align`.
    pub fn store(&self, val: ObsVal, ptr: ObsVal, align: Align) {
        if let ObsVal::Pair(s0, s1) = val {
            self.store(ObsVal::Scalar(s0), ptr, align);
            let off1 = ObsType::Prim(s0.ty()).byte_size();
            let p1 = self.ptradd(ptr, off1);
            self.store(ObsVal::Scalar(s1), p1, align);
            return;
        }
        let val_ty = val.as_scalar().ty();
        if let ObsVal::Scalar(ObsScalar::StackSlot { slot_val, offset }) = ptr
            && let Ok(off16) = i16::try_from(offset)
        {
            self.with_func(|func| {
                let v = materialize_scalar(func, val.as_scalar());
                func.store(val_ty, slot_val, v, off16);
            });
            return;
        }
        self.with_func(|func| {
            let p = materialize_scalar(func, ptr.as_scalar());
            let v = materialize_scalar(func, val.as_scalar());
            func.store(val_ty, p, v, 0);
        });
    }

    /// Offsets `ptr` by a static byte offset `byte_offset`.
    pub fn ptradd(&self, ptr: ObsVal, byte_offset: u64) -> ObsVal {
        if byte_offset == 0 {
            return ptr;
        }
        if let ObsVal::Scalar(ObsScalar::StackSlot { slot_val, offset }) = ptr
            && let Ok(delta) = i32::try_from(byte_offset)
            && let Some(new_off) = offset.checked_add(delta)
            && (i16::MIN as i32..=i16::MAX as i32).contains(&new_off)
        {
            return ObsVal::stack_slot(slot_val, new_off);
        }
        if let ObsVal::Scalar(ObsScalar::SymAddr {
            sym,
            is_const,
            offset,
        }) = ptr
        {
            return ObsVal::Scalar(ObsScalar::SymAddr {
                sym,
                is_const,
                offset: offset.wrapping_add(byte_offset as i64),
            });
        }
        let v = self.with_func(|func| {
            let p = materialize_scalar(func, ptr.as_scalar());
            if let Ok(imm32) = i32::try_from(byte_offset) {
                func.ptr_add_imm(p, imm32)
            } else {
                let off_val = func.i64_const(byte_offset as i64);
                func.ptr_add(p, off_val)
            }
        });
        ObsVal::ssa(v, ir::Type::Ptr)
    }

    /// Offsets `ptr` by a dynamic SSA integer `byte_offset`.
    pub fn ptradd_dyn(&self, ptr: ObsVal, byte_offset: ObsVal) -> ObsVal {
        if let ObsVal::Scalar(ObsScalar::ConstBits(_, bits)) = byte_offset {
            return self.ptradd(ptr, bits);
        }
        let v = self.with_func(|func| {
            let p = materialize_scalar(func, ptr.as_scalar());
            let off = materialize_scalar(func, byte_offset.as_scalar());
            func.ptr_add(p, off)
        });
        ObsVal::ssa(v, ir::Type::Ptr)
    }

    /// Copies `size` bytes from `src` to `dst`.
    pub fn memcpy(
        &self,
        dst: ObsVal,
        _dst_align: Align,
        src: ObsVal,
        _src_align: Align,
        size: u64,
    ) {
        if size == 0 {
            return;
        }
        self.with_func(|func| {
            let d = materialize_scalar(func, dst.as_scalar());
            let s = materialize_scalar(func, src.as_scalar());
            if let Ok(imm16) = u16::try_from(size) {
                func.mem_copy_imm(d, s, imm16);
            } else {
                let len_val = func.i64_const(size as i64);
                func.mem_copy(d, s, len_val);
            }
        });
    }

    /// Copies a dynamic number of bytes `byte_len` from `src` to `dst` (`allow_overlap = true` for `memmove`).
    pub fn memcpy_dyn(&self, dst: ObsVal, src: ObsVal, byte_len: ObsVal, allow_overlap: bool) {
        self.with_func(|func| {
            let d = materialize_scalar(func, dst.as_scalar());
            let s = materialize_scalar(func, src.as_scalar());
            let l = materialize_scalar(func, byte_len.as_scalar());
            if allow_overlap {
                func.mem_move(d, s, l);
            } else {
                func.mem_copy(d, s, l);
            }
        });
    }

    /// Fills `byte_len` bytes starting at `dst` with `byte_val`.
    pub fn memset_dyn(&self, dst: ObsVal, byte_val: ObsVal, byte_len: ObsVal) {
        self.with_func(|func| {
            let d = materialize_scalar(func, dst.as_scalar());
            let b = materialize_scalar(func, byte_val.as_scalar());
            let l = materialize_scalar(func, byte_len.as_scalar());
            func.mem_fill(d, b, l);
        });
    }

    /// Emits an atomic load of `ty` from `ptr` with memory ordering `order`.
    pub fn atomic_load(&self, ty: ObsType, order: ir::MemOrder, ptr: ObsVal) -> ObsVal {
        let ir_ty = ty.as_ir_type();
        let v = self.with_func(|func| {
            let p = materialize_scalar(func, ptr.as_scalar());
            func.atomic_load(ir_ty, order, p)
        });
        ObsVal::ssa(v, ir_ty)
    }

    /// Emits an atomic store of `val` into `ptr` with memory ordering `order`.
    pub fn atomic_store(&self, order: ir::MemOrder, ptr: ObsVal, val: ObsVal) {
        let ir_ty = val.as_scalar().ty();
        self.with_func(|func| {
            let p = materialize_scalar(func, ptr.as_scalar());
            let v = materialize_scalar(func, val.as_scalar());
            func.atomic_store(ir_ty, order, p, v);
        });
    }

    /// Emits an atomic read-modify-write operation returning the previous value.
    pub fn atomic_rmw(
        &self,
        rmw_op: ir::AtomicRmwOp,
        order: ir::MemOrder,
        ptr: ObsVal,
        val: ObsVal,
    ) -> ObsVal {
        let ir_ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let p = materialize_scalar(func, ptr.as_scalar());
            let rhs = materialize_scalar(func, val.as_scalar());
            func.atomic_rmw(ir_ty, rmw_op, order, p, rhs)
        });
        ObsVal::ssa(v, ir_ty)
    }

    /// Emits an atomic compare-and-exchange returning `(old_val, succeeded_i1)`.
    pub fn atomic_cas(
        &self,
        order: ir::MemOrder,
        ptr: ObsVal,
        expected: ObsVal,
        desired: ObsVal,
    ) -> (ObsVal, ObsVal) {
        let ir_ty = expected.as_scalar().ty();
        let (old, ok) = self.with_func(|func| {
            let p = materialize_scalar(func, ptr.as_scalar());
            let exp = materialize_scalar(func, expected.as_scalar());
            let des = materialize_scalar(func, desired.as_scalar());
            func.atomic_cas(ir_ty, order, p, exp, des)
        });
        (ObsVal::ssa(old, ir_ty), ObsVal::ssa(ok, ir::Type::I1))
    }

    /// Emits a hardware memory fence with ordering `order`.
    pub fn mem_fence(&self, order: ir::MemOrder) {
        self.with_func(|func| {
            func.mem_fence(order);
        });
    }

    /// Emits a CPU spin-loop hint (`pause` / `yield`).
    pub fn spin_hint(&self) {
        self.with_func(|func| {
            func.spin_hint();
        });
    }

    /// Rotates `val` left by `amt` bits (supports 8/16/32/64/128-bit integers).
    pub fn rotl(&self, val: ObsVal, amt: ObsVal) -> ObsVal {
        if matches!(val, ObsVal::Pair(..)) {
            let c128 = self.cx.const_uint_big(self.cx.type_i128(), 128);
            let c127 = self.cx.const_uint_big(self.cx.type_i128(), 127);
            let s = self.and(self.intcast(amt, self.cx.type_i128(), false), c127);
            let inv = self.sub(c128, s);
            let z = self.cx.const_uint_big(self.cx.type_i128(), 0);
            let is_zero = self.icmp(IntPredicate::IntEQ, s, z);
            let rotated = self.or(self.shl_128(val, s), self.shr_128(val, inv, false));
            return self.select(is_zero, val, rotated);
        }
        let ty = val.as_scalar().ty();
        let amt_i64 = self.intcast(amt, self.cx.type_i64(), false);
        let v = self.with_func(|func| {
            let l = materialize_scalar(func, val.as_scalar());
            let r = materialize_scalar(func, amt_i64.as_scalar());
            func.rotl(ty, l, r)
        });
        ObsVal::ssa(v, ty)
    }

    /// Rotates `val` right by `amt` bits (supports 8/16/32/64/128-bit integers).
    pub fn rotr(&self, val: ObsVal, amt: ObsVal) -> ObsVal {
        if matches!(val, ObsVal::Pair(..)) {
            let c128 = self.cx.const_uint_big(self.cx.type_i128(), 128);
            let c127 = self.cx.const_uint_big(self.cx.type_i128(), 127);
            let s = self.and(self.intcast(amt, self.cx.type_i128(), false), c127);
            let inv = self.sub(c128, s);
            let z = self.cx.const_uint_big(self.cx.type_i128(), 0);
            let is_zero = self.icmp(IntPredicate::IntEQ, s, z);
            let rotated = self.or(self.shr_128(val, s, false), self.shl_128(val, inv));
            return self.select(is_zero, val, rotated);
        }
        let ty = val.as_scalar().ty();
        let amt_i64 = self.intcast(amt, self.cx.type_i64(), false);
        let v = self.with_func(|func| {
            let l = materialize_scalar(func, val.as_scalar());
            let r = materialize_scalar(func, amt_i64.as_scalar());
            func.rotr(ty, l, r)
        });
        ObsVal::ssa(v, ty)
    }

    /// Counts leading zeros in `val` (supports 8/16/32/64/128-bit integers).
    pub fn clz(&self, val: ObsVal) -> ObsVal {
        if let ObsVal::Pair(lo, hi) = val {
            let lo_v = ObsVal::Scalar(lo);
            let hi_v = ObsVal::Scalar(hi);
            let hi_zero = self.icmp(IntPredicate::IntEQ, hi_v, self.cx.const_u64(0));
            let hi_clz = self.clz(hi_v);
            let lo_clz = self.add(self.clz(lo_v), self.cx.const_u64(64));
            let res64 = self.select(hi_zero, lo_clz, hi_clz);
            return self.zext(res64, self.cx.type_i128());
        }
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.clz(ty, s)
        });
        ObsVal::ssa(v, ty)
    }

    /// Counts trailing zeros in `val` (supports 8/16/32/64/128-bit integers).
    pub fn ctz(&self, val: ObsVal) -> ObsVal {
        if let ObsVal::Pair(lo, hi) = val {
            let lo_v = ObsVal::Scalar(lo);
            let hi_v = ObsVal::Scalar(hi);
            let lo_zero = self.icmp(IntPredicate::IntEQ, lo_v, self.cx.const_u64(0));
            let lo_ctz = self.ctz(lo_v);
            let hi_ctz = self.add(self.ctz(hi_v), self.cx.const_u64(64));
            let res64 = self.select(lo_zero, hi_ctz, lo_ctz);
            return self.zext(res64, self.cx.type_i128());
        }
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.ctz(ty, s)
        });
        ObsVal::ssa(v, ty)
    }

    /// Counts set bits (`popcnt`) in `val` (supports 8/16/32/64/128-bit integers).
    pub fn popcnt(&self, val: ObsVal) -> ObsVal {
        if let ObsVal::Pair(lo, hi) = val {
            let c0 = self.popcnt(ObsVal::Scalar(lo));
            let c1 = self.popcnt(ObsVal::Scalar(hi));
            let sum = self.add(c0, c1);
            return self.zext(sum, self.cx.type_i128());
        }
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.popcnt(ty, s)
        });
        ObsVal::ssa(v, ty)
    }

    /// Reverses byte order in `val` (supports 8/16/32/64/128-bit integers).
    pub fn byte_swap(&self, val: ObsVal) -> ObsVal {
        if let ObsVal::Pair(lo, hi) = val {
            let new_lo = self.byte_swap(ObsVal::Scalar(hi));
            let new_hi = self.byte_swap(ObsVal::Scalar(lo));
            return ObsVal::Pair(new_lo.as_scalar(), new_hi.as_scalar());
        }
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.byte_swap(ty, s)
        });
        ObsVal::ssa(v, ty)
    }

    /// Reverses bit order in `val` (supports 8/16/32/64/128-bit integers).
    pub fn bit_reverse(&self, val: ObsVal) -> ObsVal {
        if let ObsVal::Pair(lo, hi) = val {
            let new_lo = self.bit_reverse(ObsVal::Scalar(hi));
            let new_hi = self.bit_reverse(ObsVal::Scalar(lo));
            return ObsVal::Pair(new_lo.as_scalar(), new_hi.as_scalar());
        }
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.bit_reverse(ty, s)
        });
        ObsVal::ssa(v, ty)
    }
}

fn emit_const_bits(func: &mut ir::Function, ty: ir::Type, bits: u64) -> ir::Value {
    match ty {
        ir::Type::F32 => func.f32_const(f32::from_bits(bits as u32)),
        ir::Type::F64 => func.f64_const(f64::from_bits(bits)),
        ir::Type::Ptr => {
            if bits == 0 {
                let z = func.i64_const(0);
                func.ptr_from_int(z)
            } else {
                let v = func.i64_const(bits as i64);
                func.ptr_from_int(v)
            }
        }
        ir::Type::Void => func.i64_const(0),
        _ => func.iconst(ty, bits as i64),
    }
}

fn materialize_scalar(func: &mut ir::Function, s: ObsScalar) -> ir::Value {
    match s {
        ObsScalar::Ssa(val, _) => val,
        ObsScalar::ConstBits(ty, bits) => emit_const_bits(func, ty, bits),
        ObsScalar::Undef(ty) => emit_const_bits(func, ty, 0),
        ObsScalar::SymAddr {
            sym,
            is_const: _,
            offset,
        } => {
            if let Ok(off32) = i32::try_from(offset) {
                func.ptr_sym(sym, off32)
            } else {
                let base = func.ptr_sym(sym, 0);
                let off_val = func.i64_const(offset);
                func.ptr_add(base, off_val)
            }
        }
        ObsScalar::StackSlot { slot_val, offset } => {
            if offset == 0 {
                slot_val
            } else {
                func.ptr_add_imm(slot_val, offset)
            }
        }
    }
}

fn coerce_value_to_type(
    func: &mut ir::Function,
    v: ir::Value,
    actual_ty: ir::Type,
    expected_ty: ir::Type,
) -> ir::Value {
    if actual_ty == expected_ty {
        return v;
    }
    if actual_ty == ir::Type::Ptr && expected_ty.is_int() {
        let i64_val = func.ptr_to_int(ir::Type::I64, v);
        return if expected_ty == ir::Type::I64 {
            i64_val
        } else {
            func.trunc(expected_ty, i64_val)
        };
    }
    if actual_ty.is_int() && expected_ty == ir::Type::Ptr {
        let i64_val = if actual_ty == ir::Type::I64 {
            v
        } else {
            func.ext_u(ir::Type::I64, v)
        };
        return func.ptr_from_int(i64_val);
    }
    if actual_ty.is_int() && expected_ty.is_int() {
        let ab = actual_ty.fixed_bits().unwrap_or(64);
        let eb = expected_ty.fixed_bits().unwrap_or(64);
        return if eb < ab {
            func.trunc(expected_ty, v)
        } else {
            func.ext_u(expected_ty, v)
        };
    }
    v
}

#[inline]
pub fn type_is_immediate(layout: TyAndLayout<'_>) -> bool {
    matches!(
        layout.backend_repr,
        BackendRepr::Scalar(_) | BackendRepr::SimdVector { .. }
    )
}

#[inline]
pub fn type_is_scalar_pair(layout: TyAndLayout<'_>) -> bool {
    matches!(layout.backend_repr, BackendRepr::ScalarPair { .. })
}

pub const fn int_predicate(op: BinOp, signed: bool) -> IntPredicate {
    match (op, signed) {
        (BinOp::Eq, _) => IntPredicate::IntEQ,
        (BinOp::Ne, _) => IntPredicate::IntNE,
        (BinOp::Lt, true) => IntPredicate::IntSLT,
        (BinOp::Lt, false) => IntPredicate::IntULT,
        (BinOp::Le, true) => IntPredicate::IntSLE,
        (BinOp::Le, false) => IntPredicate::IntULE,
        (BinOp::Gt, true) => IntPredicate::IntSGT,
        (BinOp::Gt, false) => IntPredicate::IntUGT,
        (BinOp::Ge, true) => IntPredicate::IntSGE,
        (BinOp::Ge, false) => IntPredicate::IntUGE,
        _ => IntPredicate::IntEQ,
    }
}

pub const fn real_predicate(op: BinOp) -> RealPredicate {
    match op {
        BinOp::Eq => RealPredicate::RealOEQ,
        BinOp::Ne => RealPredicate::RealUNE,
        BinOp::Lt => RealPredicate::RealOLT,
        BinOp::Le => RealPredicate::RealOLE,
        BinOp::Gt => RealPredicate::RealOGT,
        BinOp::Ge => RealPredicate::RealOGE,
        _ => RealPredicate::RealOEQ,
    }
}

pub fn eval_array_len<'tcx>(
    tcx: vsc_middle::ty::TyCtxt<'tcx>,
    c: vsc_middle::ty::Const<'tcx>,
) -> u64 {
    match *c.kind() {
        ConstKind::Value(_, n) => n as u64,
        ConstKind::Unevaluated(d, args) => {
            let mut out = self::consts_eval_helper::NullOut;
            let mut interp = vsc_const_eval::interpret::Interpreter::new(tcx, &mut out);
            let body = tcx.optimized_mir(d);
            match interp.call_body(body, args, Vec::new(), Vec::new()) {
                Ok(vsc_const_eval::interpret::Value::Int(i)) => i.bits as u64,
                _ => 0,
            }
        }
        _ => 0,
    }
}

mod consts_eval_helper {
    pub struct NullOut;
    impl vsc_const_eval::interpret::Output for NullOut {
        fn stdout(&mut self, _: &str) {}
        fn stderr(&mut self, _: &str) {}
    }
}

fn array_or_slice_stride<'tcx>(
    cx: &CodegenCx<'tcx>,
    layout: TyAndLayout<'tcx>,
) -> (Ty<'tcx>, u64) {
    let elem = match *layout.ty.kind() {
        TyKind::Array(e, _) | TyKind::Slice(e) => e,
        TyKind::Str => cx.tcx.types.u8,
        other => panic!("index into non-array/slice {other:?}"),
    };
    let stride = match layout.fields {
        FieldsShape::Array { stride, .. } => stride.bytes(),
        _ => {
            let el = cx.layout_of(elem);
            el.size.align_to(el.align).bytes()
        }
    };
    (elem, stride)
}

#[inline]
pub fn ty_is_signed(ty: Ty<'_>) -> bool {
    matches!(ty.kind(), TyKind::Int(_))
}

#[inline]
fn is_str_slice_ref(ty: Ty<'_>) -> bool {
    matches!(
        ty.kind(),
        TyKind::Ref(_, inner, _) if matches!(inner.kind(), TyKind::Str)
    )
}

impl<'tcx> CodegenCx<'tcx> {
    pub fn fn_abi_of_drop_glue(
        &self,
        ty: Ty<'tcx>,
    ) -> &'tcx vsc_target::callconv::FnAbi<'tcx, Ty<'tcx>> {
        let drop_inst = Instance::resolve_drop_in_place(self.tcx, ty);
        self.fn_abi_of_instance(drop_inst)
    }
}
