//! MIR binary operations, checked arithmetic, 128-bit software arithmetic, and slice byte equality.

use obsidian::ir;
use vsc_abi::Align;
use vsc_codegen_ssa::common::IntPredicate;
use vsc_codegen_ssa::mir::{OperandRef, OperandValue};
use vsc_middle::mir::BinOp;
use vsc_middle::ty::{Ty, TyKind};

use super::{
    Builder, int_predicate, is_str_slice_ref, materialize_scalar, real_predicate, ty_is_signed,
};
use crate::value::{ObsScalar, ObsType, ObsVal};

impl<'a, 'tcx> Builder<'a, 'tcx> {
    pub fn codegen_binary_op(
        &self,
        op: BinOp,
        lhs: OperandRef<'tcx, ObsVal>,
        rhs: OperandRef<'tcx, ObsVal>,
        _dst_ty: Ty<'tcx>,
    ) -> OperandValue<ObsVal> {
        if matches!(op, BinOp::Eq | BinOp::Ne) && is_str_slice_ref(lhs.layout.ty) {
            if let (OperandValue::Pair(pa, la), OperandValue::Pair(pb, lb)) = (lhs.val, rhs.val) {
                let eq = self.codegen_slice_bytes_eq(pa, la, pb, lb);
                let res = if op == BinOp::Ne { self.not(eq) } else { eq };
                return OperandValue::Immediate(res);
            }
        }

        if op == BinOp::Offset {
            let OperandValue::Immediate(ptr) = lhs.val else {
                panic!("Offset lhs not ptr");
            };
            let OperandValue::Immediate(count) = rhs.val else {
                panic!("Offset rhs not scalar");
            };
            let ptee_ty = match *lhs.layout.ty.kind() {
                TyKind::RawPtr(t, _) | TyKind::Ref(_, t, _) => t,
                other => panic!("Offset on non-pointer {other:?}"),
            };
            let elem_size = self.cx.layout_of(ptee_ty).size.bytes();
            if elem_size == 0 {
                return OperandValue::Immediate(ptr);
            }
            let idx = self.intcast(count, self.cx.type_isize(), ty_is_signed(rhs.layout.ty));
            let bytes = self.mul(idx, self.cx.const_usize(elem_size));
            return OperandValue::Immediate(self.ptradd_dyn(ptr, bytes));
        }

        let (OperandValue::Immediate(l), OperandValue::Immediate(r)) = (lhs.val, rhs.val) else {
            panic!(
                "non-scalar operands to binary op {op:?}: {:?}",
                (lhs.layout.ty, rhs.layout.ty)
            );
        };
        let is_float = matches!(lhs.layout.ty.kind(), TyKind::Float(_));
        let signed = ty_is_signed(lhs.layout.ty);
        let ty = l.as_scalar().ty();

        let out = if matches!(
            op,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        ) {
            if is_float {
                self.fcmp(real_predicate(op), l, r)
            } else {
                self.icmp(int_predicate(op, signed), l, r)
            }
        } else if is_float {
            match op {
                BinOp::Add => {
                    let v = self.with_func(|func| {
                        let lv = materialize_scalar(func, l.as_scalar());
                        let rv = materialize_scalar(func, r.as_scalar());
                        func.fadd(ty, lv, rv)
                    });
                    ObsVal::ssa(v, ty)
                }
                BinOp::Sub => {
                    let v = self.with_func(|func| {
                        let lv = materialize_scalar(func, l.as_scalar());
                        let rv = materialize_scalar(func, r.as_scalar());
                        func.fsub(ty, lv, rv)
                    });
                    ObsVal::ssa(v, ty)
                }
                BinOp::Mul => {
                    let v = self.with_func(|func| {
                        let lv = materialize_scalar(func, l.as_scalar());
                        let rv = materialize_scalar(func, r.as_scalar());
                        func.fmul(ty, lv, rv)
                    });
                    ObsVal::ssa(v, ty)
                }
                BinOp::Div => {
                    let v = self.with_func(|func| {
                        let lv = materialize_scalar(func, l.as_scalar());
                        let rv = materialize_scalar(func, r.as_scalar());
                        func.fdiv(ty, lv, rv)
                    });
                    ObsVal::ssa(v, ty)
                }
                BinOp::Rem => {
                    let (fmod_ty, fmod_fn) = self.cx.c_fmod(ty == ir::Type::F32);
                    self.call_raw(fmod_ty, fmod_fn, &mut [l, r])
                }
                other => panic!("unsupported float binop {other:?}"),
            }
        } else if matches!(l, ObsVal::Pair(..)) {
            match op {
                BinOp::Add => self.add(l, r),
                BinOp::Sub => self.sub(l, r),
                BinOp::Mul => self.mul(l, r),
                BinOp::BitAnd => self.and(l, r),
                BinOp::BitOr => self.or(l, r),
                BinOp::BitXor => self.xor(l, r),
                BinOp::Div => self.div_rem_128(l, r, signed, false),
                BinOp::Rem => self.div_rem_128(l, r, signed, true),
                BinOp::Shl => self.shl_128(l, r),
                BinOp::Shr => self.shr_128(l, r, signed),
                other => panic!("unsupported 128-bit int binop {other:?}"),
            }
        } else {
            match op {
                BinOp::Add => self.add(l, r),
                BinOp::Sub => self.sub(l, r),
                BinOp::Mul => self.mul(l, r),
                BinOp::Div => {
                    let v = self.with_func(|func| {
                        let lv = materialize_scalar(func, l.as_scalar());
                        let rv = materialize_scalar(func, r.as_scalar());
                        if signed {
                            func.div_s(ty, lv, rv)
                        } else {
                            func.div_u(ty, lv, rv)
                        }
                    });
                    ObsVal::ssa(v, ty)
                }
                BinOp::Rem => {
                    let v = self.with_func(|func| {
                        let lv = materialize_scalar(func, l.as_scalar());
                        let rv = materialize_scalar(func, r.as_scalar());
                        if signed {
                            func.rem_s(ty, lv, rv)
                        } else {
                            func.rem_u(ty, lv, rv)
                        }
                    });
                    ObsVal::ssa(v, ty)
                }
                BinOp::BitAnd => self.and(l, r),
                BinOp::BitOr => self.or(l, r),
                BinOp::BitXor => self.xor(l, r),
                BinOp::Shl => {
                    let r_cast = self.intcast(r, ObsType::Prim(ty), false);
                    let v = self.with_func(|func| {
                        let lv = materialize_scalar(func, l.as_scalar());
                        let rv = materialize_scalar(func, r_cast.as_scalar());
                        func.shl(ty, lv, rv)
                    });
                    ObsVal::ssa(v, ty)
                }
                BinOp::Shr => {
                    let r_cast = self.intcast(r, ObsType::Prim(ty), false);
                    let v = self.with_func(|func| {
                        let lv = materialize_scalar(func, l.as_scalar());
                        let rv = materialize_scalar(func, r_cast.as_scalar());
                        if signed {
                            func.shr_s(ty, lv, rv)
                        } else {
                            func.shr_u(ty, lv, rv)
                        }
                    });
                    ObsVal::ssa(v, ty)
                }
                other => panic!("unsupported int binop {other:?}"),
            }
        };
        OperandValue::Immediate(out)
    }

