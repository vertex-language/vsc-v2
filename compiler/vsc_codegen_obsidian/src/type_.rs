//! Obsidian type and constant construction helpers (`vsc_codegen_obsidian::type_`).

use obsidian::ir;

use crate::context::CodegenCx;
use crate::value::{ObsFnSig, ObsScalar, ObsType, ObsVal};

impl<'tcx> CodegenCx<'tcx> {
    #[inline]
    pub fn type_void(&self) -> ObsType {
        ObsType::Prim(ir::Type::Void)
    }

    #[inline]
    pub fn type_i1(&self) -> ObsType {
        ObsType::Prim(ir::Type::I1)
    }

    #[inline]
    pub fn type_i8(&self) -> ObsType {
        ObsType::Prim(ir::Type::I8)
    }

    #[inline]
    pub fn type_i16(&self) -> ObsType {
        ObsType::Prim(ir::Type::I16)
    }

    #[inline]
    pub fn type_i32(&self) -> ObsType {
        ObsType::Prim(ir::Type::I32)
    }

    #[inline]
    pub fn type_i64(&self) -> ObsType {
        ObsType::Prim(ir::Type::I64)
    }

    #[inline]
    pub fn type_i128(&self) -> ObsType {
        ObsType::Pair(ir::Type::I64, ir::Type::I64)
    }

    #[inline]
    pub fn type_isize(&self) -> ObsType {
        self.type_ix(self.tcx.target.pointer_width as u64)
    }

    #[inline]
    pub fn type_ix(&self, num_bits: u64) -> ObsType {
        match num_bits {
            1 => self.type_i1(),
            2..=8 => self.type_i8(),
            9..=16 => self.type_i16(),
            17..=32 => self.type_i32(),
            33..=64 => self.type_i64(),
            _ => self.type_i128(),
        }
    }

    #[inline]
    pub fn type_f16(&self) -> ObsType {
        ObsType::Prim(ir::Type::I16)
    }

    #[inline]
    pub fn type_f32(&self) -> ObsType {
        ObsType::Prim(ir::Type::F32)
    }

    #[inline]
    pub fn type_f64(&self) -> ObsType {
        ObsType::Prim(ir::Type::F64)
    }

    #[inline]
    pub fn type_x87_f80(&self) -> ObsType {
        ObsType::Prim(ir::Type::F64)
    }

    #[inline]
    pub fn type_f128(&self) -> ObsType {
        ObsType::Prim(ir::Type::F64)
    }

    #[inline]
    pub fn type_ptr(&self) -> ObsType {
        ObsType::Prim(ir::Type::Ptr)
    }

    #[inline]
    pub fn type_ptr_in(&self, _address_space: u32) -> ObsType {
        ObsType::Prim(ir::Type::Ptr)
    }

    #[inline]
    pub fn type_array(&self, elem: ObsType, len: u64) -> ObsType {
        ObsType::Array {
            elem: elem.as_ir_type(),
            count: len,
        }
    }

    #[inline]
    pub fn type_vector(&self, elem: ObsType, len: u64) -> ObsType {
        ObsType::Array {
            elem: elem.as_ir_type(),
            count: len,
        }
    }

    #[inline]
    pub fn type_struct(&self, els: &[ObsType], _packed: bool) -> ObsType {
        match els {
            [] => ObsType::Prim(ir::Type::Void),
            [only] => *only,
            [a, b] => ObsType::Pair(a.as_ir_type(), b.as_ir_type()),
            many => {
                let total_bytes: u64 = many.iter().map(|t| t.byte_size()).sum();
                ObsType::Array {
                    elem: ir::Type::I8,
                    count: total_bytes,
                }
            }
        }
    }

    #[inline]
    pub fn type_func(&self, args: &[ObsType], ret: ObsType, variadic: bool) -> ObsType {
        let mut params = Vec::with_capacity(args.len());
        for a in args {
            match *a {
                ObsType::Prim(ir::Type::Void) => {}
                ObsType::Prim(t) => params.push(t),
                ObsType::Pair(t0, t1) => {
                    params.push(t0);
                    params.push(t1);
                }
                ObsType::Array { elem, count } => {
                    for _ in 0..count {
                        params.push(elem);
                    }
                }
                ObsType::Func(_) => params.push(ir::Type::Ptr),
            }
        }
        let returns = match ret {
            ObsType::Prim(ir::Type::Void) => Vec::new(),
            ObsType::Prim(t) => vec![t],
            ObsType::Pair(t0, t1) => vec![t0, t1],
            ObsType::Array { elem, count } => {
                if count == 0 {
                    Vec::new()
                } else if count == 1 {
                    vec![elem]
                } else {
                    vec![ir::Type::I64, ir::Type::I64]
                }
            }
            ObsType::Func(_) => vec![ir::Type::Ptr],
        };
        let id = self.intern_fn_sig(ObsFnSig {
            params,
            returns,
            call_conv: ir::CallConv::C,
            is_variadic: variadic,
        });
        ObsType::Func(id)
    }

