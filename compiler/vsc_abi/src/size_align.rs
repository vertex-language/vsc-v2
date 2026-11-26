//! Size, alignment, integer/float/primitive descriptors, and scalar representations.

use std::fmt;
use std::ops::{Add, AddAssign, Mul, Sub};

use crate::{TargetDataLayout, WrappingRange};

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Size {
    raw: u64,
}

impl fmt::Debug for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Size({} bytes)", self.bytes())
    }
}

impl Size {
    pub const ZERO: Size = Size { raw: 0 };

    #[inline]
    pub const fn from_bits(bits: u64) -> Self {
        Self { raw: bits.div_ceil(8) }
    }

    #[inline]
    pub const fn from_bytes(bytes: u64) -> Self {
        Self { raw: bytes }
    }

    #[inline]
    pub const fn bytes(self) -> u64 {
        self.raw
    }

    #[inline]
    pub const fn bytes_usize(self) -> usize {
        self.raw as usize
    }

    #[inline]
    pub const fn bits(self) -> u64 {
        self.raw.checked_mul(8).expect("Size::bits overflow")
    }

    #[inline]
    pub fn checked_bits(self) -> Option<u64> {
        self.raw.checked_mul(8)
    }

    #[inline]
    pub fn align_to(self, align: Align) -> Self {
        let mask = align.bytes() - 1;
        Size::from_bytes((self.bytes() + mask) & !mask)
    }

    #[inline]
    pub fn is_aligned(self, align: Align) -> bool {
        (self.bytes() & (align.bytes() - 1)) == 0
    }

    #[inline]
    pub fn checked_add(self, other: Self, dl: &TargetDataLayout) -> Option<Self> {
        let sum = self.raw.checked_add(other.raw)?;
        if sum >= dl.obj_size_bound() { None } else { Some(Size::from_bytes(sum)) }
    }

    #[inline]
    pub fn checked_mul(self, count: u64, dl: &TargetDataLayout) -> Option<Self> {
        let prod = self.raw.checked_mul(count)?;
        if prod >= dl.obj_size_bound() { None } else { Some(Size::from_bytes(prod)) }
    }

    #[inline]
    pub const fn unsigned_int_max(self) -> u128 {
        match self.raw {
            0 => 0,
            1 => u8::MAX as u128,
            2 => u16::MAX as u128,
            4 => u32::MAX as u128,
            8 => u64::MAX as u128,
            16 => u128::MAX,
            n if n < 16 => (1u128 << (n * 8)) - 1,
            _ => u128::MAX,
        }
    }

    #[inline]
    pub const fn signed_int_min(self) -> i128 {
        match self.raw {
            1 => i8::MIN as i128,
            2 => i16::MIN as i128,
            4 => i32::MIN as i128,
            8 => i64::MIN as i128,
            16 => i128::MIN,
            _ => 0,
        }
    }

    #[inline]
    pub const fn signed_int_max(self) -> i128 {
        match self.raw {
            1 => i8::MAX as i128,
            2 => i16::MAX as i128,
            4 => i32::MAX as i128,
            8 => i64::MAX as i128,
            16 => i128::MAX,
            _ => 0,
        }
    }

    #[inline]
    pub fn truncate(self, value: u128) -> u128 {
        value & self.unsigned_int_max()
    }
}

impl Add for Size {
    type Output = Size;
    #[inline]
    fn add(self, other: Size) -> Size {
        Size::from_bytes(self.bytes() + other.bytes())
    }
}

impl AddAssign for Size {
    #[inline]
    fn add_assign(&mut self, other: Size) {
        self.raw += other.raw;
    }
}

impl Sub for Size {
    type Output = Size;
    #[inline]
    fn sub(self, other: Size) -> Size {
        Size::from_bytes(self.bytes() - other.bytes())
    }
}