    pub fn codegen_checked_binary_op(
        &self,
        op: BinOp,
        lhs: OperandRef<'tcx, ObsVal>,
        rhs: OperandRef<'tcx, ObsVal>,
    ) -> OperandValue<ObsVal> {
        let (OperandValue::Immediate(l), OperandValue::Immediate(r)) = (lhs.val, rhs.val) else {
            panic!("non-scalar operands to checked binary op {op:?}");
        };
        let signed = ty_is_signed(lhs.layout.ty);
        if matches!(l, ObsVal::Pair(..)) {
            return self.codegen_checked_binary_op_128(op, l, r, signed);
        }
        let ir_ty = l.as_scalar().ty();
        let llty = ObsType::Prim(ir_ty);

        match (op, signed) {
            (BinOp::Add, true) => {
                let (res, ovf) = self.with_func(|func| {
                    let lv = materialize_scalar(func, l.as_scalar());
                    let rv = materialize_scalar(func, r.as_scalar());
                    func.add_ovf_s(ir_ty, lv, rv)
                });
                OperandValue::Pair(ObsVal::ssa(res, ir_ty), ObsVal::ssa(ovf, ir::Type::I1))
            }
            (BinOp::Add, false) => {
                let (res, ovf) = self.with_func(|func| {
                    let lv = materialize_scalar(func, l.as_scalar());
                    let rv = materialize_scalar(func, r.as_scalar());
                    func.add_ovf_u(ir_ty, lv, rv)
                });
                OperandValue::Pair(ObsVal::ssa(res, ir_ty), ObsVal::ssa(ovf, ir::Type::I1))
            }
            (BinOp::Sub, true) => {
                let (res, ovf) = self.with_func(|func| {
                    let lv = materialize_scalar(func, l.as_scalar());
                    let rv = materialize_scalar(func, r.as_scalar());
                    func.sub_ovf_s(ir_ty, lv, rv)
                });
                OperandValue::Pair(ObsVal::ssa(res, ir_ty), ObsVal::ssa(ovf, ir::Type::I1))
            }
            (BinOp::Sub, false) => {
                let (res, ovf) = self.with_func(|func| {
                    let lv = materialize_scalar(func, l.as_scalar());
                    let rv = materialize_scalar(func, r.as_scalar());
                    func.sub_ovf_u(ir_ty, lv, rv)
                });
                OperandValue::Pair(ObsVal::ssa(res, ir_ty), ObsVal::ssa(ovf, ir::Type::I1))
            }
            (BinOp::Mul, true) => {
                let (res, ovf) = self.with_func(|func| {
                    let lv = materialize_scalar(func, l.as_scalar());
                    let rv = materialize_scalar(func, r.as_scalar());
                    func.mul_ovf_s(ir_ty, lv, rv)
                });
                OperandValue::Pair(ObsVal::ssa(res, ir_ty), ObsVal::ssa(ovf, ir::Type::I1))
            }
            (BinOp::Mul, false) => {
                let (res, ovf) = self.with_func(|func| {
                    let lv = materialize_scalar(func, l.as_scalar());
                    let rv = materialize_scalar(func, r.as_scalar());
                    func.mul_ovf_u(ir_ty, lv, rv)
                });
                OperandValue::Pair(ObsVal::ssa(res, ir_ty), ObsVal::ssa(ovf, ir::Type::I1))
            }
            (BinOp::Shl, _) => {
                let width = self.cx.int_width(llty);
                let r_cast = self.intcast(r, llty, false);
                let mask = self.cx.const_uint(llty, width - 1);
                let masked = self.and(r_cast, mask);
                let val = self.with_func(|func| {
                    let lv = materialize_scalar(func, l.as_scalar());
                    let rv = materialize_scalar(func, masked.as_scalar());
                    func.shl(ir_ty, lv, rv)
                });
                let ovf = self.icmp(IntPredicate::IntUGE, r_cast, self.cx.const_uint(llty, width));
                OperandValue::Pair(ObsVal::ssa(val, ir_ty), ovf)
            }
            (BinOp::Shr, _) => {
                let width = self.cx.int_width(llty);
                let r_cast = self.intcast(r, llty, false);
                let mask = self.cx.const_uint(llty, width - 1);
                let masked = self.and(r_cast, mask);
                let val = self.with_func(|func| {
                    let lv = materialize_scalar(func, l.as_scalar());
                    let rv = materialize_scalar(func, masked.as_scalar());
                    if signed {
                        func.shr_s(ir_ty, lv, rv)
                    } else {
                        func.shr_u(ir_ty, lv, rv)
                    }
                });
                let ovf = self.icmp(IntPredicate::IntUGE, r_cast, self.cx.const_uint(llty, width));
                OperandValue::Pair(ObsVal::ssa(val, ir_ty), ovf)
            }
            _ => {
                let val = match self.codegen_binary_op(op, lhs, rhs, lhs.layout.ty) {
                    OperandValue::Immediate(v) => v,
                    _ => unreachable!(),
                };
                OperandValue::Pair(val, self.cx.const_bool(false))
            }
        }
    }

