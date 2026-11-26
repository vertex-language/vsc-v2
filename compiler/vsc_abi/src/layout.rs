//! Target-independent type layout calculator (`rustc_abi::layout`).

use std::cmp;
use std::fmt;
use std::num::NonZero;
use std::ops::{Deref, RangeFrom, RangeToInclusive};

use vsc_index::{Idx, IndexSlice, IndexVec};

use crate::{
    BackendRepr, FieldsShape, HasDataLayout, Integer, LayoutData, ReprOptions, Size, StructKind,
    Variants, WrappingRange,
};

mod enum_layout;
mod simple;
mod ty;

pub use ty::{Layout, TyAbiInterface, TyAndLayout};

vsc_index::newtype_index! {
    /// Index of a field inside a struct, union, or enum variant.
    pub struct FieldIdx {
        DEBUG_FORMAT = "field[{}]"
    }
}

vsc_index::newtype_index! {
    /// Index of a variant inside an enum or struct.
    pub struct VariantIdx {
        DEBUG_FORMAT = "variant[{}]"
    }
}

pub const FIRST_VARIANT: VariantIdx = VariantIdx::ZERO;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NicheOptimizations {
    Enabled,
    Disabled,
}

/// Returns `true` if a variant is uninhabited and has only zero-sized fields.
fn absent<FieldIdx, VariantIdx, F>(fields: &IndexSlice<FieldIdx, F>) -> bool
where
    FieldIdx: Idx,
    VariantIdx: Idx,
    F: Deref<Target = LayoutData<FieldIdx, VariantIdx>> + fmt::Debug,
{
    let uninhabited = fields.iter().any(|f| f.is_uninhabited());
    let is_1zst = fields.iter().all(|f| f.is_1zst());
    uninhabited && is_1zst
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LayoutCalculatorError<F> {
    UnexpectedUnsized(F),
    SizeOverflow,
    EmptyUnion,
    ReprConflict,
    ZeroLengthSimdType,
    OversizedSimdType { max_lanes: usize },
    NonPrimitiveSimdType(F),
}

impl<F> LayoutCalculatorError<F> {
    pub fn without_payload(&self) -> LayoutCalculatorError<()> {
        use LayoutCalculatorError::*;
        match *self {
            UnexpectedUnsized(_) => UnexpectedUnsized(()),
            SizeOverflow => SizeOverflow,
            EmptyUnion => EmptyUnion,
            ReprConflict => ReprConflict,
            ZeroLengthSimdType => ZeroLengthSimdType,
            OversizedSimdType { max_lanes } => OversizedSimdType { max_lanes },
            NonPrimitiveSimdType(_) => NonPrimitiveSimdType(()),
        }
    }

    pub fn fallback_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use LayoutCalculatorError::*;
        f.write_str(match self {
            UnexpectedUnsized(_) => "an unsized type was found where a sized type was expected",
            SizeOverflow => "size overflow",
            EmptyUnion => "type is a union with no fields",
            ReprConflict => "type has an invalid repr",
            ZeroLengthSimdType | OversizedSimdType { .. } | NonPrimitiveSimdType(_) => {
                "invalid simd type definition"
            }
        })
    }
}

pub type LayoutCalculatorResult<FieldIdx, VariantIdx, F> =
    Result<LayoutData<FieldIdx, VariantIdx>, LayoutCalculatorError<F>>;

#[derive(Clone, Copy, Debug)]
pub struct LayoutCalculator<Cx> {
    pub cx: Cx,
}

impl<Cx: HasDataLayout> LayoutCalculator<Cx> {
    pub fn new(cx: Cx) -> Self {
        Self { cx }
    }

