use vsc_index::IndexVec;

use super::*;

#[test]
fn parse_darwin_arm64_datalayout() {
    let dl = TargetDataLayout::parse_from_datalayout_string(
        "e-m:o-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-n32:64-S128-Fn32",
        AddressSpace::ZERO,
    )
    .unwrap();
    assert_eq!(dl.endian, Endian::Little);
    assert_eq!(dl.pointer_size(), Size::from_bytes(8));
    assert_eq!(dl.pointer_align(), Align::EIGHT);
    assert_eq!(dl.i64_align, Align::EIGHT);
    assert_eq!(dl.i128_align, Align::SIXTEEN);
}

#[test]
fn parse_x86_64_linux_datalayout() {
    let dl = TargetDataLayout::parse_from_datalayout_string(
        "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128",
        AddressSpace::ZERO,
    )
    .unwrap();
    assert_eq!(dl.endian, Endian::Little);
    assert_eq!(dl.pointer_size(), Size::from_bytes(8));
    assert_eq!(dl.x87_f80_align, Align::SIXTEEN);
}

#[test]
fn struct_reordering_vs_repr_c() {
    let dl = TargetDataLayout::default();
    let calc = LayoutCalculator::new(&dl);
    let u8_s = LayoutData::<FieldIdx, VariantIdx>::scalar(
        &dl,
        Scalar::Initialized {
            value: Primitive::Int(Integer::I8, false),
            valid_range: WrappingRange::full(Size::from_bytes(1)),
        },
    );
    let i64_s = LayoutData::<FieldIdx, VariantIdx>::scalar(
        &dl,
        Scalar::Initialized {
            value: Primitive::Int(Integer::I64, true),
            valid_range: WrappingRange::full(Size::from_bytes(8)),
        },
    );
    let fields = IndexVec::from_raw(vec![&u8_s, &i64_s, &u8_s]);

    let rust_layout = calc
        .layout_of_univariant(&fields, &ReprOptions::default(), StructKind::AlwaysSized)
        .unwrap();
    assert_eq!(rust_layout.size, Size::from_bytes(16));
    assert_eq!(rust_layout.align, Align::EIGHT);

    let c_repr = ReprOptions { flags: ReprFlags::IS_C, ..Default::default() };
    let c_layout = calc.layout_of_univariant(&fields, &c_repr, StructKind::AlwaysSized).unwrap();
    assert_eq!(c_layout.size, Size::from_bytes(24));
    assert_eq!(c_layout.fields.offset(0), Size::from_bytes(0));
    assert_eq!(c_layout.fields.offset(1), Size::from_bytes(8));
    assert_eq!(c_layout.fields.offset(2), Size::from_bytes(16));
    assert!(c_layout.is_repr_c());
}

#[test]
fn option_bool_uses_niche() {
    let dl = TargetDataLayout::default();
    let calc = LayoutCalculator::new(&dl);
    let bool_l = LayoutData::<FieldIdx, VariantIdx>::scalar(
        &dl,
        Scalar::Initialized {
            value: Primitive::Int(Integer::I8, false),
            valid_range: WrappingRange { start: 0, end: 1 },
        },
    );
    let variants = IndexVec::from_raw(vec![IndexVec::new(), IndexVec::from_raw(vec![&bool_l])]);
    let discrs = vec![(VariantIdx::from_usize(0), 0u128), (VariantIdx::from_usize(1), 1u128)];
    let layout = calc
        .layout_of_struct_or_enum(
            &ReprOptions::default(),
            &variants,
            true,
            false,
            |_, _| (Integer::I8, false),
            discrs.into_iter(),
            true,
        )
        .unwrap();
    assert_eq!(layout.size, Size::from_bytes(1));
    assert_eq!(layout.align, Align::ONE);
    assert!(matches!(
        layout.variants,
        Variants::Multiple { tag_encoding: TagEncoding::Niche { .. }, .. }
    ));
}

#[test]
fn tagged_enum_layout() {
    let dl = TargetDataLayout::default();
    let calc = LayoutCalculator::new(&dl);
    let i32_l = LayoutData::<FieldIdx, VariantIdx>::scalar(
        &dl,
        Scalar::Initialized {
            value: Primitive::Int(Integer::I32, true),
            valid_range: WrappingRange::full(Size::from_bytes(4)),
        },
    );
    let variants =
        IndexVec::from_raw(vec![IndexVec::from_raw(vec![&i32_l]), IndexVec::from_raw(vec![&i32_l])]);
    let discrs = vec![(VariantIdx::from_usize(0), 0u128), (VariantIdx::from_usize(1), 1u128)];
    let layout = calc
        .layout_of_struct_or_enum(
            &ReprOptions::default(),
            &variants,
            true,
            false,
            |_, _| (Integer::I8, false),
            discrs.into_iter(),
            true,
        )
        .unwrap();
    assert_eq!(layout.size, Size::from_bytes(8));
    assert_eq!(layout.align, Align::FOUR);
    assert!(matches!(layout.backend_repr, BackendRepr::ScalarPair { .. }));
}
