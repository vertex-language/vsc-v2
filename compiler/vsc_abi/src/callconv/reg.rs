//! Abstract register descriptors for ABI cast targets (`rustc_abi::callconv::reg`).

use crate::{Float, HasDataLayout, Integer, Primitive, Size};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum RegKind {
    Integer,
    Float,
    Vector { hint_vector_elem: Primitive },
}

impl RegKind {
    pub fn from_primitive(p: Primitive) -> Self {
        match p {
            Primitive::Int(..) | Primitive::Pointer(_) => RegKind::Integer,
            Primitive::Float(_) => RegKind::Float,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Reg {
    pub kind: RegKind,
    pub size: Size,
}

impl Reg {
    pub const fn i8() -> Reg {
        Reg { kind: RegKind::Integer, size: Size::from_bytes(1) }
    }

    pub const fn i16() -> Reg {
        Reg { kind: RegKind::Integer, size: Size::from_bytes(2) }
    }

    pub const fn i32() -> Reg {
        Reg { kind: RegKind::Integer, size: Size::from_bytes(4) }
    }

    pub const fn i64() -> Reg {
        Reg { kind: RegKind::Integer, size: Size::from_bytes(8) }
    }

    pub const fn i128() -> Reg {
        Reg { kind: RegKind::Integer, size: Size::from_bytes(16) }
    }

    pub const fn f16() -> Reg {
        Reg { kind: RegKind::Float, size: Float::F16.size() }
    }

    pub const fn f32() -> Reg {
        Reg { kind: RegKind::Float, size: Float::F32.size() }
    }

    pub const fn f64() -> Reg {
        Reg { kind: RegKind::Float, size: Float::F64.size() }
    }

    pub const fn f128() -> Reg {
        Reg { kind: RegKind::Float, size: Float::F128.size() }
    }

    pub const fn x87_f80() -> Reg {
        Reg { kind: RegKind::Float, size: Float::X87F80.size() }
    }

    pub const fn opaque_vector(size: Size) -> Reg {
        Reg {
            kind: RegKind::Vector { hint_vector_elem: Primitive::Int(Integer::I8, false) },
            size,
        }
    }

    pub fn scratch_align<C: HasDataLayout>(self, cx: &C) -> crate::Align {
        match self.kind {
            RegKind::Integer => {
                Integer::fit_unsigned(self.size.unsigned_int_max()).align(cx.data_layout())
            }
            RegKind::Float => match self.size.bits() {
                16 => Float::F16.align(cx.data_layout()),
                32 => Float::F32.align(cx.data_layout()),
                64 => Float::F64.align(cx.data_layout()),
                80 => Float::X87F80.align(cx.data_layout()),
                _ => Float::F128.align(cx.data_layout()),
            },
            RegKind::Vector { hint_vector_elem } => {
                crate::Align::from_bytes(self.size.bytes().next_power_of_two())
                    .unwrap_or_else(|_| hint_vector_elem.default_align(cx.data_layout()))
            }
        }
    }
}