    pub fn layout_of_struct_or_enum<FieldIdx, VariantIdx, F>(
        &self,
        repr: &ReprOptions,
        variants: &IndexSlice<VariantIdx, IndexVec<FieldIdx, F>>,
        is_enum: bool,
        is_special_no_niche: bool,
        discr_range_of_repr: impl Fn(RangeFrom<i128>, RangeToInclusive<u128>) -> (Integer, bool),
        discriminants: impl Iterator<Item = (VariantIdx, u128)>,
        always_sized: bool,
    ) -> LayoutCalculatorResult<FieldIdx, VariantIdx, F>
    where
        FieldIdx: Idx,
        VariantIdx: Idx,
        F: Deref<Target = LayoutData<FieldIdx, VariantIdx>> + fmt::Debug + Copy,
    {
        if is_enum {
            return self::enum_layout::layout_of_enum(
                self,
                repr,
                variants,
                discr_range_of_repr,
                discriminants,
            );
        }
        assert_eq!(variants.len(), 1);
        let niche_opt = if is_special_no_niche {
            NicheOptimizations::Disabled
        } else {
            NicheOptimizations::Enabled
        };
        self.layout_of_struct(repr, variants, VariantIdx::new(0), false, niche_opt, always_sized)
    }

    pub fn layout_of_union<FieldIdx, VariantIdx, F>(
        &self,
        repr: &ReprOptions,
        variants: &IndexSlice<VariantIdx, IndexVec<FieldIdx, F>>,
    ) -> LayoutCalculatorResult<FieldIdx, VariantIdx, F>
    where
        FieldIdx: Idx,
        VariantIdx: Idx,
        F: Deref<Target = LayoutData<FieldIdx, VariantIdx>> + fmt::Debug + Copy,
    {
        let dl = self.cx.data_layout();
        let mut align = if repr.pack.is_some() { dl.i8_align } else { dl.aggregate_align };
        let mut max_repr_align = repr.align;

        struct AbiMismatch;
        let mut common_non_zst_repr = if repr.inhibits_union_abi_opt() {
            Err(AbiMismatch)
        } else {
            Ok(None)
        };

        let mut size = Size::ZERO;
        let only_variant_idx = VariantIdx::new(0);
        let only_variant = &variants[only_variant_idx];
        for field in only_variant {
            if field.is_unsized() {
                return Err(LayoutCalculatorError::UnexpectedUnsized(*field));
            }
            align = align.max(field.align);
            max_repr_align = max_repr_align.max(field.max_repr_align);
            size = cmp::max(size, field.size);

            if field.is_zst() {
                continue;
            }
            if let Ok(common) = common_non_zst_repr {
                let field_abi = field.backend_repr.to_union();
                match common {
                    Some((prev_abi, _)) if prev_abi != field_abi => {
                        common_non_zst_repr = Err(AbiMismatch);
                    }
                    Some(_) => {}
                    None => {
                        common_non_zst_repr = Ok(Some((field_abi, field.align)));
                    }
                }
            }
        }

        if let Some(pack) = repr.pack {
            align = align.min(pack);
        }
        let unadjusted_abi_align = align;
        if let Some(repr_align) = repr.align {
            align = align.max(repr_align);
        }

        let backend_repr = match common_non_zst_repr {
            Err(AbiMismatch) | Ok(None) => BackendRepr::Memory { sized: true },
            Ok(Some((BackendRepr::Scalar(s), _))) if s.size(dl) > Size::ZERO => {
                if s.default_align(dl) > align {
                    BackendRepr::Memory { sized: true }
                } else {
                    BackendRepr::Scalar(s)
                }
            }
            Ok(Some((r, _))) => r,
        };

        let Some(union_field_count) = NonZero::new(only_variant.len()) else {
            return Err(LayoutCalculatorError::EmptyUnion);
        };
        let repr_c =
            repr.c() || (repr.transparent() && only_variant.iter().any(|field| field.repr_c));

        Ok(LayoutData {
            variants: Variants::Single { index: only_variant_idx },
            fields: FieldsShape::Union(union_field_count),
            backend_repr,
            largest_niche: None,
            uninhabited: false,
            align,
            size: size.align_to(align),
            max_repr_align,
            unadjusted_abi_align,
            repr_c,
            randomization_seed: 0,
        })
    }