    pub(super) fn shl_128(&self, lhs: ObsVal, rhs: ObsVal) -> ObsVal {
        let ObsVal::Pair(l0, l1) = lhs else {
            unreachable!();
        };
        let sh64 = self.intcast(rhs, self.cx.type_i64(), false);
        let (res_lo, res_hi) = self.with_func(|func| {
            let lo = materialize_scalar(func, l0);
            let hi = materialize_scalar(func, l1);
            let s_raw = materialize_scalar(func, sh64.as_scalar());
            let c127 = func.i64_const(127);
            let c63 = func.i64_const(63);
            let c64 = func.i64_const(64);
            let z = func.i64_const(0);
            let s = func.and(ir::Type::I64, s_raw, c127);
            let is_zero = func.icmp(ir::Type::I64, ir::IntCC::Eq, s, z);
            let ge_64 = func.icmp(ir::Type::I64, ir::IntCC::Uge, s, c64);
            let s_mod = func.and(ir::Type::I64, s, c63);
            let inv = func.sub(ir::Type::I64, c64, s_mod);

            // 0 < s < 64:
            let lo_sm = func.shl(ir::Type::I64, lo, s_mod);
            let hi_sh = func.shl(ir::Type::I64, hi, s_mod);
            let lo_carry = func.shr_u(ir::Type::I64, lo, inv);
            let hi_sm = func.or(ir::Type::I64, hi_sh, lo_carry);
            // s == 0 guard:
            let lo_sm0 = func.select(ir::Type::I64, is_zero, lo, lo_sm);
            let hi_sm0 = func.select(ir::Type::I64, is_zero, hi, hi_sm);
            // 64 <= s < 128:
            let hi_big = func.shl(ir::Type::I64, lo, s_mod);
            let out_lo = func.select(ir::Type::I64, ge_64, z, lo_sm0);
            let out_hi = func.select(ir::Type::I64, ge_64, hi_big, hi_sm0);
            (out_lo, out_hi)
        });
        ObsVal::Pair(
            ObsScalar::Ssa(res_lo, ir::Type::I64),
            ObsScalar::Ssa(res_hi, ir::Type::I64),
        )
    }

