//! Function calls, aggregate manipulation, control-flow terminators, casts, and scalar ALU/bitwise ops.

use obsidian::ir;
use vsc_codegen_ssa::common::{IntPredicate, RealPredicate};

use super::{Builder, coerce_value_to_type, materialize_scalar};
use crate::common::{int_predicate_to_obs, real_predicate_to_obs};
use crate::value::{ObsBlock, ObsScalar, ObsType, ObsVal};

impl<'a, 'tcx> Builder<'a, 'tcx> {
    /// Emits a direct or indirect function call conforming to `fn_ty`.
    pub fn call_raw(&self, fn_ty: ObsType, callee: ObsVal, args: &mut [ObsVal]) -> ObsVal {
        let ObsType::Func(sig_id) = fn_ty else {
            panic!("call_raw requires ObsType::Func, got {fn_ty:?}");
        };
        let sig = self.cx.get_fn_sig(sig_id);
        self.with_func(|func| {
            let mut arg_vals = Vec::with_capacity(args.len() + 2);
            let mut param_idx = 0usize;
            for a in args.iter() {
                match *a {
                    ObsVal::Pair(s0, s1) => {
                        for s in [s0, s1] {
                            let mut v = materialize_scalar(func, s);
                            if let Some(&expected_ty) = sig.params.get(param_idx) {
                                let actual_ty = func.value_type(v);
                                if actual_ty != expected_ty {
                                    v = coerce_value_to_type(func, v, actual_ty, expected_ty);
                                }
                            }
                            arg_vals.push(v);
                            param_idx += 1;
                        }
                    }
                    ObsVal::Scalar(s) => {
                        let mut v = materialize_scalar(func, s);
                        if let Some(&expected_ty) = sig.params.get(param_idx) {
                            let actual_ty = func.value_type(v);
                            if actual_ty != expected_ty {
                                v = coerce_value_to_type(func, v, actual_ty, expected_ty);
                            }
                        }
                        arg_vals.push(v);
                        param_idx += 1;
                    }
                }
            }
            let fixed_count = (sig.params.len() as u8).min(127);
            if let Some((sym, false, 0)) = callee.as_sym() {
                if sig.is_variadic {
                    match sig.returns.as_slice() {
                        [] => {
                            let _ = func.call_variadic(sym, fixed_count, &arg_vals, ir::Type::Void);
                            ObsVal::undef(ir::Type::Void)
                        }
                        [ret_ty, ..] => {
                            let v = func.call_variadic(sym, fixed_count, &arg_vals, *ret_ty);
                            ObsVal::ssa(v, *ret_ty)
                        }
                    }
                } else {
                    match sig.returns.as_slice() {
                        [] => {
                            let _ = func.call(sym, &arg_vals, ir::Type::Void);
                            ObsVal::undef(ir::Type::Void)
                        }
                        [ret_ty] => {
                            let v = func.call(sym, &arg_vals, *ret_ty);
                            ObsVal::ssa(v, *ret_ty)
                        }
                        [r0, r1, ..] => {
                            let (v0, v1) = func.call_pair(sym, &arg_vals, *r0, *r1);
                            ObsVal::Pair(ObsScalar::Ssa(v0, *r0), ObsScalar::Ssa(v1, *r1))
                        }
                    }
                }
            } else {
                let callee_ptr = materialize_scalar(func, callee.as_scalar());
                if sig.is_variadic {
                    match sig.returns.as_slice() {
                        [] => {
                            let _ = func.call_indirect_variadic(
                                callee_ptr,
                                fixed_count,
                                &arg_vals,
                                ir::Type::Void,
                            );
                            ObsVal::undef(ir::Type::Void)
                        }
                        [ret_ty, ..] => {
                            let v = func.call_indirect_variadic(
                                callee_ptr,
                                fixed_count,
                                &arg_vals,
                                *ret_ty,
                            );
                            ObsVal::ssa(v, *ret_ty)
                        }
                    }
                } else {
                    match sig.returns.as_slice() {
                        [] => {
                            let _ = func.call_indirect(callee_ptr, &arg_vals, ir::Type::Void);
                            ObsVal::undef(ir::Type::Void)
                        }
                        [ret_ty] => {
                            let v = func.call_indirect(callee_ptr, &arg_vals, *ret_ty);
                            ObsVal::ssa(v, *ret_ty)
                        }
                        [r0, r1, ..] => {
                            let (v0, v1) =
                                func.call_indirect_pair(callee_ptr, &arg_vals, *r0, *r1);
                            ObsVal::Pair(ObsScalar::Ssa(v0, *r0), ObsScalar::Ssa(v1, *r1))
                        }
                    }
                }
            }
        })
    }

