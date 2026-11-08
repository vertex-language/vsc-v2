//! Arithmetic, comparisons and casts, with Rust's semantics: integers wrap
//! at their width, overflow is detected for the checked operations, float
//! to integer casts saturate.

use std::cmp::Ordering;

use vsc_middle::mir::{BinOp, CastKind, UnOp};
use vsc_middle::thir::PointerCoercion;
use vsc_middle::ty::{self, Ty, TyKind};

use super::*;

impl<'a, 'tcx> Interpreter<'a, 'tcx> {
    /// `l op r`, wrapping, and whether it overflowed.
    pub fn checked_op(&self, op: BinOp, l: Value<'tcx>, r: Value<'tcx>) -> InterpResult<(Value<'tcx>, bool)> {
        let (Value::Int(a), Value::Int(b)) = (&l, &r) else { return Ok((self.binary_op(op, l, r)?, false)) };
        let (x, y) = (a.to_i128(), b.to_i128());
        let exact = match op {
            BinOp::Add => x.checked_add(y),
            BinOp::Sub => x.checked_sub(y),
            BinOp::Mul => x.checked_mul(y),
            _ => return Ok((self.binary_op(op, l, r)?, false)),
        };
        let wrapped = match op {
            BinOp::Add => (a.bits).wrapping_add(b.bits),
            BinOp::Sub => (a.bits).wrapping_sub(b.bits),
            _ => (a.bits).wrapping_mul(b.bits),
        };
        let overflow = match exact {
            Some(v) => !Int::fits(v, a.size, a.signed),
            None => true,
        };
        Ok((Value::Int(Int::from_bits(wrapped, a.size, a.signed)), overflow))
    }

