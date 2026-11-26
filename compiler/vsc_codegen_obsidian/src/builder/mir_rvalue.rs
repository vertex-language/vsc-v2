//! MIR statement, rvalue, aggregate construction, cast/transmute, and unary op codegen.

use obsidian::ir;
use vsc_abi::{Align, BackendRepr, VariantIdx};
use vsc_codegen_ssa::common::IntPredicate;
use vsc_codegen_ssa::mir::{OperandRef, OperandValue, PlaceRef, PlaceValue, align_for_offset};
use vsc_hir::LangItem;
use vsc_middle::mir::{
    AggregateKind, Body, CastKind, Operand, Rvalue, StatementKind, UnOp,
};
use vsc_middle::thir::PointerCoercion;
use vsc_middle::ty::layout::TyAndLayout;
use vsc_middle::ty::{ClosureKind, Instance, Ty, TyKind};

use super::{Builder, eval_array_len, materialize_scalar, ty_is_signed, type_is_scalar_pair};
use crate::type_of::layout_is_bool;
use crate::value::{ObsScalar, ObsType, ObsVal};

impl<'a, 'tcx> Builder<'a, 'tcx> {
    /// Emits an entire `Statement`.
    pub fn codegen_statement(
        &mut self,
        body: &Body<'tcx>,
        locals: &[PlaceRef<'tcx, ObsVal>],
        instance: Instance<'tcx>,
        stmt: &StatementKind<'tcx>,
    ) {
        match stmt {
            StatementKind::Assign(box_pair) => {
                let (place, rvalue) = &**box_pair;
                let dst = self.codegen_place(body, locals, instance, place);
                self.codegen_rvalue(body, locals, instance, dst, rvalue);
            }
            StatementKind::SetDiscriminant {
                place,
                variant_index,
            } => {
                let p = self.codegen_place(body, locals, instance, place);
                self.codegen_set_discriminant(p, *variant_index);
            }
            StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {}
        }
    }

    /// Lowers `rvalue` directly into `dst`.
    pub fn codegen_rvalue(
        &mut self,
        body: &Body<'tcx>,
        locals: &[PlaceRef<'tcx, ObsVal>],
        instance: Instance<'tcx>,
        dst: PlaceRef<'tcx, ObsVal>,
        rvalue: &Rvalue<'tcx>,
    ) {
        match rvalue {
            Rvalue::Use(op) => {
                let val = self.codegen_operand(body, locals, instance, op);
                self.store_operand(val.val, dst);
            }
            Rvalue::Repeat(op, count_const) => {
                let count = eval_array_len(
                    self.cx.tcx,
                    instance.instantiate_const(self.cx.tcx, *count_const),
                );
                if count == 0 {
                    return;
                }
                let elem_op = self.codegen_operand(body, locals, instance, op);
                let stride = elem_op.layout.size.align_to(elem_op.layout.align).bytes();
                if stride == 0 {
                    return;
                }
                let total_bytes = count.saturating_mul(stride);
                // Fast path 1: 1-byte element (`[b; N]`) -> single `mem.fill`.
                if stride == 1 && count > 8 {
                    if let OperandValue::Immediate(v) = elem_op.val {
                        let len_v = self.cx.const_uint(self.cx.type_isize(), total_bytes);
                        self.memset_dyn(dst.val.llval, v, len_v);
                        return;
                    }
                }
                // Fast path 2: Const-zero scalar/pair (`[0u64; N]`, etc.) -> single `mem.fill(0)`.
                let is_const_zero = match elem_op.val {
                    OperandValue::Immediate(ObsVal::Scalar(ObsScalar::ConstBits(_, 0))) => true,
                    OperandValue::Pair(
                        ObsVal::Scalar(ObsScalar::ConstBits(_, 0)),
                        ObsVal::Scalar(ObsScalar::ConstBits(_, 0)),
                    ) => true,
                    _ => false,
                };
                if is_const_zero && total_bytes > 32 {
                    let zero = self.cx.const_uint(self.cx.type_i8(), 0);
                    let len_v = self.cx.const_uint(self.cx.type_isize(), total_bytes);
                    self.memset_dyn(dst.val.llval, zero, len_v);
                    return;
                }
                // Fast path 3: Large non-byte aggregate repeat (e.g. `[zeroKEvent(); 64]`) -> IR loop.
                if count > 16 && total_bytes > 128 {
                    let loop_bb = self.append_block_here("repeat_loop");
                    let done_bb = self.append_block_here("repeat_done");
                    let idx_slot = self.alloca(self.cx.type_isize(), Align::EIGHT, "repeat_idx");
                    let zero = self.cx.const_uint(self.cx.type_isize(), 0);
                    self.store(zero, idx_slot, Align::EIGHT);
                    self.br(loop_bb);

                    self.position_at_end(loop_bb);
                    let idx = self.load(self.cx.type_isize(), idx_slot, Align::EIGHT);
                    let byte_off = if stride == 1 {
                        idx
                    } else {
                        let s_val = self.cx.const_uint(self.cx.type_isize(), stride);
                        self.mul(idx, s_val)
                    };
                    let elem_ptr = self.ptradd_dyn(dst.val.llval, byte_off);
                    let elem_dst = PlaceRef::new_sized(elem_ptr, elem_op.layout);
                    self.store_operand(elem_op.val, elem_dst);
                    let one = self.cx.const_uint(self.cx.type_isize(), 1);
                    let next_idx = self.add(idx, one);
                    self.store(next_idx, idx_slot, Align::EIGHT);
                    let lim = self.cx.const_uint(self.cx.type_isize(), count);
                    let cont = self.icmp(IntPredicate::IntULT, next_idx, lim);
                    self.cond_br(cont, loop_bb, done_bb);

                    self.position_at_end(done_bb);
                    return;
                }
                for i in 0..count {
                    let ptr = self.ptradd(dst.val.llval, i * stride);
                    let elem_dst = PlaceRef::new_sized(ptr, elem_op.layout);
                    self.store_operand(elem_op.val, elem_dst);
                }
            }
            Rvalue::Ref(_, p) | Rvalue::RawPtr(_, p) => {
                let place = self.codegen_place(body, locals, instance, p);
                let op = if let Some(extra) = place.val.llextra {
                    OperandValue::Pair(place.val.llval, extra)
                } else {
                    OperandValue::Immediate(place.val.llval)
                };
                self.store_operand(op, dst);
            }
            Rvalue::Len(p) => {
                let place = self.codegen_place(body, locals, instance, p);
                let len = self.codegen_place_len(place);
                self.store_operand(OperandValue::Immediate(len), dst);
            }
            Rvalue::Cast(kind, op, dst_ty) => {
                let dst_ty = instance.instantiate_ty(self.cx.tcx, *dst_ty);
                let src = self.codegen_operand(body, locals, instance, op);
                let dst_layout = self.cx.layout_of(dst_ty);
                let val = self.codegen_cast(*kind, src, dst_layout);
                self.store_operand(val, dst);
            }
            Rvalue::BinaryOp(op, box_pair) => {
                let (l, r) = &**box_pair;
                let lhs = self.codegen_operand(body, locals, instance, l);
                let rhs = self.codegen_operand(body, locals, instance, r);
                let val = self.codegen_binary_op(*op, lhs, rhs, dst.layout.ty);
                self.store_operand(val, dst);
            }
            Rvalue::CheckedBinaryOp(op, box_pair) => {
                let (l, r) = &**box_pair;
                let lhs = self.codegen_operand(body, locals, instance, l);
                let rhs = self.codegen_operand(body, locals, instance, r);
                let val = self.codegen_checked_binary_op(*op, lhs, rhs);
                let ordered = match val {
                    OperandValue::Pair(a, b)
                        if dst.layout.fields.count() >= 2
                            && dst.layout.fields.offset(0) > dst.layout.fields.offset(1) =>
                    {
                        OperandValue::Pair(b, a)
                    }
                    other => other,
                };
                self.store_operand(ordered, dst);
            }
            Rvalue::UnaryOp(op, op_arg) => {
                let arg = self.codegen_operand(body, locals, instance, op_arg);
                let val = self.codegen_unary_op(*op, arg, dst.layout.ty);
                self.store_operand(val, dst);
            }
            Rvalue::Discriminant(p) => {
                let place = self.codegen_place(body, locals, instance, p);
                let val = self.codegen_discriminant(place, dst.layout);
                self.store_operand(val, dst);
            }
            Rvalue::Aggregate(kind, ops) => match &**kind {
                AggregateKind::Array(_) | AggregateKind::Tuple | AggregateKind::Closure(..) => {
                    self.store_aggregate_fields(body, locals, instance, dst, ops);
                }
                AggregateKind::Coroutine(..) => {
                    let var_dst = PlaceRef {
                        val: dst.val,
                        layout: dst.layout.for_variant(&self.cx.layout_cx, VariantIdx::from_usize(0)),
                    };
                    self.store_aggregate_fields(body, locals, instance, var_dst, ops);
                    self.codegen_set_discriminant(dst, 0);
                }
                AggregateKind::Adt {
                    def_id,
                    variant_index,
                    active_field,
                    ..
                } => {
                    let adt = self.cx.tcx.adt_def(*def_id);
                    if adt.is_union() {
                        if let Some(op) = ops.first() {
                            let field_idx = active_field.unwrap_or(0);
                            let off = dst.layout.fields.offset(field_idx);
                            let flayout = dst.layout.field(&self.cx.layout_cx, field_idx);
                            let fptr = self.ptradd(dst.val.llval, off.bytes());
                            let val = self.codegen_operand(body, locals, instance, op);
                            self.store_operand(
                                val.val,
                                PlaceRef {
                                    val: PlaceValue::new_sized(
                                        fptr,
                                        align_for_offset(dst.val.align, off),
                                    ),
                                    layout: flayout,
                                },
                            );
                        }
                    } else if adt.is_enum() {
                        let var_dst = PlaceRef {
                            val: dst.val,
                            layout: dst.layout.for_variant(
                                &self.cx.layout_cx,
                                VariantIdx::from_usize(*variant_index),
                            ),
                        };
                        self.store_aggregate_fields(body, locals, instance, var_dst, ops);
                        self.codegen_set_discriminant(dst, *variant_index);
                    } else {
                        self.store_aggregate_fields(body, locals, instance, dst, ops);
                    }
                }
            },
            Rvalue::ClassNew(op) => {
                let val = self.codegen_operand(body, locals, instance, op);
                let size = val.layout.size.bytes().max(1);
                let malloc = self.cx.c_malloc();
                let malloc_ty =
                    self.cx
                        .type_func(&[self.cx.type_isize()], self.cx.type_ptr(), false);
                let ptr = self.call_raw(malloc_ty, malloc, &mut [self.cx.const_usize(size)]);
                self.store_operand(val.val, PlaceRef::new_sized(ptr, val.layout));
                self.store_operand(OperandValue::Immediate(ptr), dst);
            }
        }
    }

    fn store_aggregate_fields(
        &self,
        body: &Body<'tcx>,
        locals: &[PlaceRef<'tcx, ObsVal>],
        instance: Instance<'tcx>,
        dst: PlaceRef<'tcx, ObsVal>,
        ops: &[Operand<'tcx>],
    ) {
        if dst.layout.is_zst() {
            return;
        }
        if let BackendRepr::ScalarPair { .. } = dst.layout.backend_repr
            && ops.len() >= 2
        {
            let mut vals: Vec<_> = ops
                .iter()
                .enumerate()
                .map(|(i, o)| (dst.layout.fields.offset(i), self.codegen_operand(body, locals, instance, o)))
                .filter(|(_, v)| !v.layout.is_zst())
                .collect();
            if vals.len() == 2 {
                // Respect ABI field reordering: `ScalarPair(a, b)` is ordered by byte offset, not declaration index!
                vals.sort_by_key(|(off, _)| *off);
                if let (OperandValue::Immediate(a), OperandValue::Immediate(b)) =
                    (vals[0].1.val, vals[1].1.val)
                {
                    self.store_operand(OperandValue::Pair(a, b), dst);
                    return;
                }
            }
        }

        for (i, op) in ops.iter().enumerate() {
            let val = self.codegen_operand(body, locals, instance, op);
            if val.layout.is_zst() || i >= dst.layout.fields.count() {
                continue;
            }
            let off = dst.layout.fields.offset(i);
            let fptr = self.ptradd(dst.val.llval, off.bytes());
            let flayout = dst.layout.field(&self.cx.layout_cx, i);
            self.store_operand(
                val.val,
                PlaceRef {
                    val: PlaceValue::new_sized(fptr, align_for_offset(dst.val.align, off)),
                    layout: flayout,
                },
            );
        }
    }

    pub fn codegen_cast(
        &mut self,
        kind: CastKind,
        src: OperandRef<'tcx, ObsVal>,
        dst_layout: TyAndLayout<'tcx>,
    ) -> OperandValue<ObsVal> {
        let tcx = self.cx.tcx;
        let dst_ty = self.cx.immediate_obs_type(dst_layout);
        match kind {
            CastKind::IntToInt => {
                let OperandValue::Immediate(v) = src.val else {
                    return OperandValue::ZeroSized;
                };
                if layout_is_bool(dst_layout) {
                    let src_ty = v.ty();
                    return OperandValue::Immediate(self.icmp(
                        IntPredicate::IntNE,
                        v,
                        self.cx.const_uint(src_ty, 0),
                    ));
                }
                OperandValue::Immediate(self.intcast(v, dst_ty, ty_is_signed(src.layout.ty)))
            }
            CastKind::FloatToFloat => {
                let OperandValue::Immediate(v) = src.val else {
                    return OperandValue::ZeroSized;
                };
                let src_bits = src.layout.size.bits();
                let dst_bits = dst_layout.size.bits();
                let out = if dst_bits < src_bits {
                    let val = self.with_func(|func| {
                        let s = materialize_scalar(func, v.as_scalar());
                        func.float_demote(s)
                    });
                    ObsVal::ssa(val, ir::Type::F32)
                } else if dst_bits > src_bits {
                    let val = self.with_func(|func| {
                        let s = materialize_scalar(func, v.as_scalar());
                        func.float_promote(s)
                    });
                    ObsVal::ssa(val, ir::Type::F64)
                } else {
                    v
                };
                OperandValue::Immediate(out)
            }
            CastKind::IntToFloat => {
                let OperandValue::Immediate(v) = src.val else {
                    return OperandValue::ZeroSized;
                };
                let signed = ty_is_signed(src.layout.ty);
                let ir_dst = dst_ty.as_ir_type();
                if let ObsVal::Pair(lo_s, hi_s) = v {
                    let val = self.with_func(|func| {
                        let lo = materialize_scalar(func, lo_s);
                        let hi = materialize_scalar(func, hi_s);
                        let hi_f = if signed {
                            func.float_from_int_s(ir::Type::F64, hi)
                        } else {
                            func.float_from_int_u(ir::Type::F64, hi)
                        };
                        let lo_f = func.float_from_int_u(ir::Type::F64, lo);
                        let two64 = func.f64_const(18446744073709551616.0);
                        let scaled = func.f64_mul(hi_f, two64);
                        let sum = func.f64_add(scaled, lo_f);
                        if ir_dst == ir::Type::F32 {
                            func.float_demote(sum)
                        } else {
                            sum
                        }
                    });
                    return OperandValue::Immediate(ObsVal::ssa(val, ir_dst));
                }
                let val = self.with_func(|func| {
                    let s = materialize_scalar(func, v.as_scalar());
                    if signed {
                        func.float_from_int_s(ir_dst, s)
                    } else {
                        func.float_from_int_u(ir_dst, s)
                    }
                });
                OperandValue::Immediate(ObsVal::ssa(val, ir_dst))
            }
            CastKind::FloatToInt => {
                let OperandValue::Immediate(v) = src.val else {
                    return OperandValue::ZeroSized;
                };
                let signed = ty_is_signed(dst_layout.ty);
                if dst_ty == ObsType::Pair(ir::Type::I64, ir::Type::I64) {
                    let (lo, hi) = self.with_func(|func| {
                        let raw = materialize_scalar(func, v.as_scalar());
                        let f64_v = if func.value_type(raw) == ir::Type::F32 {
                            func.float_promote(raw)
                        } else {
                            raw
                        };
                        let zero_f = func.f64_const(0.0);
                        let is_neg = func.fcmp(ir::FloatCC::Lt, f64_v, zero_f);
                        let mag = func.abs(ir::Type::F64, f64_v);
                        let two64 = func.f64_const(18446744073709551616.0);
                        let div_hi = func.f64_div(mag, two64);
                        let hi_u = func.from_float_trunc_u(ir::Type::I64, div_hi);
                        let hi_f = func.float_from_int_u(ir::Type::F64, hi_u);
                        let hi_scaled = func.f64_mul(hi_f, two64);
                        let rem_f = func.f64_sub(mag, hi_scaled);
                        let lo_u = func.from_float_trunc_u(ir::Type::I64, rem_f);
                        if signed {
                            let z64 = func.i64_const(0);
                            let (neg_lo, borrow) = func.sub_ovf_u(ir::Type::I64, z64, lo_u);
                            let b64 = func.ext_u(ir::Type::I64, borrow);
                            let neg_hi0 = func.sub(ir::Type::I64, z64, hi_u);
                            let neg_hi = func.sub(ir::Type::I64, neg_hi0, b64);
                            let r_lo = func.select(ir::Type::I64, is_neg, neg_lo, lo_u);
                            let r_hi = func.select(ir::Type::I64, is_neg, neg_hi, hi_u);
                            (r_lo, r_hi)
                        } else {
                            let z64 = func.i64_const(0);
                            let r_lo = func.select(ir::Type::I64, is_neg, z64, lo_u);
                            let r_hi = func.select(ir::Type::I64, is_neg, z64, hi_u);
                            (r_lo, r_hi)
                        }
                    });
                    return OperandValue::Immediate(ObsVal::Pair(
                        ObsScalar::Ssa(lo, ir::Type::I64),
                        ObsScalar::Ssa(hi, ir::Type::I64),
                    ));
                }
                let ir_dst = dst_ty.as_ir_type();
                let val = self.with_func(|func| {
                    let s = materialize_scalar(func, v.as_scalar());
                    if signed {
                        func.from_float_sat_s(ir_dst, s)
                    } else {
                        func.from_float_sat_u(ir_dst, s)
                    }
                });
                OperandValue::Immediate(ObsVal::ssa(val, ir_dst))
            }
            CastKind::PtrToPtr => match src.val {
                OperandValue::Immediate(p) => OperandValue::Immediate(p),
                OperandValue::Pair(p, meta) => {
                    if type_is_scalar_pair(dst_layout) {
                        OperandValue::Pair(p, meta)
                    } else {
                        OperandValue::Immediate(p)
                    }
                }
                OperandValue::ZeroSized => OperandValue::ZeroSized,
                OperandValue::Ref(r) => OperandValue::Immediate(r.llval),
            },
            CastKind::PointerExposeProvenance => {
                let ptr = match src.val {
                    OperandValue::Immediate(p) | OperandValue::Pair(p, _) => p,
                    _ => self.cx.const_null(self.cx.type_ptr()),
                };
                OperandValue::Immediate(self.ptrtoint(ptr, dst_ty))
            }
            CastKind::PointerWithExposedProvenance => {
                let OperandValue::Immediate(v) = src.val else {
                    return OperandValue::Immediate(self.cx.const_null(dst_ty));
                };
                let iv = self.intcast(v, self.cx.type_isize(), false);
                OperandValue::Immediate(self.inttoptr(iv, dst_ty))
            }
            CastKind::PointerCoercion(PointerCoercion::ReifyFnPointer) => {
                let TyKind::FnDef(def_id, args) = *src.layout.ty.kind() else {
                    return OperandValue::Immediate(self.cx.const_null(self.cx.type_ptr()));
                };
                let inst = Instance::try_resolve(tcx, def_id, args)
                    .unwrap_or_else(|| Instance::new_raw(def_id, args));
                OperandValue::Immediate(self.cx.get_fn(inst))
            }
            CastKind::PointerCoercion(PointerCoercion::Unsize) => match src.val {
                OperandValue::Pair(p, len) => OperandValue::Pair(p, len),
                OperandValue::Immediate(p) => {
                    let array_ty = match *src.layout.ty.kind() {
                        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => inner,
                        other => panic!("Unsize from non-pointer {other:?}"),
                    };
                    let len = match *array_ty.kind() {
                        TyKind::Array(_, c) => eval_array_len(tcx, c),
                        other => panic!("Unsize from non-array {other:?}"),
                    };
                    OperandValue::Pair(p, self.cx.const_usize(len))
                }
                _ => OperandValue::Pair(
                    self.cx.const_null(self.cx.type_ptr()),
                    self.cx.const_usize(0),
                ),
            },
            CastKind::PointerCoercion(PointerCoercion::ToFunc) => {
                self.codegen_to_func(src, dst_layout)
            }
        }
    }

    /// Transmutes `src` bytes directly into `dst`.
    pub fn codegen_transmute(
        &self,
        src: OperandRef<'tcx, ObsVal>,
        dst: PlaceRef<'tcx, ObsVal>,
    ) {
        if src.layout.is_zst() || dst.layout.is_zst() {
            return;
        }
        let tmp = self.alloca_layout(src.layout, "transmute_tmp");
        self.store_operand(src.val, tmp);
        let size = src.layout.size.bytes().min(dst.layout.size.bytes());
        self.memcpy(
            dst.val.llval,
            dst.val.align,
            tmp.val.llval,
            tmp.val.align,
            size,
        );
    }

    fn codegen_to_func(
        &self,
        src: OperandRef<'tcx, ObsVal>,
        _dst_layout: TyAndLayout<'tcx>,
    ) -> OperandValue<ObsVal> {
        let tcx = self.cx.tcx;
        match *src.layout.ty.kind() {
            TyKind::Adt(adt, _) if tcx.is_lang_item(adt.did(), LangItem::Func) => src.val,
            TyKind::Closure(closure_def, cargs) => {
                let inst = Instance::resolve_closure(tcx, closure_def, cargs, ClosureKind::Fn);
                let llfn = self.cx.get_fn(inst);
                let env_ptr = if src.layout.is_zst() {
                    self.cx.zst_env_ptr()
                } else {
                    let size = src.layout.size.bytes().max(1);
                    let malloc = self.cx.c_malloc();
                    let malloc_ty =
                        self.cx
                            .type_func(&[self.cx.type_isize()], self.cx.type_ptr(), false);
                    let heap =
                        self.call_raw(malloc_ty, malloc, &mut [self.cx.const_usize(size)]);
                    self.store_operand(src.val, PlaceRef::new_sized(heap, src.layout));
                    heap
                };
                OperandValue::Pair(llfn, env_ptr)
            }
            TyKind::FnDef(def_id, fn_args) => {
                let tramp =
                    crate::callee::get_or_emit_fn_def_trampoline(self.cx, def_id, fn_args);
                OperandValue::Pair(tramp, self.cx.const_null(self.cx.type_ptr()))
            }
            TyKind::FnPtr(sig) => {
                let tramp =
                    crate::callee::get_or_emit_fn_ptr_trampoline(self.cx, src.layout.ty, sig);
                let OperandValue::Immediate(raw_fn) = src.val else {
                    panic!("FnPtr without immediate");
                };
                OperandValue::Pair(tramp, raw_fn)
            }
            other => panic!("ToFunc from unsupported type {other:?}"),
        }
    }

    pub fn codegen_unary_op(
        &self,
        op: UnOp,
        arg: OperandRef<'tcx, ObsVal>,
        _dst_ty: Ty<'tcx>,
    ) -> OperandValue<ObsVal> {
        let OperandValue::Immediate(v) = arg.val else {
            return OperandValue::ZeroSized;
        };
        let out = match op {
            UnOp::Not => self.not(v),
            UnOp::Neg => {
                if matches!(v, ObsVal::Pair(..)) {
                    self.sub(self.cx.const_uint_big(self.cx.type_i128(), 0), v)
                } else {
                    let ty = v.as_scalar().ty();
                    let res = self.with_func(|func| {
                        let s = materialize_scalar(func, v.as_scalar());
                        func.neg(ty, s)
                    });
                    ObsVal::ssa(res, ty)
                }
            }
        };
        OperandValue::Immediate(out)
    }
}