    pub fn layout_of_struct<FieldIdx, VariantIdx, F>(
        &self,
        repr: &ReprOptions,
        variants: &IndexSlice<VariantIdx, IndexVec<FieldIdx, F>>,
        variant_idx: VariantIdx,
        is_enum: bool,
        niche_optimizations: NicheOptimizations,
        always_sized: bool,
    ) -> LayoutCalculatorResult<FieldIdx, VariantIdx, F>
    where
        FieldIdx: Idx,
        VariantIdx: Idx,
        F: Deref<Target = LayoutData<FieldIdx, VariantIdx>> + fmt::Debug + Copy,
    {
        let dl = self.cx.data_layout();
        let v = variant_idx;
        let kind = if is_enum || variants[v].is_empty() || always_sized {
            StructKind::AlwaysSized
        } else {
            StructKind::MaybeUnsized
        };

        let mut st = self.layout_of_univariant(&variants[v], repr, kind)?;
        st.variants = Variants::Single { index: v };

        if niche_optimizations == NicheOptimizations::Disabled {
            let hide_niches = |scalar: &mut crate::Scalar| {
                if let crate::Scalar::Initialized { value, valid_range } = scalar {
                    *valid_range = WrappingRange::full(value.size(dl));
                }
            };
            match &mut st.backend_repr {
                BackendRepr::Scalar(scalar) => hide_niches(scalar),
                BackendRepr::ScalarPair { a, b, .. } => {
                    hide_niches(a);
                    hide_niches(b);
                }
                BackendRepr::SimdVector { element, .. } => hide_niches(element),
                BackendRepr::Memory { .. } => {}
            }
            st.largest_niche = None;
        }

        Ok(st)
    }