    pub fn extract_value(&self, agg: ObsVal, idx: u32) -> ObsVal {
        match (agg, idx) {
            (ObsVal::Pair(a, _), 0) => ObsVal::Scalar(a),
            (ObsVal::Pair(_, b), 1) => ObsVal::Scalar(b),
            (ObsVal::Scalar(s), 0) => ObsVal::Scalar(s),
            _ => panic!("extract_value out of bounds ({idx}) on {agg:?}"),
        }
    }

    pub fn insert_value(&self, agg: ObsVal, elem: ObsVal, idx: u32) -> ObsVal {
        let e = elem.as_scalar();
        match (agg, idx) {
            (ObsVal::Pair(_, b), 0) => ObsVal::Pair(e, b),
            (ObsVal::Pair(a, _), 1) => ObsVal::Pair(a, e),
            (ObsVal::Scalar(s), 0) => ObsVal::Pair(e, s),
            (ObsVal::Scalar(s), 1) => ObsVal::Pair(s, e),
            _ => panic!("insert_value out of bounds ({idx}) on {agg:?}"),
        }
    }

    pub fn br(&self, target: ObsBlock) {
        if self.is_cur_block_terminated() {
            return;
        }
        self.with_func(|func| {
            func.br(target.block, &[]);
        });
    }

    pub fn cond_br(&self, cond: ObsVal, then_bb: ObsBlock, else_bb: ObsBlock) {
        if self.is_cur_block_terminated() {
            return;
        }
        self.with_func(|func| {
            let c = materialize_scalar(func, cond.as_scalar());
            func.br_if(c, then_bb.block, &[], else_bb.block, &[]);
        });
    }

    pub fn switch_int(&self, discr: ObsVal, default_bb: ObsBlock, cases: &[(u64, ObsBlock)]) {
        if self.is_cur_block_terminated() {
            return;
        }
        let obs_cases: Vec<(u64, ir::Block)> =
            cases.iter().map(|&(v, bb)| (v, bb.block)).collect();
        self.with_func(|func| {
            let idx = materialize_scalar(func, discr.as_scalar());
            func.switch_int(idx, default_bb.block, &obs_cases);
        });
    }

    pub fn ret_void(&self) {
        if self.is_cur_block_terminated() {
            return;
        }
        self.with_func(|func| {
            func.ret(&[]);
        });
    }

    pub fn ret(&self, val: ObsVal) {
        if self.is_cur_block_terminated() {
            return;
        }
        self.with_func(|func| match val {
            ObsVal::Scalar(ObsScalar::Undef(ir::Type::Void)) => {
                func.ret(&[]);
            }
            ObsVal::Scalar(s) => {
                let mut v = materialize_scalar(func, s);
                if let Some(&ret_ty) = func.return_types.first() {
                    let actual_ty = func.value_type(v);
                    if actual_ty != ret_ty {
                        v = coerce_value_to_type(func, v, actual_ty, ret_ty);
                    }
                }
                func.ret(&[v]);
            }
            ObsVal::Pair(s0, s1) => {
                let v0 = materialize_scalar(func, s0);
                let v1 = materialize_scalar(func, s1);
                func.ret(&[v0, v1]);
            }
        });
    }

    pub fn unreachable(&self) {
        if self.is_cur_block_terminated() {
            return;
        }
        self.with_func(|func| {
            func.trap(ir::TrapReason::Unreachable);
        });
    }

