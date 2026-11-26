//! Function argument and return value ABI classification (`rustc_target::callconv`).

use std::{fmt, iter};

use vsc_abi::{
    AddressSpace, Align, BackendRepr, CanonAbi, ExternAbi, HasDataLayout, Integer, Primitive, Reg,
    Scalar, Size, TyAbiInterface, TyAndLayout,
};

use crate::spec::{Arch, HasTargetSpec, HasX86AbiOpt};

mod aarch64;
mod x86_64;
mod x86_win64;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum IndirectMode {
    Pointer,
    OnStack,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum PassMode {
    Ignore,
    Direct(ArgAttributes),
    Pair(ArgAttributes, ArgAttributes),
    Cast { pad_i32_count: u8, cast: Box<CastTarget> },
    Indirect { attrs: ArgAttributes, address_space: Option<AddressSpace>, mode: IndirectMode },
}

impl PassMode {
    pub fn eq_abi(&self, other: &Self) -> bool {
        match (self, other) {
            (PassMode::Ignore, PassMode::Ignore) => true,
            (PassMode::Direct(a), PassMode::Direct(b)) => a.eq_abi(b),
            (PassMode::Pair(a1, a2), PassMode::Pair(b1, b2)) => a1.eq_abi(b1) && a2.eq_abi(b2),
            (
                PassMode::Cast { pad_i32_count: pa, cast: ca },
                PassMode::Cast { pad_i32_count: pb, cast: cb },
            ) => pa == pb && ca.eq_abi(cb),
            (
                PassMode::Indirect { attrs: aa, address_space: asa, mode: ma },
                PassMode::Indirect { attrs: ab, address_space: asb, mode: mb },
            ) => aa.eq_abi(ab) && asa == asb && ma == mb,
            _ => false,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ArgExtension {
    None,
    Zext,
    Sext,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct ArgAttribute(u16);

impl ArgAttribute {
    pub const NO_ALIAS: Self = Self(1 << 0);
    pub const NO_CAPTURE: Self = Self(1 << 1);
    pub const NON_NULL: Self = Self(1 << 2);
    pub const READ_ONLY: Self = Self(1 << 3);
    pub const NO_UNDEF: Self = Self(1 << 4);
    pub const WRITABLE: Self = Self(1 << 5);
    pub const IN_REG: Self = Self(1 << 6);

    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ArgAttributes {
    pub regular: ArgAttribute,
    pub arg_ext: ArgExtension,
    pub pointee_size: Size,
    pub pointee_align: Option<Align>,
}

impl Default for ArgAttributes {
    fn default() -> Self {
        Self::new()
    }
}

impl ArgAttributes {
    pub fn new() -> Self {
        ArgAttributes {
            regular: ArgAttribute::empty(),
            arg_ext: ArgExtension::None,
            pointee_size: Size::ZERO,
            pointee_align: None,
        }
    }

    pub fn ext(&mut self, ext: ArgExtension) -> &mut Self {
        assert!(self.arg_ext == ArgExtension::None || self.arg_ext == ext);
        self.arg_ext = ext;
        self
    }

    pub fn set(&mut self, attr: ArgAttribute) -> &mut Self {
        self.regular.insert(attr);
        self
    }

    pub fn contains(&self, attr: ArgAttribute) -> bool {
        self.regular.contains(attr)
    }

    pub fn eq_abi(&self, other: &Self) -> bool {
        self.regular.contains(ArgAttribute::IN_REG)
            == other.regular.contains(ArgAttribute::IN_REG)
            && self.arg_ext == other.arg_ext
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Uniform {
    pub unit: Reg,
    pub total: Size,
    pub is_consecutive: bool,
}

impl Uniform {
    pub fn new(unit: Reg, total: Size) -> Self {
        Self { unit, total, is_consecutive: false }
    }

    pub fn consecutive(unit: Reg, total: Size) -> Self {
        Self { unit, total, is_consecutive: true }
    }
}

impl From<Reg> for Uniform {
    fn from(unit: Reg) -> Uniform {
        Uniform { unit, total: unit.size, is_consecutive: false }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct CastTarget {
    pub prefix: Vec<Reg>,
    pub rest_offset: Option<Size>,
    pub rest: Uniform,
    pub attrs: ArgAttributes,
}

impl From<Reg> for CastTarget {
    fn from(unit: Reg) -> CastTarget {
        CastTarget::from(Uniform::from(unit))
    }
}

impl From<Uniform> for CastTarget {
    fn from(rest: Uniform) -> CastTarget {
        CastTarget {
            prefix: Vec::new(),
            rest_offset: None,
            rest,
            attrs: ArgAttributes::new(),
        }
    }
}

impl CastTarget {
    pub fn prefixed(prefix: Vec<Reg>, rest: Uniform) -> CastTarget {
        CastTarget { prefix, rest_offset: None, rest, attrs: ArgAttributes::new() }
    }

    pub fn pair(a: Reg, b: Reg) -> CastTarget {
        CastTarget {
            prefix: vec![a],
            rest_offset: None,
            rest: Uniform::from(b),
            attrs: ArgAttributes::new(),
        }
    }

    pub fn unaligned_size<C: HasDataLayout>(&self, _cx: &C) -> Size {
        let prefix_size = if let Some(offset_from_start) = self.rest_offset {
            offset_from_start
        } else {
            self.prefix.iter().map(|r| r.size).fold(Size::ZERO, |acc, s| acc + s)
        };
        let rest_size =
            self.rest.unit.size * self.rest.total.bytes().div_ceil(self.rest.unit.size.bytes());
        prefix_size + rest_size
    }

    pub fn size<C: HasDataLayout>(&self, cx: &C) -> Size {
        self.unaligned_size(cx).align_to(self.align(cx))
    }

    pub fn align<C: HasDataLayout>(&self, cx: &C) -> Align {
        self.prefix.iter().map(|r| r.scratch_align(cx)).fold(
            cx.data_layout().aggregate_align.max(self.rest.unit.scratch_align(cx)),
            |acc, align| acc.max(align),
        )
    }

    pub fn eq_abi(&self, other: &Self) -> bool {
        self.prefix == other.prefix
            && self.rest_offset == other.rest_offset
            && self.rest == other.rest
            && self.attrs.eq_abi(&other.attrs)
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ArgAbi<'a, Ty> {
    pub layout: TyAndLayout<'a, Ty>,
    pub mode: PassMode,
}

impl<'a, Ty: fmt::Display> fmt::Debug for ArgAbi<'a, Ty> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArgAbi").field("layout", &self.layout).field("mode", &self.mode).finish()
    }
}

impl<'a, Ty> ArgAbi<'a, Ty> {
    pub fn new(
        layout: TyAndLayout<'a, Ty>,
        scalar_attrs: impl Fn(Scalar, Size) -> ArgAttributes,
    ) -> Self {
        let mode = match layout.backend_repr {
            _ if layout.is_zst() => PassMode::Ignore,
            BackendRepr::Scalar(scalar) => PassMode::Direct(scalar_attrs(scalar, Size::ZERO)),
            BackendRepr::ScalarPair { a, b, b_offset } => {
                PassMode::Pair(scalar_attrs(a, Size::ZERO), scalar_attrs(b, b_offset))
            }
            BackendRepr::SimdVector { .. } => PassMode::Direct(ArgAttributes::new()),
            BackendRepr::Memory { .. } => Self::indirect_pass_mode(&layout),
        };
        ArgAbi { layout, mode }
    }

    fn indirect_pass_mode(layout: &TyAndLayout<'a, Ty>) -> PassMode {
        let mut attrs = ArgAttributes::new();
        attrs.pointee_size = layout.size;
        attrs.pointee_align = Some(layout.align);
        PassMode::Indirect { attrs, address_space: None, mode: IndirectMode::Pointer }
    }

    #[inline]
    pub fn is_ignore(&self) -> bool {
        matches!(self.mode, PassMode::Ignore)
    }

    #[inline]
    pub fn is_indirect(&self) -> bool {
        matches!(self.mode, PassMode::Indirect { .. })
    }

    #[inline]
    pub fn make_indirect(&mut self) {
        self.mode = Self::indirect_pass_mode(&self.layout);
    }

    #[inline]
    pub fn make_indirect_from_ignore(&mut self) {
        self.make_indirect();
    }

    #[inline]
    pub fn pass_by_stack_offset(&mut self, align: Option<Align>) {
        let mut attrs = ArgAttributes::new();
        attrs.pointee_size = self.layout.size;
        attrs.pointee_align = Some(align.unwrap_or(self.layout.align));
        self.mode = PassMode::Indirect {
            attrs,
            address_space: None,
            mode: IndirectMode::OnStack,
        };
    }

    #[inline]
    pub fn cast_to<T: Into<CastTarget>>(&mut self, target: T) {
        self.mode = PassMode::Cast { pad_i32_count: 0, cast: Box::new(target.into()) };
    }

    pub fn extend_integer_width_to(&mut self, bits: u64) {
        if let BackendRepr::Scalar(scalar) = self.layout.backend_repr
            && let Primitive::Int(i, signed) = scalar.primitive()
            && i.size().bits() < bits
            && let PassMode::Direct(attrs) = &mut self.mode
        {
            if i == Integer::I8 && scalar.is_bool() {
                attrs.ext(ArgExtension::Zext);
            } else {
                attrs.ext(if signed { ArgExtension::Sext } else { ArgExtension::Zext });
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct FnAbi<'a, Ty> {
    pub args: Box<[ArgAbi<'a, Ty>]>,
    pub ret: ArgAbi<'a, Ty>,
    pub c_variadic: bool,
    pub fixed_count: u32,
    pub conv: CanonAbi,
    pub can_unwind: bool,
}

impl<'a, Ty: fmt::Display> fmt::Debug for FnAbi<'a, Ty> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FnAbi")
            .field("args", &self.args)
            .field("ret", &self.ret)
            .field("c_variadic", &self.c_variadic)
            .field("fixed_count", &self.fixed_count)
            .field("conv", &self.conv)
            .field("can_unwind", &self.can_unwind)
            .finish()
    }
}

impl<'a, Ty> FnAbi<'a, Ty> {
    pub fn adjust_for_foreign_abi<C>(&mut self, cx: &C, abi: ExternAbi)
    where
        Ty: TyAbiInterface<'a, C> + Copy,
        C: HasDataLayout + HasTargetSpec + HasX86AbiOpt,
    {
        let spec = cx.target_spec();
        match &spec.arch {
            Arch::X86_64 => match abi {
                ExternAbi::SysV64 { .. } => x86_64::compute_abi_info(cx, self),
                ExternAbi::Win64 { .. } | ExternAbi::Vectorcall { .. } => {
                    x86_win64::compute_abi_info(cx, self)
                }
                _ => {
                    if spec.is_like_windows {
                        x86_win64::compute_abi_info(cx, self)
                    } else {
                        x86_64::compute_abi_info(cx, self)
                    }
                }
            },
            Arch::AArch64 | Arch::Arm64EC => {
                let kind = if spec.is_like_darwin {
                    aarch64::AbiKind::DarwinPCS
                } else if spec.is_like_windows {
                    aarch64::AbiKind::Win64
                } else {
                    aarch64::AbiKind::AAPCS
                };
                aarch64::compute_abi_info(cx, self, kind);
            }
            other => panic!("no foreign ABI lowering implemented for {other}"),
        }
    }

    pub fn adjust_for_rust_abi<C>(&mut self, cx: &C)
    where
        Ty: TyAbiInterface<'a, C> + Copy,
        C: HasDataLayout + HasTargetSpec,
    {
        let spec = cx.target_spec();
        if matches!(spec.arch, Arch::AArch64) {
            aarch64::compute_rust_abi_info(cx, self);
        }

        let max_direct_bits = spec.pointer_width as u64 * 2;
        for arg in self.args.iter_mut().chain(iter::once(&mut self.ret)) {
            if matches!(arg.mode, PassMode::Ignore | PassMode::Cast { .. }) {
                continue;
            }
            if !arg.layout.is_sized() {
                continue;
            }
            match arg.layout.backend_repr {
                BackendRepr::Scalar(_) => {}
                BackendRepr::ScalarPair { .. } => {}
                BackendRepr::SimdVector { .. } => {
                    if arg.layout.size.bits() > 128 {
                        arg.make_indirect();
                    }
                }
                BackendRepr::Memory { .. } => {
                    if arg.layout.size.bits() > max_direct_bits
                        && !matches!(arg.mode, PassMode::Indirect { .. })
                    {
                        arg.make_indirect();
                    }
                }
            }
        }
    }
}
