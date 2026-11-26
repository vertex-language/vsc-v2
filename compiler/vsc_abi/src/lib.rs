//! Target-independent type layout and ABI primitives (`rustc_abi`).

use std::fmt;

pub mod callconv;
pub mod canon_abi;
pub mod extern_abi;
pub mod layout;
mod layout_data;
mod size_align;
#[cfg(test)]
mod tests;
pub mod wrapping_range;

pub use callconv::{Heterogeneous, HomogeneousAggregate, Reg, RegKind};
pub use canon_abi::{ArmCall, CanonAbi, InterruptKind, X86Call};
pub use extern_abi::{CVariadicStatus, ExternAbi, all_names};
pub use layout::{
    FIRST_VARIANT, FieldIdx, Layout, LayoutCalculator, LayoutCalculatorError,
    LayoutCalculatorResult, NicheOptimizations, TyAbiInterface, TyAndLayout, VariantIdx,
};
pub use layout_data::*;
pub use size_align::*;
pub use wrapping_range::WrappingRange;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct ReprFlags(u8);

impl ReprFlags {
    pub const IS_C: Self = Self(1 << 0);
    pub const IS_SIMD: Self = Self(1 << 1);
    pub const IS_TRANSPARENT: Self = Self(1 << 2);
    pub const IS_LINEAR: Self = Self(1 << 3);

    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[inline]
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum IntegerType {
    Pointer(bool),
    Fixed(Integer, bool),
}

/// Representation options specified via `@repr(...)` attributes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct ReprOptions {
    pub int: Option<IntegerType>,
    pub align: Option<Align>,
    pub pack: Option<Align>,
    pub flags: ReprFlags,
    pub field_shuffle_seed: u64,
}

impl ReprOptions {
    #[inline]
    pub fn c(&self) -> bool {
        self.flags.contains(ReprFlags::IS_C)
    }

    #[inline]
    pub fn simd(&self) -> bool {
        self.flags.contains(ReprFlags::IS_SIMD)
    }

    #[inline]
    pub fn transparent(&self) -> bool {
        self.flags.contains(ReprFlags::IS_TRANSPARENT)
    }

    #[inline]
    pub fn packed(&self) -> bool {
        self.pack.is_some()
    }

    #[inline]
    pub fn inhibit_newtype_abi_optimization(&self) -> bool {
        self.c() || self.align.is_some()
    }

    #[inline]
    pub fn inhibit_struct_field_reordering(&self) -> bool {
        self.c()
            || self.simd()
            || self.packed()
            || self.flags.contains(ReprFlags::IS_LINEAR)
            || self.transparent()
    }

    #[inline]
    pub fn inhibit_enum_layout_opt(&self) -> bool {
        self.c() || self.int.is_some()
    }

    #[inline]
    pub fn inhibits_union_abi_opt(&self) -> bool {
        self.c()
    }