    pub fn float_promote(&self, val: ObsVal) -> ObsVal {
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.float_promote(s)
        });
        ObsVal::ssa(v, ir::Type::F64)
    }

    pub fn fsqrt(&self, val: ObsVal) -> ObsVal {
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.fsqrt(ty, s)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn fabs(&self, val: ObsVal) -> ObsVal {
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.abs(ty, s)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn ffloor(&self, val: ObsVal) -> ObsVal {
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.ffloor(ty, s)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn fceil(&self, val: ObsVal) -> ObsVal {
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.fceil(ty, s)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn ftrunc(&self, val: ObsVal) -> ObsVal {
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.ftrunc(ty, s)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn fnearest(&self, val: ObsVal) -> ObsVal {
        let ty = val.as_scalar().ty();
        let v = self.with_func(|func| {
            let s = materialize_scalar(func, val.as_scalar());
            func.fnearest(ty, s)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn icmp(&self, pred: IntPredicate, lhs: ObsVal, rhs: ObsVal) -> ObsVal {
        if let (ObsVal::Pair(l_lo, l_hi), ObsVal::Pair(r_lo, r_hi)) = (lhs, rhs) {
            let l0 = ObsVal::Scalar(l_lo);
            let l1 = ObsVal::Scalar(l_hi);
            let r0 = ObsVal::Scalar(r_lo);
            let r1 = ObsVal::Scalar(r_hi);
            return match pred {
                IntPredicate::IntEQ => {
                    let hi_eq = self.icmp(IntPredicate::IntEQ, l1, r1);
                    let lo_eq = self.icmp(IntPredicate::IntEQ, l0, r0);
                    self.and(hi_eq, lo_eq)
                }
                IntPredicate::IntNE => {
                    let hi_ne = self.icmp(IntPredicate::IntNE, l1, r1);
                    let lo_ne = self.icmp(IntPredicate::IntNE, l0, r0);
                    self.or(hi_ne, lo_ne)
                }
                IntPredicate::IntULT => {
                    let hi_lt = self.icmp(IntPredicate::IntULT, l1, r1);
                    let hi_eq = self.icmp(IntPredicate::IntEQ, l1, r1);
                    let lo_lt = self.icmp(IntPredicate::IntULT, l0, r0);
                    self.or(hi_lt, self.and(hi_eq, lo_lt))
                }
                IntPredicate::IntULE => {
                    let hi_lt = self.icmp(IntPredicate::IntULT, l1, r1);
                    let hi_eq = self.icmp(IntPredicate::IntEQ, l1, r1);
                    let lo_le = self.icmp(IntPredicate::IntULE, l0, r0);
                    self.or(hi_lt, self.and(hi_eq, lo_le))
                }
                IntPredicate::IntUGT => {
                    let hi_gt = self.icmp(IntPredicate::IntUGT, l1, r1);
                    let hi_eq = self.icmp(IntPredicate::IntEQ, l1, r1);
                    let lo_gt = self.icmp(IntPredicate::IntUGT, l0, r0);
                    self.or(hi_gt, self.and(hi_eq, lo_gt))
                }
                IntPredicate::IntUGE => {
                    let hi_gt = self.icmp(IntPredicate::IntUGT, l1, r1);
                    let hi_eq = self.icmp(IntPredicate::IntEQ, l1, r1);
                    let lo_ge = self.icmp(IntPredicate::IntUGE, l0, r0);
                    self.or(hi_gt, self.and(hi_eq, lo_ge))
                }
                IntPredicate::IntSLT => {
                    let hi_lt = self.icmp(IntPredicate::IntSLT, l1, r1);
                    let hi_eq = self.icmp(IntPredicate::IntEQ, l1, r1);
                    let lo_lt = self.icmp(IntPredicate::IntULT, l0, r0);
                    self.or(hi_lt, self.and(hi_eq, lo_lt))
                }
                IntPredicate::IntSLE => {
                    let hi_lt = self.icmp(IntPredicate::IntSLT, l1, r1);
                    let hi_eq = self.icmp(IntPredicate::IntEQ, l1, r1);
                    let lo_le = self.icmp(IntPredicate::IntULE, l0, r0);
                    self.or(hi_lt, self.and(hi_eq, lo_le))
                }
                IntPredicate::IntSGT => {
                    let hi_gt = self.icmp(IntPredicate::IntSGT, l1, r1);
                    let hi_eq = self.icmp(IntPredicate::IntEQ, l1, r1);
                    let lo_gt = self.icmp(IntPredicate::IntUGT, l0, r0);
                    self.or(hi_gt, self.and(hi_eq, lo_gt))
                }
                IntPredicate::IntSGE => {
                    let hi_gt = self.icmp(IntPredicate::IntSGT, l1, r1);
                    let hi_eq = self.icmp(IntPredicate::IntEQ, l1, r1);
                    let lo_ge = self.icmp(IntPredicate::IntUGE, l0, r0);
                    self.or(hi_gt, self.and(hi_eq, lo_ge))
                }
            };
        }
        let cc = int_predicate_to_obs(pred);
        let op_ty = lhs.as_scalar().ty();
        let v = self.with_func(|func| {
            let l = materialize_scalar(func, lhs.as_scalar());
            let r = materialize_scalar(func, rhs.as_scalar());
            func.icmp(op_ty, cc, l, r)
        });
        ObsVal::ssa(v, ir::Type::I1)
    }

    pub fn fcmp(&self, pred: RealPredicate, lhs: ObsVal, rhs: ObsVal) -> ObsVal {
        let cc = real_predicate_to_obs(pred);
        let v = self.with_func(|func| {
            let l = materialize_scalar(func, lhs.as_scalar());
            let r = materialize_scalar(func, rhs.as_scalar());
            func.fcmp(cc, l, r)
        });
        ObsVal::ssa(v, ir::Type::I1)
    }

    pub fn select(&self, cond: ObsVal, then_val: ObsVal, else_val: ObsVal) -> ObsVal {
        if let (ObsVal::Pair(t0, t1), ObsVal::Pair(e0, e1)) = (then_val, else_val) {
            let r0 = self.select(cond, ObsVal::Scalar(t0), ObsVal::Scalar(e0));
            let r1 = self.select(cond, ObsVal::Scalar(t1), ObsVal::Scalar(e1));
            return ObsVal::Pair(r0.as_scalar(), r1.as_scalar());
        }
        let ty = then_val.as_scalar().ty();
        let v = self.with_func(|func| {
            let c = materialize_scalar(func, cond.as_scalar());
            let t = materialize_scalar(func, then_val.as_scalar());
            let e = materialize_scalar(func, else_val.as_scalar());
            func.select(ty, c, t, e)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn zext(&self, val: ObsVal, dst_ty: ObsType) -> ObsVal {
        if dst_ty == ObsType::Pair(ir::Type::I64, ir::Type::I64) {
            if matches!(val, ObsVal::Pair(..)) {
                return val;
            }
            let lo = self.zext(val, self.cx.type_i64());
            return ObsVal::Pair(lo.as_scalar(), ObsScalar::ConstBits(ir::Type::I64, 0));
        }
        let ir_dst = dst_ty.as_ir_type();
        if val.as_scalar().ty() == ir_dst {
            return val;
        }
        let v = self.with_func(|func| {
            let src = materialize_scalar(func, val.as_scalar());
            func.ext_u(ir_dst, src)
        });
        ObsVal::ssa(v, ir_dst)
    }

    pub fn sext(&self, val: ObsVal, dst_ty: ObsType) -> ObsVal {
        if dst_ty == ObsType::Pair(ir::Type::I64, ir::Type::I64) {
            if matches!(val, ObsVal::Pair(..)) {
                return val;
            }
            let lo = self.sext(val, self.cx.type_i64());
            let hi = self.with_func(|func| {
                let lo_v = materialize_scalar(func, lo.as_scalar());
                let c63 = func.i64_const(63);
                func.shr_s(ir::Type::I64, lo_v, c63)
            });
            return ObsVal::Pair(lo.as_scalar(), ObsScalar::Ssa(hi, ir::Type::I64));
        }
        let ir_dst = dst_ty.as_ir_type();
        if val.as_scalar().ty() == ir_dst {
            return val;
        }
        let v = self.with_func(|func| {
            let src = materialize_scalar(func, val.as_scalar());
            func.ext_s(ir_dst, src)
        });
        ObsVal::ssa(v, ir_dst)
    }

    pub fn trunc(&self, val: ObsVal, dst_ty: ObsType) -> ObsVal {
        if dst_ty == ObsType::Pair(ir::Type::I64, ir::Type::I64) {
            return val;
        }
        let ir_dst = dst_ty.as_ir_type();
        let scalar = val.as_scalar();
        if scalar.ty() == ir_dst {
            return ObsVal::Scalar(scalar);
        }
        let v = self.with_func(|func| {
            let src = materialize_scalar(func, scalar);
            func.trunc(ir_dst, src)
        });
        ObsVal::ssa(v, ir_dst)
    }

    pub fn intcast(&self, val: ObsVal, dst_ty: ObsType, is_signed: bool) -> ObsVal {
        let src_ty = val.ty();
        let src_w = self.cx.int_width(src_ty);
        let dst_w = self.cx.int_width(dst_ty);
        if dst_w < src_w {
            self.trunc(val, dst_ty)
        } else if dst_w > src_w {
            if is_signed {
                self.sext(val, dst_ty)
            } else {
                self.zext(val, dst_ty)
            }
        } else {
            val
        }
    }

    pub fn ptrtoint(&self, val: ObsVal, dst_ty: ObsType) -> ObsVal {
        let ir_dst = dst_ty.as_ir_type();
        let v = self.with_func(|func| {
            let src = materialize_scalar(func, val.as_scalar());
            let i64_val = func.ptr_to_int(ir::Type::I64, src);
            if ir_dst == ir::Type::I64 {
                i64_val
            } else {
                func.trunc(ir_dst, i64_val)
            }
        });
        ObsVal::ssa(v, ir_dst)
    }

    pub fn inttoptr(&self, val: ObsVal, _dst_ty: ObsType) -> ObsVal {
        let v = self.with_func(|func| {
            let src = materialize_scalar(func, val.as_scalar());
            let src_ty = func.value_type(src);
            let i64_val = if src_ty == ir::Type::I64 {
                src
            } else {
                func.ext_u(ir::Type::I64, src)
            };
            func.ptr_from_int(i64_val)
        });
        ObsVal::ssa(v, ir::Type::Ptr)
    }

    pub fn bitcast(&self, val: ObsVal, dst_ty: ObsType) -> ObsVal {
        let ir_dst = dst_ty.as_ir_type();
        if val.as_scalar().ty() == ir_dst {
            return val;
        }
        let v = self.with_func(|func| {
            let src = materialize_scalar(func, val.as_scalar());
            func.bitcast(ir_dst, src)
        });
        ObsVal::ssa(v, ir_dst)
    }

    pub fn and(&self, lhs: ObsVal, rhs: ObsVal) -> ObsVal {
        if let (ObsVal::Pair(l0, l1), ObsVal::Pair(r0, r1)) = (lhs, rhs) {
            let a = self.and(ObsVal::Scalar(l0), ObsVal::Scalar(r0));
            let b = self.and(ObsVal::Scalar(l1), ObsVal::Scalar(r1));
            return ObsVal::Pair(a.as_scalar(), b.as_scalar());
        }
        let ty = lhs.as_scalar().ty();
        let v = self.with_func(|func| {
            let l = materialize_scalar(func, lhs.as_scalar());
            let r = materialize_scalar(func, rhs.as_scalar());
            func.and(ty, l, r)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn or(&self, lhs: ObsVal, rhs: ObsVal) -> ObsVal {
        if let (ObsVal::Pair(l0, l1), ObsVal::Pair(r0, r1)) = (lhs, rhs) {
            let a = self.or(ObsVal::Scalar(l0), ObsVal::Scalar(r0));
            let b = self.or(ObsVal::Scalar(l1), ObsVal::Scalar(r1));
            return ObsVal::Pair(a.as_scalar(), b.as_scalar());
        }
        let ty = lhs.as_scalar().ty();
        let v = self.with_func(|func| {
            let l = materialize_scalar(func, lhs.as_scalar());
            let r = materialize_scalar(func, rhs.as_scalar());
            func.or(ty, l, r)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn xor(&self, lhs: ObsVal, rhs: ObsVal) -> ObsVal {
        if let (ObsVal::Pair(l0, l1), ObsVal::Pair(r0, r1)) = (lhs, rhs) {
            let a = self.xor(ObsVal::Scalar(l0), ObsVal::Scalar(r0));
            let b = self.xor(ObsVal::Scalar(l1), ObsVal::Scalar(r1));
            return ObsVal::Pair(a.as_scalar(), b.as_scalar());
        }
        let ty = lhs.as_scalar().ty();
        let v = self.with_func(|func| {
            let l = materialize_scalar(func, lhs.as_scalar());
            let r = materialize_scalar(func, rhs.as_scalar());
            func.xor(ty, l, r)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn add(&self, lhs: ObsVal, rhs: ObsVal) -> ObsVal {
        if let (ObsVal::Pair(l0, l1), ObsVal::Pair(r0, r1)) = (lhs, rhs) {
            let (lo, hi) = self.with_func(|func| {
                let ll = materialize_scalar(func, l0);
                let lh = materialize_scalar(func, l1);
                let rl = materialize_scalar(func, r0);
                let rh = materialize_scalar(func, r1);
                let (sum_lo, carry) = func.add_ovf_u(ir::Type::I64, ll, rl);
                let carry64 = func.ext_u(ir::Type::I64, carry);
                let hi0 = func.add(ir::Type::I64, lh, rh);
                let sum_hi = func.add(ir::Type::I64, hi0, carry64);
                (sum_lo, sum_hi)
            });
            return ObsVal::Pair(
                ObsScalar::Ssa(lo, ir::Type::I64),
                ObsScalar::Ssa(hi, ir::Type::I64),
            );
        }
        let ty = lhs.as_scalar().ty();
        let v = self.with_func(|func| {
            let l = materialize_scalar(func, lhs.as_scalar());
            let r = materialize_scalar(func, rhs.as_scalar());
            func.add(ty, l, r)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn sub(&self, lhs: ObsVal, rhs: ObsVal) -> ObsVal {
        if let (ObsVal::Pair(l0, l1), ObsVal::Pair(r0, r1)) = (lhs, rhs) {
            let (lo, hi) = self.with_func(|func| {
                let ll = materialize_scalar(func, l0);
                let lh = materialize_scalar(func, l1);
                let rl = materialize_scalar(func, r0);
                let rh = materialize_scalar(func, r1);
                let (diff_lo, borrow) = func.sub_ovf_u(ir::Type::I64, ll, rl);
                let borrow64 = func.ext_u(ir::Type::I64, borrow);
                let hi0 = func.sub(ir::Type::I64, lh, rh);
                let diff_hi = func.sub(ir::Type::I64, hi0, borrow64);
                (diff_lo, diff_hi)
            });
            return ObsVal::Pair(
                ObsScalar::Ssa(lo, ir::Type::I64),
                ObsScalar::Ssa(hi, ir::Type::I64),
            );
        }
        let ty = lhs.as_scalar().ty();
        let v = self.with_func(|func| {
            let l = materialize_scalar(func, lhs.as_scalar());
            let r = materialize_scalar(func, rhs.as_scalar());
            func.sub(ty, l, r)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn mul(&self, lhs: ObsVal, rhs: ObsVal) -> ObsVal {
        if let (ObsVal::Pair(l0, l1), ObsVal::Pair(r0, r1)) = (lhs, rhs) {
            let (lo, hi) = self.with_func(|func| {
                let ll = materialize_scalar(func, l0);
                let lh = materialize_scalar(func, l1);
                let rl = materialize_scalar(func, r0);
                let rh = materialize_scalar(func, r1);
                let (prod_lo, prod_hi0) = func.mul_wide_u(ir::Type::I64, ll, rl);
                let c1 = func.mul(ir::Type::I64, ll, rh);
                let c2 = func.mul(ir::Type::I64, lh, rl);
                let h1 = func.add(ir::Type::I64, prod_hi0, c1);
                let prod_hi = func.add(ir::Type::I64, h1, c2);
                (prod_lo, prod_hi)
            });
            return ObsVal::Pair(
                ObsScalar::Ssa(lo, ir::Type::I64),
                ObsScalar::Ssa(hi, ir::Type::I64),
            );
        }
        let ty = lhs.as_scalar().ty();
        let v = self.with_func(|func| {
            let l = materialize_scalar(func, lhs.as_scalar());
            let r = materialize_scalar(func, rhs.as_scalar());
            func.mul(ty, l, r)
        });
        ObsVal::ssa(v, ty)
    }

    pub fn not(&self, val: ObsVal) -> ObsVal {
        if let ObsVal::Pair(s0, s1) = val {
            let a = self.not(ObsVal::Scalar(s0));
            let b = self.not(ObsVal::Scalar(s1));
            return ObsVal::Pair(a.as_scalar(), b.as_scalar());
        }
        let ty = val.as_scalar().ty();
        if ty == ir::Type::I1 {
            return self.xor(val, self.cx.const_bool(true));
        }
        let v = self.with_func(|func| {
            let src = materialize_scalar(func, val.as_scalar());
            func.not(ty, src)
        });
        ObsVal::ssa(v, ty)
    }
}
