//! Enum layout calculation including niche optimization and tagged unions (`rustc_abi::layout::enum_layout`).

use std::cmp;
use std::ops::{Deref, RangeFrom, RangeInclusive, RangeToInclusive};

use vsc_index::{Idx, IndexSlice, IndexVec};

use super::{LayoutCalculator, LayoutCalculatorError, LayoutCalculatorResult, absent};
use crate::{
    Align, BackendRepr, FieldsShape, HasDataLayout, Integer, LayoutData, Niche, Primitive,
    ReprOptions, Scalar, Size, StructKind, TagEncoding, Variants, WrappingRange,
};

pub(super) fn layout_of_enum<Cx: HasDataLayout, FieldIdx, VariantIdx, F>(
    calculator: &LayoutCalculator<Cx>,
    repr: &ReprOptions,
    variants: &IndexSlice<VariantIdx, IndexVec<FieldIdx, F>>,
    discr_range_of_repr: impl Fn(RangeFrom<i128>, RangeToInclusive<u128>) -> (Integer, bool),
    discriminants: impl Iterator<Item = (VariantIdx, u128)>,
) -> LayoutCalculatorResult<FieldIdx, VariantIdx, F>
where
    FieldIdx: Idx,
    VariantIdx: Idx,
    F: Deref<Target = LayoutData<FieldIdx, VariantIdx>> + std::fmt::Debug + Copy,
{
    let dl = calculator.cx.data_layout();
    if repr.packed() {
        return Err(LayoutCalculatorError::ReprConflict);
    }

    let discrs: Vec<(VariantIdx, u128)> = discriminants.collect();

    let niche_filling_layout = calculate_niche_filling_layout(calculator, repr, variants);
    let tagged_layout = calculate_tagged_layout(
        calculator,
        repr,
        variants,
        discr_range_of_repr,
        discrs.iter().copied(),
    )?;

    let Some(nl) = niche_filling_layout else {
        return Ok(tagged_layout);
    };

    use cmp::Ordering::*;
    let niche_size =
        |l: &LayoutData<FieldIdx, VariantIdx>| l.largest_niche.map_or(0, |n| n.available(dl));
    let best_layout = match (
        tagged_layout.size.cmp(&nl.size),
        niche_size(&tagged_layout).cmp(&niche_size(&nl)),
    ) {
        (Greater, _) => nl,
        (Equal, Less) => nl,
        _ => tagged_layout,
    };

    Ok(best_layout)
}