    pub fn discr_type(&self) -> IntegerType {
        self.int.unwrap_or(IntegerType::Pointer(true))
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PointerSpec {
    pub pointer_offset: Size,
    pub pointer_size: Size,
    pub pointer_align: Align,
    pub is_fat: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TargetDataLayout {
    pub endian: Endian,
    pub i1_align: Align,
    pub i8_align: Align,
    pub i16_align: Align,
    pub i32_align: Align,
    pub i64_align: Align,
    pub i128_align: Align,
    pub f16_align: Align,
    pub f32_align: Align,
    pub f64_align: Align,
    pub x87_f80_align: Align,
    pub f128_align: Align,
    pub default_address_space: AddressSpace,
    pub instruction_address_space: AddressSpace,
    pub default_address_space_pointer_spec: PointerSpec,
    pub address_space_info: Vec<(AddressSpace, PointerSpec)>,
    pub aggregate_align: Align,
    pub c_enum_min_size: Integer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetDataLayoutError<'a> {
    InvalidAddressSpace(&'a str),
    InvalidBits(&'a str),
    InvalidAlignment(&'a str),
    MissingAlignment(&'a str),
    InvalidSpec(&'a str),
}

impl fmt::Display for TargetDataLayoutError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid target data layout: {self:?}")
    }
}

impl Default for TargetDataLayout {
    fn default() -> Self {
        Self {
            endian: Endian::Little,
            i1_align: Align::ONE,
            i8_align: Align::ONE,
            i16_align: Align::TWO,
            i32_align: Align::FOUR,
            i64_align: Align::EIGHT,
            i128_align: Align::EIGHT,
            f16_align: Align::TWO,
            f32_align: Align::FOUR,
            f64_align: Align::EIGHT,
            x87_f80_align: Align::SIXTEEN,
            f128_align: Align::SIXTEEN,
            default_address_space: AddressSpace::ZERO,
            instruction_address_space: AddressSpace::ZERO,
            default_address_space_pointer_spec: PointerSpec {
                pointer_offset: Size::from_bytes(8),
                pointer_size: Size::from_bytes(8),
                pointer_align: Align::EIGHT,
                is_fat: false,
            },
            address_space_info: Vec::new(),
            aggregate_align: Align::ONE,
            c_enum_min_size: Integer::I32,
        }
    }
}

impl TargetDataLayout {
    pub fn parse_from_datalayout_string<'a>(
        input: &'a str,
        default_address_space: AddressSpace,
    ) -> Result<Self, TargetDataLayoutError<'a>> {
        let parse_u64 = |s: &'a str| -> Result<u64, TargetDataLayoutError<'a>> {
            s.parse::<u64>().map_err(|_| TargetDataLayoutError::InvalidBits(s))
        };
        let parse_align = |parts: &[&'a str], cause: &'a str| -> Result<Align, TargetDataLayoutError<'a>> {
            let first = parts.first().ok_or(TargetDataLayoutError::MissingAlignment(cause))?;
            let bits = parse_u64(first)?;
            Align::from_bits(bits).map_err(|_| TargetDataLayoutError::InvalidAlignment(cause))
        };

        let mut dl = Self { default_address_space, ..Self::default() };
        let mut i128_seen = false;
        let mut ptr_size_bits = 64u64;

        for spec in input.split('-').filter(|s| !s.is_empty()) {
            let parts: Vec<&str> = spec.split(':').collect();
            match &*parts {
                ["e"] => dl.endian = Endian::Little,
                ["E"] => dl.endian = Endian::Big,
                [p] if p.starts_with('m') || p.starts_with('n') || p.starts_with('S')
                    || p.starts_with("Fn") =>
                {
                    // Mangling (`m:o`, `m:e`, `m:w`), native int widths (`n32:64`), stack align
                    // (`S128`), and function pointer alignment (`Fn32`) are parsed/accepted cleanly.
                }
                [p] if p.starts_with('P') => {
                    let as_num: u32 =
                        p[1..].parse().map_err(|_| TargetDataLayoutError::InvalidAddressSpace(spec))?;
                    dl.instruction_address_space = AddressSpace(as_num);
                }
                ["a", rest @ ..] => dl.aggregate_align = parse_align(rest, spec)?,
                ["f16", rest @ ..] => dl.f16_align = parse_align(rest, spec)?,
                ["f32", rest @ ..] => dl.f32_align = parse_align(rest, spec)?,
                ["f64", rest @ ..] => dl.f64_align = parse_align(rest, spec)?,
                ["f80", rest @ ..] => dl.x87_f80_align = parse_align(rest, spec)?,
                ["f128", rest @ ..] => dl.f128_align = parse_align(rest, spec)?,
                ["i1", rest @ ..] => dl.i1_align = parse_align(rest, spec)?,
                ["i8", rest @ ..] => dl.i8_align = parse_align(rest, spec)?,
                ["i16", rest @ ..] => dl.i16_align = parse_align(rest, spec)?,
                ["i32", rest @ ..] => dl.i32_align = parse_align(rest, spec)?,
                ["i64", rest @ ..] => dl.i64_align = parse_align(rest, spec)?,
                ["i128", rest @ ..] => {
                    dl.i128_align = parse_align(rest, spec)?;
                    i128_seen = true;
                }
                [p, s, a @ ..] if p.starts_with('p') => {
                    let mut p_rest = &p[1..];
                    let is_fat = if let Some(stripped) = p_rest.strip_prefix('f') {
                        p_rest = stripped;
                        true
                    } else {
                        false
                    };
                    let addr_space = if p_rest.is_empty() {
                        AddressSpace::ZERO
                    } else {
                        AddressSpace(
                            p_rest
                                .parse()
                                .map_err(|_| TargetDataLayoutError::InvalidAddressSpace(spec))?,
                        )
                    };
                    let size_bits = parse_u64(s)?;
                    let size = Size::from_bits(size_bits);
                    let align = parse_align(a, spec)?;
                    let ps = PointerSpec {
                        pointer_offset: size,
                        pointer_size: size,
                        pointer_align: align,
                        is_fat,
                    };
                    if addr_space == default_address_space {
                        ptr_size_bits = size_bits;
                        dl.default_address_space_pointer_spec = ps;
                    } else {
                        dl.address_space_info.push((addr_space, ps));
                    }
                }
                _ => {
                    // Accept unknown future datalayout keys (e.g., vectors `v...`, non-integral `ni...`)
                    // unless completely malformed.
                    if parts[0].is_empty() {
                        return Err(TargetDataLayoutError::InvalidSpec(spec));
                    }
                }
            }
        }

        if !i128_seen {
            dl.i128_align = if ptr_size_bits >= 64 { dl.i64_align } else { Align::EIGHT };
        }
        Ok(dl)
    }

    #[inline]
    pub fn pointer_size(&self) -> Size {
        self.default_address_space_pointer_spec.pointer_size
    }

    #[inline]
    pub fn pointer_align(&self) -> Align {
        self.default_address_space_pointer_spec.pointer_align
    }

    #[inline]
    pub fn ptr_sized_integer(&self) -> Integer {
        Integer::from_size(self.pointer_size())
            .expect("pointer size must match a standard integer width")
    }

    #[inline]
    pub fn obj_size_bound(&self) -> u64 {
        match self.pointer_size().bytes() {
            1 => 1 << 7,
            2 => 1 << 15,
            4 => 1 << 31,
            8 => 1 << 47,
            _ => 1 << 47,
        }
    }

    #[inline]
    pub fn llsize_of_alloc(&self, size: Size, align: Align) -> Size {
        size.align_to(align)
    }

    #[inline]
    pub fn data_address_space(&self) -> AddressSpace {
        self.default_address_space
    }

    #[inline]
    pub fn pointer_spec(&self, address_space: AddressSpace) -> &PointerSpec {
        if address_space == self.default_address_space {
            &self.default_address_space_pointer_spec
        } else {
            self.address_space_info
                .iter()
                .find(|(a, _)| *a == address_space)
                .map_or(&self.default_address_space_pointer_spec, |(_, s)| s)
        }
    }
}

pub trait HasDataLayout {
    fn data_layout(&self) -> &TargetDataLayout;
}

impl HasDataLayout for TargetDataLayout {
    #[inline]
    fn data_layout(&self) -> &TargetDataLayout {
        self
    }
}

impl<T: HasDataLayout + ?Sized> HasDataLayout for &T {
    #[inline]
    fn data_layout(&self) -> &TargetDataLayout {
        (*self).data_layout()
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Endian {
    Little,
    Big,
}

impl Endian {
    pub fn as_str(self) -> &'static str {
        match self {
            Endian::Little => "little",
            Endian::Big => "big",
        }
    }
}