impl Mul<u64> for Size {
    type Output = Size;
    #[inline]
    fn mul(self, count: u64) -> Size {
        Size::from_bytes(self.bytes() * count)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Align {
    pow2: u8,
}

impl fmt::Debug for Align {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Align({} bytes)", self.bytes())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AlignFromBytesError {
    NotPowerOfTwo(u64),
    TooLarge(u64),
}

impl Align {
    pub const ONE: Align = Align { pow2: 0 };
    pub const TWO: Align = Align { pow2: 1 };
    pub const FOUR: Align = Align { pow2: 2 };
    pub const EIGHT: Align = Align { pow2: 3 };
    pub const SIXTEEN: Align = Align { pow2: 4 };

    #[inline]
    pub const fn from_bytes(bytes: u64) -> Result<Self, AlignFromBytesError> {
        if bytes == 0 || !bytes.is_power_of_two() {
            return Err(AlignFromBytesError::NotPowerOfTwo(bytes));
        }
        let pow2 = bytes.trailing_zeros();
        if pow2 > 29 {
            return Err(AlignFromBytesError::TooLarge(bytes));
        }
        Ok(Align { pow2: pow2 as u8 })
    }

    #[inline]
    pub const fn from_bits(bits: u64) -> Result<Self, AlignFromBytesError> {
        let bytes = if bits == 0 { 1 } else { bits.div_ceil(8) };
        Self::from_bytes(bytes)
    }

    #[inline]
    pub const fn bytes(self) -> u64 {
        1u64 << self.pow2
    }

    #[inline]
    pub const fn bits(self) -> u64 {
        self.bytes() * 8
    }

    #[inline]
    pub const fn pow2(self) -> u8 {
        self.pow2
    }

    #[inline]
    pub fn max_for_offset(offset: Size) -> Self {
        let bytes = offset.bytes();
        let pow2 = if bytes == 0 { 29 } else { bytes.trailing_zeros().min(29) };
        Align { pow2: pow2 as u8 }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Integer {
    I8,
    I16,
    I32,
    I64,
    I128,
}

impl Integer {
    #[inline]
    pub const fn size(self) -> Size {
        match self {
            Integer::I8 => Size::from_bytes(1),
            Integer::I16 => Size::from_bytes(2),
            Integer::I32 => Size::from_bytes(4),
            Integer::I64 => Size::from_bytes(8),
            Integer::I128 => Size::from_bytes(16),
        }
    }

    #[inline]
    pub fn align(self, dl: &TargetDataLayout) -> Align {
        match self {
            Integer::I8 => dl.i8_align,
            Integer::I16 => dl.i16_align,
            Integer::I32 => dl.i32_align,
            Integer::I64 => dl.i64_align,
            Integer::I128 => dl.i128_align,
        }
    }

    #[inline]
    pub fn from_size(size: Size) -> Option<Self> {
        match size.bytes() {
            1 => Some(Integer::I8),
            2 => Some(Integer::I16),
            4 => Some(Integer::I32),
            8 => Some(Integer::I64),
            16 => Some(Integer::I128),
            _ => None,
        }
    }

    #[inline]
    pub fn fit_unsigned(n: u128) -> Self {
        match n {
            0..=0xff => Integer::I8,
            0x100..=0xffff => Integer::I16,
            0x1_0000..=0xffff_ffff => Integer::I32,
            0x1_0000_0000..=0xffff_ffff_ffff_ffff => Integer::I64,
            _ => Integer::I128,
        }
    }

    #[inline]
    pub fn fit_signed(n: i128) -> Self {
        if let Ok(_) = i8::try_from(n) {
            Integer::I8
        } else if let Ok(_) = i16::try_from(n) {
            Integer::I16
        } else if let Ok(_) = i32::try_from(n) {
            Integer::I32
        } else if let Ok(_) = i64::try_from(n) {
            Integer::I64
        } else {
            Integer::I128
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Float {
    F16,
    F32,
    F64,
    X87F80,
    F128,
}

impl Float {
    #[inline]
    pub const fn size(self) -> Size {
        match self {
            Float::F16 => Size::from_bytes(2),
            Float::F32 => Size::from_bytes(4),
            Float::F64 => Size::from_bytes(8),
            Float::X87F80 => Size::from_bytes(16),
            Float::F128 => Size::from_bytes(16),
        }
    }

    #[inline]
    pub fn align(self, dl: &TargetDataLayout) -> Align {
        match self {
            Float::F16 => dl.f16_align,
            Float::F32 => dl.f32_align,
            Float::F64 => dl.f64_align,
            Float::X87F80 => dl.x87_f80_align,
            Float::F128 => dl.f128_align,
        }
    }

    #[inline]
    pub const fn bit_width(self) -> u16 {
        match self {
            Float::F16 => 16,
            Float::F32 => 32,
            Float::F64 => 64,
            Float::X87F80 => 80,
            Float::F128 => 128,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AddressSpace(pub u32);

impl AddressSpace {
    pub const ZERO: Self = Self(0);
    pub const DATA: Self = Self(0);
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Numeric {
    Int(Integer, bool),
    Float(Float),
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Primitive {
    Int(Integer, bool),
    Float(Float),
    Pointer(AddressSpace),
}

impl Primitive {
    #[inline]
    pub fn size(self, dl: &TargetDataLayout) -> Size {
        match self {
            Primitive::Int(i, _) => i.size(),
            Primitive::Float(f) => f.size(),
            Primitive::Pointer(a) => dl.pointer_spec(a).pointer_size,
        }
    }

    #[inline]
    pub fn default_align(self, dl: &TargetDataLayout) -> Align {
        match self {
            Primitive::Int(i, _) => i.align(dl),
            Primitive::Float(f) => f.align(dl),
            Primitive::Pointer(a) => dl.pointer_spec(a).pointer_align,
        }
    }

    #[inline]
    pub fn is_float(self) -> bool {
        matches!(self, Primitive::Float(_))
    }

    #[inline]
    pub fn is_int(self) -> bool {
        matches!(self, Primitive::Int(..))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Scalar {
    Initialized { value: Primitive, valid_range: WrappingRange },
    Union { value: Primitive },
}

impl Scalar {
    #[inline]
    pub fn primitive(self) -> Primitive {
        match self {
            Scalar::Initialized { value, .. } | Scalar::Union { value } => value,
        }
    }

    #[inline]
    pub fn size(self, dl: &TargetDataLayout) -> Size {
        self.primitive().size(dl)
    }

    #[inline]
    pub fn default_align(self, dl: &TargetDataLayout) -> Align {
        self.primitive().default_align(dl)
    }

    #[inline]
    pub fn valid_range(self, dl: &TargetDataLayout) -> WrappingRange {
        match self {
            Scalar::Initialized { valid_range, .. } => valid_range,
            Scalar::Union { value } => WrappingRange::full(value.size(dl)),
        }
    }

    #[inline]
    pub fn is_bool(self) -> bool {
        matches!(
            self,
            Scalar::Initialized {
                value: Primitive::Int(Integer::I8, false),
                valid_range: WrappingRange { start: 0, end: 1 },
            }
        )
    }

    #[inline]
    pub fn is_uninit_valid(self) -> bool {
        matches!(self, Scalar::Union { .. })
    }

    #[inline]
    pub fn to_union(self) -> Self {
        Scalar::Union { value: self.primitive() }
    }
}