/// Try to store the enum discriminant inside a niche of one non-absent data-carrying variant
/// while all remaining present variants are 1-ZSTs (e.g., `Option<&T>`, `Option<bool>`, `Result<(), &T>`).
fn calculate_niche_filling_layout<Cx: HasDataLayout, FieldIdx, VariantIdx, F>(
    calculator: &LayoutCalculator<Cx>,
    repr: &ReprOptions,
    variants: &IndexSlice<VariantIdx, IndexVec<FieldIdx, F>>,
) -> Option<LayoutData<FieldIdx, VariantIdx>>
where
    FieldIdx: Idx,
    VariantIdx: Idx,
    F: Deref<Target = LayoutData<FieldIdx, VariantIdx>> + std::fmt::Debug + Copy,
{
    if repr.inhibit_enum_layout_opt() {
        return None;
    }
    let dl = calculator.cx.data_layout();

    let mut dataful_variant = None;
    let mut non_absent_count = 0usize;
    let mut zst_variant_align = dl.i8_align;

    for (v_idx, fields) in variants.iter_enumerated() {
        if absent(fields) {
            continue;
        }
        non_absent_count += 1;
        let has_nonzero_field = fields.iter().any(|f| !f.is_zst());
        if has_nonzero_field {
            if dataful_variant.is_some() {
                return None;
            }
            dataful_variant = Some(v_idx);
        } else {
            for f in fields {
                zst_variant_align = zst_variant_align.max(f.align);
            }
        }
    }

    if non_absent_count < 2 {
        return None;
    }
    let dataful_idx = dataful_variant?;
    // Ensure ZST variants sit contiguously outside `dataful_idx` if `variants.len() >= 2`.
    // Restrict contiguous range optimization to the standard cases where all other variants
    // precede or follow `dataful_idx`, matching `TagEncoding::Niche { niche_variants }`.
    let n_variants = variants.len();
    let tagless_count = (n_variants - 1) as u128;
    let (niche_start_v, niche_end_v) = if dataful_idx.index() == 0 {
        (VariantIdx::new(1), VariantIdx::new(n_variants - 1))
    } else if dataful_idx.index() == n_variants - 1 {
        (VariantIdx::new(0), VariantIdx::new(n_variants - 2))
    } else {
        return None;
    };
    if !((niche_start_v.index()..=niche_end_v.index())
        .all(|vi| !absent(&variants[VariantIdx::new(vi)])))
    {
        return None;
    }

    // Must not lower alignment of ZST variants.
    let mut dataful_layout = calculator
        .layout_of_univariant(&variants[dataful_idx], repr, StructKind::AlwaysSized)
        .ok()?;
    if dataful_layout.align < zst_variant_align {
        return None;
    }
    let niche = dataful_layout.largest_niche?;
    if niche.available(dl) < tagless_count {
        return None;
    }

    let niche_prim_size = niche.value.size(dl);
    let mask = niche_prim_size.unsigned_int_max();
    let niche_start = niche.valid_range.end.wrapping_add(1) & mask;
    let tag = Scalar::Initialized {
        value: niche.value,
        valid_range: WrappingRange {
            start: niche.valid_range.start,
            end: niche.valid_range.end.wrapping_add(tagless_count) & mask,
        },
    };
    let largest_niche = Niche::from_scalar(&calculator.cx, niche.offset, tag);
    dataful_layout.largest_niche = None;

    let mut variant_layouts = IndexVec::with_capacity(variants.len());
    for (v_idx, fields) in variants.iter_enumerated() {
        let mut vl = if v_idx == dataful_idx {
            dataful_layout.clone()
        } else {
            let mut vl = calculator
                .layout_of_univariant(fields, repr, StructKind::AlwaysSized)
                .ok()?;
            vl.size = dataful_layout.size;
            vl.align = dataful_layout.align;
            vl
        };
        vl.variants = Variants::Single { index: v_idx };
        variant_layouts.push(vl);
    }

    let backend_repr = if dataful_layout.fields == FieldsShape::Primitive
        && matches!(dataful_layout.backend_repr, BackendRepr::Scalar(_))
        && niche.offset == Size::ZERO
    {
        BackendRepr::Scalar(tag)
    } else {
        dataful_layout.backend_repr
    };

    Some(LayoutData {
        variants: Variants::Multiple {
            tag,
            tag_encoding: TagEncoding::Niche {
                untagged_variant: dataful_idx,
                niche_variants: RangeInclusive::from(niche_start_v..=niche_end_v),
                niche_start,
            },
            tag_field: FieldIdx::new(0),
            variants: variant_layouts,
        },
        fields: FieldsShape::Arbitrary {
            offsets: IndexVec::from_raw(vec![niche.offset]),
            in_memory_order: IndexVec::from_raw(vec![FieldIdx::new(0)]),
        },
        backend_repr,
        largest_niche,
        uninhabited: false,
        align: dataful_layout.align,
        size: dataful_layout.size,
        max_repr_align: dataful_layout.max_repr_align,
        unadjusted_abi_align: dataful_layout.unadjusted_abi_align,
        repr_c: false,
        randomization_seed: 0,
    })
}