    pub(super) fn shr_128(&self, lhs: ObsVal, rhs: ObsVal, signed: bool) -> ObsVal {
        let ObsVal::Pair(l0, l1) = lhs else {
            unreachable!();
        };
        let sh64 = self.intcast(rhs, self.cx.type_i64(), false);
        let (res_lo, res_hi) = self.with_func(|func| {
            let lo = materialize_scalar(func, l0);
            let hi = materialize_scalar(func, l1);
            let s_raw = materialize_scalar(func, sh64.as_scalar());
            let c127 = func.i64_const(127);
            let c63 = func.i64_const(63);
            let c64 = func.i64_const(64);
            let z = func.i64_const(0);
            let s = func.and(ir::Type::I64, s_raw, c127);
            let is_zero = func.icmp(ir::Type::I64, ir::IntCC::Eq, s, z);
            let ge_64 = func.icmp(ir::Type::I64, ir::IntCC::Uge, s, c64);
            let s_mod = func.and(ir::Type::I64, s, c63);
            let inv = func.sub(ir::Type::I64, c64, s_mod);

            // 0 < s < 64:
            let lo_sh = func.shr_u(ir::Type::I64, lo, s_mod);
            let hi_carry = func.shl(ir::Type::I64, hi, inv);
            let lo_sm = func.or(ir::Type::I64, lo_sh, hi_carry);
            let hi_sm = if signed {
                func.shr_s(ir::Type::I64, hi, s_mod)
            } else {
                func.shr_u(ir::Type::I64, hi, s_mod)
            };
            // s == 0 guard:
            let lo_sm0 = func.select(ir::Type::I64, is_zero, lo, lo_sm);
            let hi_sm0 = func.select(ir::Type::I64, is_zero, hi, hi_sm);
            // 64 <= s < 128:
            let lo_big = if signed {
                func.shr_s(ir::Type::I64, hi, s_mod)
            } else {
                func.shr_u(ir::Type::I64, hi, s_mod)
            };
            let hi_big = if signed {
                func.shr_s(ir::Type::I64, hi, c63)
            } else {
                z
            };
            let out_lo = func.select(ir::Type::I64, ge_64, lo_big, lo_sm0);
            let out_hi = func.select(ir::Type::I64, ge_64, hi_big, hi_sm0);
            (out_lo, out_hi)
        });
        ObsVal::Pair(
            ObsScalar::Ssa(res_lo, ir::Type::I64),
            ObsScalar::Ssa(res_hi, ir::Type::I64),
        )
    }

