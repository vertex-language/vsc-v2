//! Field shapes, backend representations, niches, and layout metadata structures.

use std::num::NonZero;
use std::ops::RangeInclusive;

use vsc_index::{Idx, IndexVec};

use crate::{
    Align, Float, HasDataLayout, Primitive, Scalar, Size, TargetDataLayout, WrappingRange,
};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum FieldsShape<FieldIdx: Idx> {
    Primitive,
    Union(NonZero<usize>),
    Array { stride: Size, count: u64 },
    Arbitrary { offsets: IndexVec<FieldIdx, Size>, in_memory_order: IndexVec<FieldIdx, FieldIdx> },
}

impl<FieldIdx: Idx> FieldsShape<FieldIdx> {
    #[inline]
    pub fn count(&self) -> usize {
        match self {
            FieldsShape::Primitive => 0,
            FieldsShape::Union(n) => n.get(),
            FieldsShape::Array { count, .. } => *count as usize,
            FieldsShape::Arbitrary { offsets, .. } => offsets.len(),
        }
    }

    #[inline]
    pub fn offset(&self, i: usize) -> Size {
        match self {
            FieldsShape::Primitive => unreachable!("FieldsShape::Primitive has no fields"),
            FieldsShape::Union(_) => Size::ZERO,
            FieldsShape::Array { stride, count } => {
                assert!(i < *count as usize);
                *stride * (i as u64)
            }
            FieldsShape::Arbitrary { offsets, .. } => offsets[FieldIdx::new(i)],
        }
    }

    pub fn fields_by_offset_order(&self) -> Vec<usize> {
        match self {
            FieldsShape::Primitive => Vec::new(),
            FieldsShape::Union(n) => (0..n.get()).collect(),
            FieldsShape::Array { count, .. } => (0..*count as usize).collect(),
            FieldsShape::Arbitrary { in_memory_order, .. } => {
                in_memory_order.iter().map(|i| i.index()).collect()
            }
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum BackendRepr {
    Scalar(Scalar),
    ScalarPair { a: Scalar, b: Scalar, b_offset: Size },
    SimdVector { element: Scalar, count: u64 },
    Memory { sized: bool },
}

impl BackendRepr {
    #[inline]
    pub fn is_sized(self) -> bool {
        !matches!(self, BackendRepr::Memory { sized: false })
    }

    #[inline]
    pub fn is_scalar(self) -> bool {
        matches!(self, BackendRepr::Scalar(_))
    }

    #[inline]
    pub fn is_scalar_pair(self) -> bool {
        matches!(self, BackendRepr::ScalarPair { .. })
    }

    #[inline]
    pub fn is_bool(self) -> bool {
        matches!(self, BackendRepr::Scalar(s) if s.is_bool())
    }

    #[inline]
    pub fn is_x87_f80(self) -> bool {
        matches!(self, BackendRepr::Scalar(s) if matches!(s.primitive(), Primitive::Float(Float::X87F80)))
    }

    #[inline]
    pub fn to_union(self) -> Self {
        match self {
            BackendRepr::Scalar(s) => BackendRepr::Scalar(s.to_union()),
            BackendRepr::ScalarPair { a, b, b_offset } => {
                BackendRepr::ScalarPair { a: a.to_union(), b: b.to_union(), b_offset }
            }
            BackendRepr::SimdVector { element, count } => {
                BackendRepr::SimdVector { element: element.to_union(), count }
            }
            BackendRepr::Memory { sized } => BackendRepr::Memory { sized },
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum TagEncoding<VariantIdx: Idx> {
    Direct,
    Niche {
        untagged_variant: VariantIdx,
        niche_variants: RangeInclusive<VariantIdx>,
        niche_start: u128,
    },
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Variants<FieldIdx: Idx, VariantIdx: Idx> {
    Empty,
    Single { index: VariantIdx },
    Multiple {
        tag: Scalar,
        tag_encoding: TagEncoding<VariantIdx>,
        tag_field: FieldIdx,
        variants: IndexVec<VariantIdx, LayoutData<FieldIdx, VariantIdx>>,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Niche {
    pub offset: Size,
    pub value: Primitive,
    pub valid_range: WrappingRange,
}

impl Niche {
    pub fn from_scalar<C: HasDataLayout>(cx: &C, offset: Size, scalar: Scalar) -> Option<Self> {
        let dl = cx.data_layout();
        let Scalar::Initialized { value, valid_range } = scalar else {
            return None;
        };
        if valid_range.is_full_for(value.size(dl)) {
            return None;
        }
        Some(Niche { offset, value, valid_range })
    }

    #[inline]
    pub fn available(self, dl: &TargetDataLayout) -> u128 {
        let size = self.value.size(dl);
        let max = size.unsigned_int_max();
        max - self.valid_range.wrapping_size(size)
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct LayoutData<FieldIdx: Idx, VariantIdx: Idx> {
    pub variants: Variants<FieldIdx, VariantIdx>,
    pub fields: FieldsShape<FieldIdx>,
    pub backend_repr: BackendRepr,
    pub largest_niche: Option<Niche>,
    pub uninhabited: bool,
    pub size: Size,
    pub align: Align,
    pub max_repr_align: Option<Align>,
    pub unadjusted_abi_align: Align,
    pub repr_c: bool,
    pub randomization_seed: u64,
}

impl<FieldIdx: Idx, VariantIdx: Idx> LayoutData<FieldIdx, VariantIdx> {
    #[inline]
    pub fn is_sized(&self) -> bool {
        self.backend_repr.is_sized()
    }

    #[inline]
    pub fn is_unsized(&self) -> bool {
        !self.is_sized()
    }

    #[inline]
    pub fn is_scalable_vector(&self) -> bool {
        false
    }

    #[inline]
    pub fn is_zst(&self) -> bool {
        matches!(self.backend_repr, BackendRepr::Memory { sized: true }) && self.size == Size::ZERO
            || (self.uninhabited && self.size == Size::ZERO && self.is_sized())
    }

    #[inline]
    pub fn is_1zst(&self) -> bool {
        self.is_zst() && self.align.bytes() == 1
    }

    #[inline]
    pub fn is_uninhabited(&self) -> bool {
        self.uninhabited
    }

    #[inline]
    pub fn is_aggregate(&self) -> bool {
        match self.backend_repr {
            BackendRepr::Scalar(_) | BackendRepr::SimdVector { .. } => false,
            BackendRepr::ScalarPair { .. } | BackendRepr::Memory { .. } => true,
        }
    }

    #[inline]
    pub fn is_repr_c(&self) -> bool {
        self.repr_c
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum PointerKind {
    Shared,
    Mutable,
    Box,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct PointeeInfo {
    pub size: Size,
    pub align: Align,
    pub safe: Option<PointerKind>,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum StructKind {
    AlwaysSized,
    MaybeUnsized,
    Prefixed(Size, Align),
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AbiFromStrErr {
    Unknown,
}