fn calculate_tagged_layout<Cx: HasDataLayout, FieldIdx, VariantIdx, F>(
    calculator: &LayoutCalculator<Cx>,
    repr: &ReprOptions,
    variants: &IndexSlice<VariantIdx, IndexVec<FieldIdx, F>>,
    discr_range_of_repr: impl Fn(RangeFrom<i128>, RangeToInclusive<u128>) -> (Integer, bool),
    discriminants: impl Iterator<Item = (VariantIdx, u128)>,
) -> LayoutCalculatorResult<FieldIdx, VariantIdx, F>
where
    FieldIdx: Idx,
    VariantIdx: Idx,
    F: Deref<Target = LayoutData<FieldIdx, VariantIdx>> + std::fmt::Debug + Copy,
{
    let dl = calculator.cx.data_layout();
    let discrs: Vec<(VariantIdx, u128)> = discriminants.collect();

    let mut present_variants = 0usize;
    let mut only_present_idx = None;
    for (v_idx, fields) in variants.iter_enumerated() {
        if !absent(fields) {
            present_variants += 1;
            if only_present_idx.is_none() {
                only_present_idx = Some(v_idx);
            }
        }
    }

    if present_variants == 0 {
        return Ok(LayoutData::never_type(&calculator.cx));
    }

    if present_variants == 1 && !repr.c() && repr.int.is_none() {
        let v_idx = only_present_idx.unwrap();
        let mut st = calculator.layout_of_univariant(&variants[v_idx], repr, StructKind::AlwaysSized)?;
        st.variants = Variants::Single { index: v_idx };
        return Ok(st);
    }

    let mut min_signed = i128::MAX;
    let mut max_unsigned = 0u128;
    for &(v_idx, d) in &discrs {
        if !repr.c() && absent(&variants[v_idx]) {
            continue;
        }
        let s = d as i128;
        if s < min_signed {
            min_signed = s;
        }
        if d > max_unsigned {
            max_unsigned = d;
        }
    }
    if min_signed == i128::MAX {
        min_signed = 0;
    }

    let (discr_int, signed) = discr_range_of_repr(min_signed.., ..=max_unsigned);
    let mut min_discr = u128::MAX;
    let mut max_discr = 0u128;
    let discr_size = discr_int.size();
    for &(v_idx, d) in &discrs {
        if !repr.c() && absent(&variants[v_idx]) {
            continue;
        }
        let val = d & discr_size.unsigned_int_max();
        min_discr = min_discr.min(val);
        max_discr = max_discr.max(val);
    }
    let tag_scalar = Scalar::Initialized {
        value: Primitive::Int(discr_int, signed),
        valid_range: WrappingRange { start: min_discr, end: max_discr },
    };
    let tag_align = tag_scalar.default_align(dl);

    // `@repr(C)` puts the discriminant first (at offset 0) followed by a union of all variants.
    // `@repr(Rust)` prefixes each variant struct with the tag so payloads pack tightly after the tag.
    let mut variant_layouts = IndexVec::with_capacity(variants.len());
    let mut align = dl.aggregate_align.max(tag_align);
    let mut max_repr_align = repr.align;
    let mut size = discr_size;

    let mut repr_c_all_fieldless = true;
    for fields in variants.iter() {
        if !fields.is_empty() {
            repr_c_all_fieldless = false;
            break;
        }
    }

    for (v_idx, fields) in variants.iter_enumerated() {
        let mut vl = if repr.c() {
            let inner = calculator.layout_of_univariant(fields, repr, StructKind::AlwaysSized)?;
            // In C repr, tag is separate at offset 0 and variant payload starts after tag (at aligned union start).
            inner
        } else {
            calculator.layout_of_univariant(
                fields,
                repr,
                StructKind::Prefixed(discr_size, tag_align),
            )?
        };
        if !fields.is_empty() || !repr.c() {
            align = align.max(vl.align);
            max_repr_align = max_repr_align.max(vl.max_repr_align);
        }
        if repr.c() {
            let payload_offset = discr_size.align_to(vl.align);
            let total = payload_offset
                .checked_add(vl.size, dl)
                .ok_or(LayoutCalculatorError::SizeOverflow)?;
            size = cmp::max(size, total);
        } else {
            size = cmp::max(size, vl.size);
        }
        vl.variants = Variants::Single { index: v_idx };
        vl.repr_c = repr.c();
        variant_layouts.push(vl);
    }

    if let Some(repr_align) = repr.align {
        align = align.max(repr_align);
    }
    size = size.align_to(align);
    if size.bytes() >= dl.obj_size_bound() {
        return Err(LayoutCalculatorError::SizeOverflow);
    }

    if repr.c() {
        let union_align = variant_layouts.iter().fold(dl.i8_align, |a, v| a.max(v.align));
        let payload_offset = discr_size.align_to(union_align);
        for vl in variant_layouts.iter_mut() {
            if let FieldsShape::Arbitrary { offsets, .. } = &mut vl.fields {
                for off in offsets.iter_mut() {
                    *off += payload_offset;
                }
            }
            vl.size = size;
            vl.align = align;
        }
        if repr_c_all_fieldless {
            size = discr_size.align_to(align);
        }
    } else {
        for vl in variant_layouts.iter_mut() {
            vl.size = size;
            vl.align = align;
        }
    }

    let backend_repr = if repr_c_all_fieldless
        || variant_layouts.iter().all(|v| v.fields.count() == 0)
    {
        BackendRepr::Scalar(tag_scalar)
    } else if let Some(pair) = try_scalar_pair(&calculator.cx, &variant_layouts, variants, size, align, tag_scalar) {
        pair
    } else {
        BackendRepr::Memory { sized: true }
    };

    Ok(LayoutData {
        variants: Variants::Multiple {
            tag: tag_scalar,
            tag_encoding: TagEncoding::Direct,
            tag_field: FieldIdx::new(0),
            variants: variant_layouts,
        },
        fields: FieldsShape::Arbitrary {
            offsets: IndexVec::from_raw(vec![Size::ZERO]),
            in_memory_order: IndexVec::from_raw(vec![FieldIdx::new(0)]),
        },
        backend_repr,
        largest_niche: Niche::from_scalar(&calculator.cx, Size::ZERO, tag_scalar),
        uninhabited: false,
        align,
        size,
        max_repr_align,
        unadjusted_abi_align: align,
        repr_c: repr.c(),
        randomization_seed: 0,
    })
}