    fn div_rem_128(&self, lhs: ObsVal, rhs: ObsVal, signed: bool, want_rem: bool) -> ObsVal {
        let zero128 = self.cx.const_uint_big(self.cx.type_i128(), 0);
        let (eff_l, eff_r, l_neg, q_neg) = if signed {
            let l_neg = self.icmp(IntPredicate::IntSLT, lhs, zero128);
            let r_neg = self.icmp(IntPredicate::IntSLT, rhs, zero128);
            let abs_l = self.select(l_neg, self.sub(zero128, lhs), lhs);
            let abs_r = self.select(r_neg, self.sub(zero128, rhs), rhs);
            let q_neg = self.xor(l_neg, r_neg);
            (abs_l, abs_r, Some(l_neg), Some(q_neg))
        } else {
            (lhs, rhs, None, None)
        };

        let rem_slot = self.alloca(self.cx.type_i128(), Align::EIGHT, "rem128_out");
        let (fn_ty, fn_val) = self.cx.udivrem128_fn();
        let q_u = self.call_raw(fn_ty, fn_val, &mut [eff_l, eff_r, rem_slot]);
        if want_rem {
            let r_u = self.load(self.cx.type_i128(), rem_slot, Align::EIGHT);
            if let Some(ln) = l_neg {
                self.select(ln, self.sub(zero128, r_u), r_u)
            } else {
                r_u
            }
        } else if let Some(qn) = q_neg {
            self.select(qn, self.sub(zero128, q_u), q_u)
        } else {
            q_u
        }
    }

