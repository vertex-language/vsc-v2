use std::fmt;

use vsc_abi::{
    BackendRepr, CanonAbi, ExternAbi, FieldIdx, Integer, Layout, LayoutCalculator, LayoutData,
    PointeeInfo, Primitive, ReprFlags, ReprOptions, Scalar, Size, StructKind, TyAbiInterface,
    TyAndLayout, VariantIdx, WrappingRange,
};
use vsc_index::IndexVec;

use crate::callconv::{ArgAbi, ArgAttributes, ArgExtension, FnAbi, PassMode};
use crate::spec::{Arch, AbiMap, BinaryFormat, Target, TargetAndDataLayout};

#[derive(Copy, Clone, Debug)]
struct DummyTy(&'static str);

impl fmt::Display for DummyTy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

struct DummyCx<'a> {
    inner: TargetAndDataLayout<'a>,
    i32_layout: Option<Layout<'a>>,
}

impl vsc_abi::HasDataLayout for DummyCx<'_> {
    fn data_layout(&self) -> &vsc_abi::TargetDataLayout {
        self.inner.data_layout()
    }
}

impl crate::spec::HasTargetSpec for DummyCx<'_> {
    fn target_spec(&self) -> &Target {
        self.inner.target_spec()
    }
}

impl crate::spec::HasX86AbiOpt for DummyCx<'_> {
    fn x86_abi_opt(&self) -> crate::spec::X86AbiOpt {
        self.inner.x86_abi_opt()
    }
}

impl<'a> TyAbiInterface<'a, DummyCx<'a>> for DummyTy {
    fn ty_and_layout_for_variant(
        this: TyAndLayout<'a, Self>,
        _cx: &DummyCx<'a>,
        _variant_index: VariantIdx,
    ) -> TyAndLayout<'a, Self> {
        this
    }

    fn ty_and_layout_field(
        this: TyAndLayout<'a, Self>,
        cx: &DummyCx<'a>,
        _i: usize,
    ) -> TyAndLayout<'a, Self> {
        if this.ty.0 == "Pair" {
            return TyAndLayout {
                ty: DummyTy("i32"),
                layout: cx.i32_layout.expect("i32_layout must be set"),
            };
        }
        this
    }

    fn ty_and_layout_pointee_info_at(
        _this: TyAndLayout<'a, Self>,
        _cx: &DummyCx<'a>,
        _offset: Size,
    ) -> Option<PointeeInfo> {
        None
    }

    fn is_adt(_this: TyAndLayout<'a, Self>) -> bool {
        false
    }
    fn is_never(_this: TyAndLayout<'a, Self>) -> bool {
        false
    }
    fn is_tuple(_this: TyAndLayout<'a, Self>) -> bool {
        false
    }
    fn is_unit(_this: TyAndLayout<'a, Self>) -> bool {
        false
    }
}

#[test]
fn all_builtin_targets_parse() {
    for triple in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-linux-android",
        "x86_64-pc-windows-msvc",
        "aarch64-pc-windows-msvc",
    ] {
        let target = Target::from_triple(triple).unwrap();
        let dl = target.parse_data_layout().unwrap();
        assert_eq!(dl.pointer_size(), Size::from_bytes(8));
        let map = AbiMap::from_target(&target);
        assert_eq!(map.canonize_abi(ExternAbi::C { unwind: false }, false).unwrap(), CanonAbi::C);
    }

    let mac = Target::from_triple("aarch64-apple-darwin").unwrap();
    assert_eq!(mac.target_triple, "arm64-apple-macosx");
    assert_eq!(mac.arch, Arch::AArch64);
    assert_eq!(mac.binary_format, BinaryFormat::MachO);
    assert!(mac.is_like_darwin);
}

#[test]
fn darwinpcs_extends_small_ints_and_casts_small_struct() {
    let target = Target::from_triple("aarch64-apple-darwin").unwrap();
    let dl_cx = DummyCx { inner: TargetAndDataLayout::new(&target), i32_layout: None };

    let i8_layout_data = LayoutData::<FieldIdx, VariantIdx>::scalar(
        &dl_cx,
        Scalar::Initialized {
            value: Primitive::Int(Integer::I8, true),
            valid_range: WrappingRange::full(Size::from_bytes(1)),
        },
    );
    let calc = LayoutCalculator::new(&dl_cx);
    let i32_layout_data = LayoutData::<FieldIdx, VariantIdx>::scalar(
        &dl_cx,
        Scalar::Initialized {
            value: Primitive::Int(Integer::I32, true),
            valid_range: WrappingRange::full(Size::from_bytes(4)),
        },
    );
    let small_struct_fields = IndexVec::from_raw(vec![&i32_layout_data, &i32_layout_data]);
    let small_struct = calc
        .layout_of_univariant(
            &small_struct_fields,
            &ReprOptions { flags: ReprFlags::IS_C, ..Default::default() },
            StructKind::AlwaysSized,
        )
        .unwrap();
    let cx = DummyCx {
        inner: TargetAndDataLayout::new(&target),
        i32_layout: Some(Layout(&i32_layout_data)),
    };

    let mut fn_abi = FnAbi {
        args: Box::new([
            ArgAbi::new(
                TyAndLayout { ty: DummyTy("i8"), layout: Layout(&i8_layout_data) },
                |_, _| ArgAttributes::new(),
            ),
            ArgAbi::new(
                TyAndLayout { ty: DummyTy("Pair"), layout: Layout(&small_struct) },
                |_, _| ArgAttributes::new(),
            ),
        ]),
        ret: ArgAbi::new(
            TyAndLayout { ty: DummyTy("i8"), layout: Layout(&i8_layout_data) },
            |_, _| ArgAttributes::new(),
        ),
        c_variadic: false,
        fixed_count: 2,
        conv: CanonAbi::C,
        can_unwind: false,
    };
    fn_abi.adjust_for_foreign_abi(&cx, ExternAbi::C { unwind: false });

    assert!(matches!(
        fn_abi.args[0].mode,
        PassMode::Direct(ref a) if a.arg_ext == ArgExtension::Sext
    ));
    assert!(matches!(
        fn_abi.ret.mode,
        PassMode::Direct(ref a) if a.arg_ext == ArgExtension::Sext
    ));
    assert!(matches!(fn_abi.args[1].mode, PassMode::Cast { .. }));
    assert!(matches!(small_struct.backend_repr, BackendRepr::ScalarPair { .. }));
}