    pub fn binary_op(&self, op: BinOp, l: Value<'tcx>, r: Value<'tcx>) -> InterpResult<Value<'tcx>> {
        use BinOp::*;
        let cmp = |o: Ordering| -> Option<Value<'tcx>> {
            Some(Value::Bool(match op {
                Eq => o == Ordering::Equal,
                Ne => o != Ordering::Equal,
                Lt => o == Ordering::Less,
                Le => o != Ordering::Greater,
                Gt => o == Ordering::Greater,
                Ge => o != Ordering::Less,
                _ => return None,
            }))
        };
        match (&l, &r) {
            (Value::Int(a), Value::Int(b)) => {
                if op.is_comparison() {
                    return Ok(cmp(a.to_i128().cmp(&b.to_i128())).unwrap());
                }
                let (x, y) = (a.to_i128(), b.to_i128());
                let bits = match op {
                    Add => a.bits.wrapping_add(b.bits),
                    Sub => a.bits.wrapping_sub(b.bits),
                    Mul => a.bits.wrapping_mul(b.bits),
                    Div | Rem => {
                        if y == 0 {
                            return self.panic(if op == Div { "attempt to divide by zero" } else { "attempt to calculate the remainder with a divisor of zero" });
                        }
                        if a.signed && y == -1 && x == Int::min(a.size, true) {
                            return self.panic(if op == Div { "attempt to divide with overflow" } else { "attempt to calculate the remainder with overflow" });
                        }
                        (if op == Div { x.wrapping_div(y) } else { x.wrapping_rem(y) }) as u128
                    }
                    BitAnd => a.bits & b.bits,
                    BitOr => a.bits | b.bits,
                    BitXor => a.bits ^ b.bits,
                    Shl | Shr => {
                        if y < 0 || y >= a.size as i128 {
                            return self.panic(if op == Shl { "attempt to shift left with overflow" } else { "attempt to shift right with overflow" });
                        }
                        let s = y as u32;
                        if op == Shl { a.bits << s } else if a.signed { (x >> s) as u128 } else { a.bits >> s }
                    }
                    _ => return self.unsupported(format!("`{op:?}` on integers")),
                };
                Ok(Value::Int(Int::from_bits(bits, a.size, a.signed)))
            }
            (Value::F64(a), Value::F64(b)) => float_op(op, *a, *b).map(Value::F64).or_else(|| cmp_f(op, a.partial_cmp(b))).ok_or(()).or_else(|_| self.unsupported("this float operation")),
            (Value::F32(a), Value::F32(b)) => {
                float_op(op, *a as f64, *b as f64).map(|v| Value::F32(v as f32)).or_else(|| cmp_f(op, a.partial_cmp(b))).ok_or(()).or_else(|_| self.unsupported("this float operation"))
            }
            (Value::Bool(a), Value::Bool(b)) => match op {
                BitAnd => Ok(Value::Bool(a & b)),
                BitOr => Ok(Value::Bool(a | b)),
                BitXor => Ok(Value::Bool(a ^ b)),
                _ => Ok(cmp(a.cmp(b)).unwrap()),
            },
            (Value::Char(a), Value::Char(b)) => Ok(cmp(a.cmp(b)).unwrap()),
            // String slices compare by contents (a pattern on `&string`).
            (Value::SlicePtr { .. }, Value::SlicePtr { .. }) if matches!(op, Eq | Ne) => {
                let (x, y) = (self.read_str(&l)?, self.read_str(&r)?);
                Ok(Value::Bool((x == y) == (op == Eq)))
            }
            (Value::Ptr(a), Value::Ptr(b)) if matches!(op, Eq | Ne) => Ok(Value::Bool((a == b) == (op == Eq))),
            _ => self.ub(format!("`{op:?}` on values of different kinds")),
        }
    }

    pub fn unary_op(&self, op: UnOp, v: Value<'tcx>) -> InterpResult<Value<'tcx>> {
        match (op, v) {
            (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
            (UnOp::Not, Value::Int(i)) => Ok(Value::Int(Int::from_bits(!i.bits, i.size, i.signed))),
            (UnOp::Neg, Value::Int(i)) => {
                if i.signed && i.to_i128() == Int::min(i.size, true) {
                    return self.panic("attempt to negate with overflow");
                }
                Ok(Value::Int(Int::from_bits(i.bits.wrapping_neg(), i.size, i.signed)))
            }
            (UnOp::Neg, Value::F64(f)) => Ok(Value::F64(-f)),
            (UnOp::Neg, Value::F32(f)) => Ok(Value::F32(-f)),
            _ => self.ub("a unary operator on the wrong kind of value"),
        }
    }

    pub fn cast(&self, kind: CastKind, v: Value<'tcx>, to: Ty<'tcx>) -> InterpResult<Value<'tcx>> {
        match kind {
            CastKind::IntToInt => {
                let x: i128 = match v {
                    Value::Int(i) => i.to_i128(),
                    Value::Bool(b) => b as i128,
                    Value::Char(c) => c as i128,
                    _ => return self.ub("an integer cast of a non-integer"),
                };
                if let TyKind::Char = to.kind() {
                    return Ok(Value::Char(char::from_u32(x as u32).unwrap_or('\u{FFFD}')));
                }
                match self.int_size(to) {
                    Some((size, signed)) => Ok(Value::Int(Int::new(x, size, signed))),
                    None => self.unsupported("this integer cast"),
                }
            }
            CastKind::IntToFloat => {
                let Value::Int(i) = v else { return self.ub("a float cast of a non-integer") };
                let x = if i.signed { i.to_i128() as f64 } else { i.bits as f64 };
                Ok(match to.kind() {
                    TyKind::Float(ty::FloatTy::F32) => Value::F32(x as f32),
                    _ => Value::F64(x),
                })
            }
            CastKind::FloatToInt => {
                let f = match v {
                    Value::F64(f) => f,
                    Value::F32(f) => f as f64,
                    _ => return self.ub("an integer cast of a non-float"),
                };
                let Some((size, signed)) = self.int_size(to) else { return self.unsupported("this float cast") };
                // Saturating, NaN to 0, as Rust's `as`.
                let v = if f.is_nan() {
                    0
                } else if signed {
                    let (min, max) = if size >= 128 { (i128::MIN, i128::MAX) } else { (-(1i128 << (size - 1)), (1i128 << (size - 1)) - 1) };
                    (f.trunc().clamp(min as f64, max as f64) as i128).clamp(min, max)
                } else {
                    let max = Int::mask(size);
                    (f.trunc().max(0.0).min(max as f64) as u128).min(max) as i128
                };
                Ok(Value::Int(Int::new(v, size, signed)))
            }
            CastKind::FloatToFloat => {
                let f = match v {
                    Value::F64(f) => f,
                    Value::F32(f) => f as f64,
                    _ => return self.ub("a float cast of a non-float"),
                };
                Ok(match to.kind() {
                    TyKind::Float(ty::FloatTy::F32) => Value::F32(f as f32),
                    _ => Value::F64(f),
                })
            }
            CastKind::PtrToPtr => Ok(v),
            CastKind::PointerCoercion(PointerCoercion::Unsize) => match v {
                Value::Ptr(p) => {
                    let len = self.array_len(&Location { alloc: p.alloc, path: p.path.clone(), slice: None })?;
                    Ok(Value::SlicePtr { array: p, start: 0, len })
                }
                other => Ok(other),
            },
            CastKind::PointerCoercion(PointerCoercion::ReifyFnPointer) => Ok(v),
            CastKind::PointerCoercion(PointerCoercion::ToFunc) => Ok(Value::Func(Box::new(v))),
            CastKind::PointerExposeProvenance | CastKind::PointerWithExposedProvenance => {
                self.unsupported("casting between pointers and integers")
            }
        }
    }
}

fn float_op(op: BinOp, a: f64, b: f64) -> Option<f64> {
    Some(match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Rem => a % b,
        _ => return None,
    })
}

fn cmp_f<'tcx>(op: BinOp, o: Option<Ordering>) -> Option<Value<'tcx>> {
    let Some(o) = o else {
        // NaN: only `!=` holds.
        return op.is_comparison().then_some(Value::Bool(op == BinOp::Ne));
    };
    Some(Value::Bool(match op {
        BinOp::Eq => o == Ordering::Equal,
        BinOp::Ne => o != Ordering::Equal,
        BinOp::Lt => o == Ordering::Less,
        BinOp::Le => o != Ordering::Greater,
        BinOp::Gt => o == Ordering::Greater,
        BinOp::Ge => o != Ordering::Less,
        _ => return None,
    }))
}