    fn codegen_checked_binary_op_128(
        &self,
        op: BinOp,
        l: ObsVal,
        r: ObsVal,
        signed: bool,
    ) -> OperandValue<ObsVal> {
        let (ObsVal::Pair(l0, l1), ObsVal::Pair(r0, r1)) = (l, r) else {
            // Shifts can have a narrower `rhs` (`u32`, `usize`, etc.):
            if matches!(op, BinOp::Shl | BinOp::Shr) {
                let res = if op == BinOp::Shl {
                    self.shl_128(l, r)
                } else {
                    self.shr_128(l, r, signed)
                };
                let r_u128 = self.intcast(r, self.cx.type_i128(), false);
                let c128 = self.cx.const_uint_big(self.cx.type_i128(), 128);
                let ovf = self.icmp(IntPredicate::IntUGE, r_u128, c128);
                return OperandValue::Pair(res, ovf);
            }
            unreachable!("expected 128-bit pair operands for {op:?}");
        };

        match (op, signed) {
            (BinOp::Add, false) => {
                let (lo, hi, ovf) = self.with_func(|func| {
                    let ll = materialize_scalar(func, l0);
                    let lh = materialize_scalar(func, l1);
                    let rl = materialize_scalar(func, r0);
                    let rh = materialize_scalar(func, r1);
                    let (sum_lo, c1) = func.add_ovf_u(ir::Type::I64, ll, rl);
                    let c1_64 = func.ext_u(ir::Type::I64, c1);
                    let (hi1, c2) = func.add_ovf_u(ir::Type::I64, lh, rh);
                    let (sum_hi, c3) = func.add_ovf_u(ir::Type::I64, hi1, c1_64);
                    let ovf = func.or(ir::Type::I1, c2, c3);
                    (sum_lo, sum_hi, ovf)
                });
                OperandValue::Pair(
                    ObsVal::Pair(
                        ObsScalar::Ssa(lo, ir::Type::I64),
                        ObsScalar::Ssa(hi, ir::Type::I64),
                    ),
                    ObsVal::ssa(ovf, ir::Type::I1),
                )
            }
            (BinOp::Add, true) => {
                let sum = self.add(l, r);
                let ObsVal::Pair(_, sum_hi) = sum else {
                    unreachable!();
                };
                let ovf = self.with_func(|func| {
                    let lh = materialize_scalar(func, l1);
                    let rh = materialize_scalar(func, r1);
                    let sh = materialize_scalar(func, sum_hi);
                    let x1 = func.xor(ir::Type::I64, lh, sh);
                    let x2 = func.xor(ir::Type::I64, rh, sh);
                    let both = func.and(ir::Type::I64, x1, x2);
                    let z = func.i64_const(0);
                    func.icmp(ir::Type::I64, ir::IntCC::Slt, both, z)
                });
                OperandValue::Pair(sum, ObsVal::ssa(ovf, ir::Type::I1))
            }
            (BinOp::Sub, false) => {
                let (lo, hi, ovf) = self.with_func(|func| {
                    let ll = materialize_scalar(func, l0);
                    let lh = materialize_scalar(func, l1);
                    let rl = materialize_scalar(func, r0);
                    let rh = materialize_scalar(func, r1);
                    let (diff_lo, b1) = func.sub_ovf_u(ir::Type::I64, ll, rl);
                    let b1_64 = func.ext_u(ir::Type::I64, b1);
                    let (hi1, b2) = func.sub_ovf_u(ir::Type::I64, lh, rh);
                    let (diff_hi, b3) = func.sub_ovf_u(ir::Type::I64, hi1, b1_64);
                    let ovf = func.or(ir::Type::I1, b2, b3);
                    (diff_lo, diff_hi, ovf)
                });
                OperandValue::Pair(
                    ObsVal::Pair(
                        ObsScalar::Ssa(lo, ir::Type::I64),
                        ObsScalar::Ssa(hi, ir::Type::I64),
                    ),
                    ObsVal::ssa(ovf, ir::Type::I1),
                )
            }
            (BinOp::Sub, true) => {
                let diff = self.sub(l, r);
                let ObsVal::Pair(_, diff_hi) = diff else {
                    unreachable!();
                };
                let ovf = self.with_func(|func| {
                    let lh = materialize_scalar(func, l1);
                    let rh = materialize_scalar(func, r1);
                    let dh = materialize_scalar(func, diff_hi);
                    let x1 = func.xor(ir::Type::I64, lh, rh);
                    let x2 = func.xor(ir::Type::I64, lh, dh);
                    let both = func.and(ir::Type::I64, x1, x2);
                    let z = func.i64_const(0);
                    func.icmp(ir::Type::I64, ir::IntCC::Slt, both, z)
                });
                OperandValue::Pair(diff, ObsVal::ssa(ovf, ir::Type::I1))
            }
            (BinOp::Mul, false) => {
                let (lo, hi, ovf) = self.with_func(|func| {
                    let ll = materialize_scalar(func, l0);
                    let lh = materialize_scalar(func, l1);
                    let rl = materialize_scalar(func, r0);
                    let rh = materialize_scalar(func, r1);
                    let z = func.i64_const(0);
                    let (prod_lo, hi0) = func.mul_wide_u(ir::Type::I64, ll, rl);
                    let (p1, p1_hi) = func.mul_wide_u(ir::Type::I64, ll, rh);
                    let (p2, p2_hi) = func.mul_wide_u(ir::Type::I64, lh, rl);
                    let (hi1, c1) = func.add_ovf_u(ir::Type::I64, hi0, p1);
                    let (prod_hi, c2) = func.add_ovf_u(ir::Type::I64, hi1, p2);
                    let lh_nz = func.icmp(ir::Type::I64, ir::IntCC::Ne, lh, z);
                    let rh_nz = func.icmp(ir::Type::I64, ir::IntCC::Ne, rh, z);
                    let both_hi = func.and(ir::Type::I1, lh_nz, rh_nz);
                    let p1_ovf = func.icmp(ir::Type::I64, ir::IntCC::Ne, p1_hi, z);
                    let p2_ovf = func.icmp(ir::Type::I64, ir::IntCC::Ne, p2_hi, z);
                    let o1 = func.or(ir::Type::I1, both_hi, p1_ovf);
                    let o2 = func.or(ir::Type::I1, p2_ovf, c1);
                    let o3 = func.or(ir::Type::I1, o1, o2);
                    let ovf = func.or(ir::Type::I1, o3, c2);
                    (prod_lo, prod_hi, ovf)
                });
                OperandValue::Pair(
                    ObsVal::Pair(
                        ObsScalar::Ssa(lo, ir::Type::I64),
                        ObsScalar::Ssa(hi, ir::Type::I64),
                    ),
                    ObsVal::ssa(ovf, ir::Type::I1),
                )
            }
            (BinOp::Mul, true) => {
                let zero128 = self.cx.const_uint_big(self.cx.type_i128(), 0);
                let l_neg = self.icmp(IntPredicate::IntSLT, l, zero128);
                let r_neg = self.icmp(IntPredicate::IntSLT, r, zero128);
                let abs_l = self.select(l_neg, self.sub(zero128, l), l);
                let abs_r = self.select(r_neg, self.sub(zero128, r), r);
                let sign = self.xor(l_neg, r_neg);
                let OperandValue::Pair(abs_prod, u_ovf) =
                    self.codegen_checked_binary_op_128(BinOp::Mul, abs_l, abs_r, false)
                else {
                    unreachable!();
                };
                let min_u128 = self.cx.const_uint_big(self.cx.type_i128(), 1u128 << 127);
                let gt_min = self.icmp(IntPredicate::IntUGT, abs_prod, min_u128);
                let ge_min = self.icmp(IntPredicate::IntUGE, abs_prod, min_u128);
                let range_ovf = self.select(sign, gt_min, ge_min);
                let ovf = self.or(u_ovf, range_ovf);
                let res = self.select(sign, self.sub(zero128, abs_prod), abs_prod);
                OperandValue::Pair(res, ovf)
            }
            (BinOp::Shl, _) => {
                let res = self.shl_128(l, r);
                let c128 = self.cx.const_uint_big(self.cx.type_i128(), 128);
                let ovf = self.icmp(IntPredicate::IntUGE, r, c128);
                OperandValue::Pair(res, ovf)
            }
            (BinOp::Shr, _) => {
                let res = self.shr_128(l, r, signed);
                let c128 = self.cx.const_uint_big(self.cx.type_i128(), 128);
                let ovf = self.icmp(IntPredicate::IntUGE, r, c128);
                OperandValue::Pair(res, ovf)
            }
            _ => {
                let res = match op {
                    BinOp::Div => self.div_rem_128(l, r, signed, false),
                    BinOp::Rem => self.div_rem_128(l, r, signed, true),
                    _ => self.add(l, r),
                };
                OperandValue::Pair(res, self.cx.const_bool(false))
            }
        }
    }

    fn codegen_slice_bytes_eq(&self, pa: ObsVal, la: ObsVal, pb: ObsVal, lb: ObsVal) -> ObsVal {
        let len_eq = self.icmp(IntPredicate::IntEQ, la, lb);
        let eff_len = self.select(len_eq, la, self.cx.const_usize(0));
        let memcmp = self.cx.c_memcmp();
        let ptr = self.cx.type_ptr();
        let memcmp_ty =
            self.cx
                .type_func(&[ptr, ptr, self.cx.type_isize()], self.cx.type_i32(), false);
        let cmp = self.call_raw(memcmp_ty, memcmp, &mut [pa, pb, eff_len]);
        let bytes_eq = self.icmp(IntPredicate::IntEQ, cmp, self.cx.const_i32(0));
        self.and(len_eq, bytes_eq)
    }
}