    #[inline]
    pub fn int_width(&self, ty: ObsType) -> u64 {
        if ty == ObsType::Pair(ir::Type::I64, ir::Type::I64) {
            return 128;
        }
        match ty.as_ir_type() {
            ir::Type::I1 => 1,
            ir::Type::I8 => 8,
            ir::Type::I16 => 16,
            ir::Type::I32 | ir::Type::F32 => 32,
            ir::Type::I64 | ir::Type::F64 | ir::Type::Ptr => 64,
            ir::Type::I128 | ir::Type::V128 => 128,
            ir::Type::Void => 0,
        }
    }

    #[inline]
    pub fn const_bool(&self, val: bool) -> ObsVal {
        ObsVal::const_bits(ir::Type::I1, u64::from(val))
    }

    #[inline]
    pub fn const_i8(&self, val: i8) -> ObsVal {
        ObsVal::const_bits(ir::Type::I8, (val as u8) as u64)
    }

    #[inline]
    pub fn const_u8(&self, val: u8) -> ObsVal {
        ObsVal::const_bits(ir::Type::I8, u64::from(val))
    }

    #[inline]
    pub fn const_i32(&self, val: i32) -> ObsVal {
        ObsVal::const_bits(ir::Type::I32, (val as u32) as u64)
    }

    #[inline]
    pub fn const_u32(&self, val: u32) -> ObsVal {
        ObsVal::const_bits(ir::Type::I32, u64::from(val))
    }

    #[inline]
    pub fn const_i64(&self, val: i64) -> ObsVal {
        ObsVal::const_bits(ir::Type::I64, val as u64)
    }

    #[inline]
    pub fn const_u64(&self, val: u64) -> ObsVal {
        ObsVal::const_bits(ir::Type::I64, val)
    }

    #[inline]
    pub fn const_usize(&self, val: u64) -> ObsVal {
        ObsVal::const_bits(self.type_isize().as_ir_type(), val)
    }

    #[inline]
    pub fn const_int(&self, ty: ObsType, val: i64) -> ObsVal {
        self.const_uint_big(ty, (val as i128) as u128)
    }

    #[inline]
    pub fn const_uint(&self, ty: ObsType, val: u64) -> ObsVal {
        self.const_uint_big(ty, u128::from(val))
    }

    #[inline]
    pub fn const_uint_big(&self, ty: ObsType, val: u128) -> ObsVal {
        if ty == ObsType::Pair(ir::Type::I64, ir::Type::I64) {
            return ObsVal::Pair(
                ObsScalar::ConstBits(ir::Type::I64, val as u64),
                ObsScalar::ConstBits(ir::Type::I64, (val >> 64) as u64),
            );
        }
        let ir_ty = ty.as_ir_type();
        let bits = self.int_width(ty);
        let masked = if bits == 0 || bits >= 64 {
            val as u64
        } else {
            (val as u64) & ((1u64 << bits) - 1)
        };
        ObsVal::const_bits(ir_ty, masked)
    }

    #[inline]
    pub fn const_real(&self, ty: ObsType, val: f64) -> ObsVal {
        match ty.as_ir_type() {
            ir::Type::F32 => ObsVal::const_bits(ir::Type::F32, (val as f32).to_bits() as u64),
            _ => ObsVal::const_bits(ir::Type::F64, val.to_bits()),
        }
    }

    #[inline]
    pub fn const_null(&self, ty: ObsType) -> ObsVal {
        match ty {
            ObsType::Pair(a, b) => ObsVal::Pair(ObsScalar::ConstBits(a, 0), ObsScalar::ConstBits(b, 0)),
            _ => ObsVal::const_bits(ty.as_ir_type(), 0),
        }
    }

    #[inline]
    pub fn const_undef(&self, ty: ObsType) -> ObsVal {
        match ty {
            ObsType::Pair(a, b) => ObsVal::Pair(ObsScalar::Undef(a), ObsScalar::Undef(b)),
            _ => ObsVal::undef(ty.as_ir_type()),
        }
    }

    #[inline]
    pub fn const_struct(&self, els: &[ObsVal], _packed: bool) -> ObsVal {
        match els {
            [] => ObsVal::undef(ir::Type::Void),
            [only] => *only,
            [a, b, ..] => ObsVal::Pair(a.as_scalar(), b.as_scalar()),
        }
    }
}
