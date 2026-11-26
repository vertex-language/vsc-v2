//! Interned layout wrapper and type-level ABI interface (`rustc_abi::layout::ty`).

use std::fmt;
use std::ops::Deref;

use crate::layout::{FieldIdx, VariantIdx};
use crate::{
    Align, BackendRepr, FieldsShape, HasDataLayout, LayoutData, Niche, PointeeInfo, Size, Variants,
};

/// Reference to a computed layout stored in the compiler arena.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct Layout<'a>(pub &'a LayoutData<FieldIdx, VariantIdx>);

impl<'a> fmt::Debug for Layout<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a> Deref for Layout<'a> {
    type Target = LayoutData<FieldIdx, VariantIdx>;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'a> Layout<'a> {
    #[inline]
    pub fn fields(self) -> &'a FieldsShape<FieldIdx> {
        &self.0.fields
    }

    #[inline]
    pub fn variants(self) -> &'a Variants<FieldIdx, VariantIdx> {
        &self.0.variants
    }

    #[inline]
    pub fn backend_repr(self) -> BackendRepr {
        self.0.backend_repr
    }

    #[inline]
    pub fn largest_niche(self) -> Option<Niche> {
        self.0.largest_niche
    }

    #[inline]
    pub fn align(self) -> Align {
        self.0.align
    }

    #[inline]
    pub fn size(self) -> Size {
        self.0.size
    }

    #[inline]
    pub fn max_repr_align(self) -> Option<Align> {
        self.0.max_repr_align
    }

    #[inline]
    pub fn unadjusted_abi_align(self) -> Align {
        self.0.unadjusted_abi_align
    }
}

/// A type paired with its computed layout (`rustc_abi::TyAndLayout`).
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct TyAndLayout<'a, Ty> {
    pub ty: Ty,
    pub layout: Layout<'a>,
}

impl<'a, Ty: fmt::Display> fmt::Debug for TyAndLayout<'a, Ty> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TyAndLayout")
            .field("ty", &format_args!("{}", self.ty))
            .field("layout", &self.layout)
            .finish()
    }
}

impl<'a, Ty> Deref for TyAndLayout<'a, Ty> {
    type Target = LayoutData<FieldIdx, VariantIdx>;
    #[inline]
    fn deref(&self) -> &Self::Target {
        self.layout.0
    }
}

impl<'a, Ty> AsRef<LayoutData<FieldIdx, VariantIdx>> for TyAndLayout<'a, Ty> {
    #[inline]
    fn as_ref(&self) -> &LayoutData<FieldIdx, VariantIdx> {
        self.layout.0
    }
}

/// Interface implemented by the compiler's type representation so `vsc_abi` and
/// `vsc_target` can traverse fields, variants, and pointee info generically.
pub trait TyAbiInterface<'a, C>: Sized + fmt::Debug + fmt::Display {
    fn ty_and_layout_for_variant(
        this: TyAndLayout<'a, Self>,
        cx: &C,
        variant_index: VariantIdx,
    ) -> TyAndLayout<'a, Self>;

    fn ty_and_layout_field(this: TyAndLayout<'a, Self>, cx: &C, i: usize) -> TyAndLayout<'a, Self>;

    fn ty_and_layout_pointee_info_at(
        this: TyAndLayout<'a, Self>,
        cx: &C,
        offset: Size,
    ) -> Option<PointeeInfo>;

    fn is_adt(this: TyAndLayout<'a, Self>) -> bool;
    fn is_never(this: TyAndLayout<'a, Self>) -> bool;
    fn is_tuple(this: TyAndLayout<'a, Self>) -> bool;
    fn is_unit(this: TyAndLayout<'a, Self>) -> bool;

    fn is_pass_indirectly_in_non_rustic_abis(
        this: TyAndLayout<'a, Self>,
        cx: &C,
    ) -> bool {
        let _ = (this, cx);
        false
    }
}

impl<'a, Ty> TyAndLayout<'a, Ty> {
    #[inline]
    pub fn for_variant<C>(self, cx: &C, variant_index: VariantIdx) -> Self
    where
        Ty: TyAbiInterface<'a, C>,
    {
        Ty::ty_and_layout_for_variant(self, cx, variant_index)
    }

    #[inline]
    pub fn field<C>(self, cx: &C, i: usize) -> Self
    where
        Ty: TyAbiInterface<'a, C>,
    {
        Ty::ty_and_layout_field(self, cx, i)
    }

    #[inline]
    pub fn pointee_info_at<C>(self, cx: &C, offset: Size) -> Option<PointeeInfo>
    where
        Ty: TyAbiInterface<'a, C>,
    {
        Ty::ty_and_layout_pointee_info_at(self, cx, offset)
    }

    #[inline]
    pub fn pass_indirectly_in_non_rustic_abis<C>(self, cx: &C) -> bool
    where
        Ty: TyAbiInterface<'a, C>,
    {
        Ty::is_pass_indirectly_in_non_rustic_abis(self, cx)
    }

    #[inline]
    pub fn is_x87_f80<C: HasDataLayout>(self, _cx: &C) -> bool {
        self.backend_repr.is_x87_f80()
    }
}
