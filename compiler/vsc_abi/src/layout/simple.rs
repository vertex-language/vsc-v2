//! Simple, infallible layout constructors (`rustc_abi::layout::simple`).

use vsc_index::{Idx, IndexVec};

use crate::{
    BackendRepr, FieldsShape, HasDataLayout, LayoutData, Niche, Primitive, Scalar, Size, Variants,
};

impl<FieldIdx: Idx, VariantIdx: Idx> LayoutData<FieldIdx, VariantIdx> {
    pub fn unit<C: HasDataLayout>(cx: &C, sized: bool) -> Self {
        let dl = cx.data_layout();
        LayoutData {
            variants: Variants::Single { index: VariantIdx::new(0) },
            fields: FieldsShape::Arbitrary {
                offsets: IndexVec::new(),
                in_memory_order: IndexVec::new(),
            },
            backend_repr: BackendRepr::Memory { sized },
            largest_niche: None,
            uninhabited: false,
            align: dl.i8_align,
            size: Size::ZERO,
            max_repr_align: None,
            unadjusted_abi_align: dl.i8_align,
            repr_c: false,
            randomization_seed: 0,
        }
    }

    pub fn never_type<C: HasDataLayout>(cx: &C) -> Self {
        let dl = cx.data_layout();
        LayoutData {
            variants: Variants::Empty,
            fields: FieldsShape::Primitive,
            backend_repr: BackendRepr::Memory { sized: true },
            largest_niche: None,
            uninhabited: true,
            align: dl.i8_align,
            size: Size::ZERO,
            max_repr_align: None,
            unadjusted_abi_align: dl.i8_align,
            repr_c: false,
            randomization_seed: 0,
        }
    }

    pub fn scalar<C: HasDataLayout>(cx: &C, scalar: Scalar) -> Self {
        let dl = cx.data_layout();
        let largest_niche = Niche::from_scalar(cx, Size::ZERO, scalar);
        let size = scalar.size(dl);
        let align = scalar.default_align(dl);
        let range = scalar.valid_range(dl);

        let seed = size
            .bytes()
            .wrapping_add(
                match scalar.primitive() {
                    Primitive::Int(_, true) => 1u64,
                    Primitive::Int(_, false) => 2,
                    Primitive::Float(_) => 3,
                    Primitive::Pointer(_) => 4,
                } << 32,
            )
            .wrapping_add((range.start as u64).rotate_right(16))
            .wrapping_add((range.end as u64).rotate_right(16));

        LayoutData {
            variants: Variants::Single { index: VariantIdx::new(0) },
            fields: FieldsShape::Primitive,
            backend_repr: BackendRepr::Scalar(scalar),
            largest_niche,
            uninhabited: false,
            size,
            align,
            max_repr_align: None,
            unadjusted_abi_align: align,
            repr_c: false,
            randomization_seed: seed,
        }
    }

    pub fn scalar_pair<C: HasDataLayout>(cx: &C, a: Scalar, b: Scalar) -> Self {
        let dl = cx.data_layout();
        let b_align = b.default_align(dl);
        let align = a.default_align(dl).max(b_align).max(dl.aggregate_align);
        let b_offset = a.size(dl).align_to(b_align);
        let size = (b_offset + b.size(dl)).align_to(align);

        let largest_niche = Niche::from_scalar(cx, b_offset, b)
            .into_iter()
            .chain(Niche::from_scalar(cx, Size::ZERO, a))
            .max_by_key(|n| n.available(dl));

        LayoutData {
            variants: Variants::Single { index: VariantIdx::new(0) },
            fields: FieldsShape::Arbitrary {
                offsets: IndexVec::from_raw(vec![Size::ZERO, b_offset]),
                in_memory_order: IndexVec::from_raw(vec![FieldIdx::new(0), FieldIdx::new(1)]),
            },
            backend_repr: BackendRepr::ScalarPair { a, b, b_offset },
            largest_niche,
            uninhabited: false,
            align,
            size,
            max_repr_align: None,
            unadjusted_abi_align: align,
            repr_c: false,
            randomization_seed: 0,
        }
    }

    pub fn array_like<C: HasDataLayout>(
        _cx: &C,
        element: &LayoutData<FieldIdx, VariantIdx>,
        count: Option<u64>,
    ) -> Self {
        let align = element.align;
        let stride = element.size.align_to(align);
        let (size, sized) = match count {
            Some(n) => (stride * n, true),
            None => (Size::ZERO, false),
        };
        let uninhabited = element.uninhabited && count.is_none_or(|n| n > 0);
        LayoutData {
            variants: Variants::Single { index: VariantIdx::new(0) },
            fields: FieldsShape::Array { stride, count: count.unwrap_or(0) },
            backend_repr: BackendRepr::Memory { sized },
            largest_niche: None,
            uninhabited,
            align,
            size,
            max_repr_align: element.max_repr_align,
            unadjusted_abi_align: align,
            repr_c: element.repr_c,
            randomization_seed: element.randomization_seed,
        }
    }
}