fn try_scalar_pair<Cx: HasDataLayout, VariantIdx: Idx, FieldIdx: Idx, F>(
    cx: &Cx,
    variant_layouts: &IndexSlice<VariantIdx, LayoutData<FieldIdx, VariantIdx>>,
    variants: &IndexSlice<VariantIdx, IndexVec<FieldIdx, F>>,
    size: Size,
    align: Align,
    tag: Scalar,
) -> Option<BackendRepr>
where
    F: Deref<Target = LayoutData<FieldIdx, VariantIdx>> + std::fmt::Debug + Copy,
{
    let dl = cx.data_layout();
    let mut common_prim: Option<(Primitive, Size)> = None;
    let mut initialized_in_all = true;

    for (fields, vl) in variants.iter().zip(variant_layouts.iter()) {
        let mut non_zst = (0..fields.len())
            .map(|i| (&fields[FieldIdx::new(i)], vl.fields.offset(i)))
            .filter(|(f, _)| !f.is_zst());
        let (field, offset) = match (non_zst.next(), non_zst.next()) {
            (None, None) => {
                initialized_in_all = false;
                continue;
            }
            (Some(pair), None) => pair,
            _ => return None,
        };
        let prim = match field.backend_repr {
            BackendRepr::Scalar(s) => {
                initialized_in_all &= matches!(s, Scalar::Initialized { .. });
                s.primitive()
            }
            _ => return None,
        };
        if let Some((old_prim, old_off)) = common_prim {
            if offset != old_off {
                return None;
            }
            let merged = match (old_prim, prim) {
                (a, b) if a == b => a,
                (p @ Primitive::Int(a, _), Primitive::Int(b, _)) if a == b => p,
                (p @ Primitive::Pointer(_), i @ Primitive::Int(..))
                | (i @ Primitive::Int(..), p @ Primitive::Pointer(_))
                    if p.size(dl) == i.size(dl) && p.default_align(dl) == i.default_align(dl) =>
                {
                    p
                }
                _ => return None,
            };
            common_prim = Some((merged, old_off));
        } else {
            common_prim = Some((prim, offset));
        }
    }

    let (primitive, payload_offset) = common_prim?;
    let prim_scalar = if initialized_in_all {
        let p_size = primitive.size(dl);
        Scalar::Initialized { value: primitive, valid_range: WrappingRange::full(p_size) }
    } else {
        Scalar::Union { value: primitive }
    };
    let pair = LayoutData::<FieldIdx, VariantIdx>::scalar_pair(cx, tag, prim_scalar);
    if pair.fields.offset(0) == Size::ZERO
        && pair.fields.offset(1) == payload_offset
        && pair.align == align
        && pair.size == size
    {
        Some(pair.backend_repr)
    } else {
        None
    }
}