    pub fn layout_of_univariant<FieldIdx, VariantIdx, F>(
        &self,
        fields: &IndexSlice<FieldIdx, F>,
        repr: &ReprOptions,
        kind: StructKind,
    ) -> LayoutCalculatorResult<FieldIdx, VariantIdx, F>
    where
        FieldIdx: Idx,
        VariantIdx: Idx,
        F: Deref<Target = LayoutData<FieldIdx, VariantIdx>> + fmt::Debug + Copy,
    {
        if repr.transparent() && (repr.c() || repr.align.is_some() || repr.pack.is_some()) {
            return Err(LayoutCalculatorError::ReprConflict);
        }

        let dl = self.cx.data_layout();
        let pack = repr.pack;
        let mut align = if pack.is_some() { dl.i8_align } else { dl.aggregate_align };
        let mut max_repr_align = repr.align;

        let mut in_memory_order: IndexVec<usize, FieldIdx> =
            IndexVec::from_raw((0..fields.len()).map(FieldIdx::new).collect());
        let optimize_field_order = !repr.inhibit_struct_field_reordering();
        let end =
            if let StructKind::MaybeUnsized = kind { fields.len() - 1 } else { fields.len() };
        let optimizing = &mut in_memory_order.raw[..end];

        if optimize_field_order && fields.len() > 1 {
            let alignment_key = |layout: &F| -> u64 {
                if let Some(pack) = pack {
                    layout.align.min(pack).bytes()
                } else {
                    let a = layout.align.bytes();
                    let s = layout.size.bytes();
                    a.max(s).trailing_zeros() as u64
                }
            };
            match kind {
                StructKind::AlwaysSized | StructKind::MaybeUnsized => {
                    optimizing.sort_by_key(|&x| {
                        let f = &fields[x];
                        let niche_size = f.largest_niche.map_or(0, |n| n.available(dl));
                        (cmp::Reverse(alignment_key(f)), cmp::Reverse(niche_size))
                    });
                }
                StructKind::Prefixed(..) => {
                    optimizing.sort_by_key(|&x| {
                        let f = &fields[x];
                        let niche_size = f.largest_niche.map_or(0, |n| n.available(dl));
                        (alignment_key(f), niche_size)
                    });
                }
            }
        }

        let mut unsized_field = None::<&F>;
        let mut offsets = IndexVec::from_elem_n(Size::ZERO, fields.len());
        let mut offset = Size::ZERO;
        let mut largest_niche = None::<crate::Niche>;
        let mut largest_niche_available = 0u128;

        if let StructKind::Prefixed(prefix_size, prefix_align) = kind {
            let prefix_align = if let Some(pack) = pack { prefix_align.min(pack) } else { prefix_align };
            align = align.max(prefix_align);
            offset = prefix_size.align_to(prefix_align);
        }

        for &i in in_memory_order.iter() {
            let field = &fields[i];
            if let Some(prev_unsized) = unsized_field {
                return Err(LayoutCalculatorError::UnexpectedUnsized(*prev_unsized));
            }
            if field.is_unsized() {
                if let StructKind::MaybeUnsized = kind {
                    unsized_field = Some(field);
                } else {
                    return Err(LayoutCalculatorError::UnexpectedUnsized(*field));
                }
            }
            let field_align = if let Some(pack) = pack { field.align.min(pack) } else { field.align };
            offset = offset.align_to(field_align);
            align = align.max(field_align);
            max_repr_align = max_repr_align.max(field.max_repr_align);
            offsets[i] = offset;

            if let Some(mut niche) = field.largest_niche {
                let available = niche.available(dl);
                if available > largest_niche_available {
                    largest_niche_available = available;
                    niche.offset += offset;
                    largest_niche = Some(niche);
                }
            }

            offset = offset.checked_add(field.size, dl).ok_or(LayoutCalculatorError::SizeOverflow)?;
        }

        let unadjusted_abi_align = align;
        if let Some(repr_align) = repr.align {
            align = align.max(repr_align);
        }

        let size = offset.align_to(align);
        if size.bytes() >= dl.obj_size_bound() {
            return Err(LayoutCalculatorError::SizeOverflow);
        }

        let sized = unsized_field.is_none();
        let mut abi = BackendRepr::Memory { sized };
        let optimize_abi = !repr.inhibit_newtype_abi_optimization();
        let mut non_zst_layout = None;

        if sized && size.bytes() > 0 {
            let mut non_zst = fields.iter_enumerated().filter(|(_, f)| !f.is_zst());
            match (non_zst.next(), non_zst.next(), non_zst.next()) {
                (Some((i, field)), None, None) => {
                    non_zst_layout = Some(field);
                    if offsets[i].bytes() == 0 && align == field.align && size == field.size {
                        match field.backend_repr {
                            BackendRepr::Scalar(_) | BackendRepr::SimdVector { .. }
                                if optimize_abi =>
                            {
                                abi = field.backend_repr;
                            }
                            BackendRepr::ScalarPair { .. } => {
                                abi = field.backend_repr;
                            }
                            _ => {}
                        }
                    }
                }
                (Some((i, a)), Some((j, b)), None) => {
                    if let (BackendRepr::Scalar(sa), BackendRepr::Scalar(sb)) =
                        (a.backend_repr, b.backend_repr)
                    {
                        let ((i, a_s), (j, b_s)) =
                            if offsets[i] < offsets[j] { ((i, sa), (j, sb)) } else { ((j, sb), (i, sa)) };
                        let pair = LayoutData::<FieldIdx, VariantIdx>::scalar_pair(&self.cx, a_s, b_s);
                        if offsets[i] == pair.fields.offset(0)
                            && offsets[j] == pair.fields.offset(1)
                            && align == pair.align
                            && size == pair.size
                        {
                            abi = pair.backend_repr;
                        }
                    }
                }
                _ => {}
            }
        }

        // Packed fields must not form invalid unaligned niches.
        if pack.is_some() {
            largest_niche = None;
            abi = BackendRepr::Memory { sized };
        }

        let repr_c = repr.c() || (repr.transparent() && non_zst_layout.is_some_and(|f| f.repr_c));
        let uninhabited = fields.iter().any(|f| f.uninhabited);

        Ok(LayoutData {
            variants: Variants::Single { index: VariantIdx::new(0) },
            fields: FieldsShape::Arbitrary {
                offsets,
                in_memory_order: IndexVec::from_raw(in_memory_order.raw),
            },
            backend_repr: abi,
            largest_niche,
            uninhabited,
            align,
            size,
            max_repr_align,
            unadjusted_abi_align,
            repr_c,
            randomization_seed: 0,
        })
    }
}
