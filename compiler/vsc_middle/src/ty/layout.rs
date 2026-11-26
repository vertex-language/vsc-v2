//! Type layout and ABI queries (`rustc_middle::ty::layout`).

use std::convert::Infallible;
use std::fmt;

pub use vsc_abi::{
    AddressSpace, Align, BackendRepr, CanonAbi, ExternAbi, FIRST_VARIANT, FieldIdx, FieldsShape,
    Float, HasDataLayout, Integer, IntegerType, Layout, LayoutCalculator, LayoutCalculatorError,
    Niche, PointeeInfo, PointerKind, Primitive, ReprFlags, ReprOptions, Scalar, Size, StructKind,
    TagEncoding, TargetDataLayout, TyAbiInterface, VariantIdx, Variants, WrappingRange,
};
pub use vsc_target::callconv::{
    ArgAbi, ArgAttribute, ArgAttributes, ArgExtension, CastTarget, FnAbi, IndirectMode, PassMode,
    Uniform,
};
pub use vsc_target::spec::{HasTargetSpec, HasX86AbiOpt, Target, TargetAndDataLayout, X86AbiOpt};
use vsc_ast::Mutability;
use vsc_span::{Span, Symbol, DUMMY_SP};

use crate::ty::{FloatTy, Instance, IntTy, PolyFnSig, Ty, TyCtxt, TyKind, UintTy};

pub type TyAndLayout<'tcx> = vsc_abi::TyAndLayout<'tcx, Ty<'tcx>>;

pub trait HasTyCtxt<'tcx> {
    fn tcx(&self) -> TyCtxt<'tcx>;
}

/// Convert an optional ABI name symbol (`extern "C"`, `extern "system"`, ...) to `ExternAbi`.
pub fn extern_abi_from_symbol(abi: Option<Symbol>) -> ExternAbi {
    let Some(abi) = abi else {
        return ExternAbi::Rust;
    };
    match abi.as_str() {
        "C" | "c" => ExternAbi::C { unwind: false },
        "C-unwind" => ExternAbi::C { unwind: true },
        "Rust" => ExternAbi::Rust,
        "rust-call" => ExternAbi::RustCall,
        "rust-cold" => ExternAbi::RustCold,
        "rust-intrinsic" => ExternAbi::Intrinsic,
        "system" => ExternAbi::System { unwind: false },
        "system-unwind" => ExternAbi::System { unwind: true },
        "cdecl" => ExternAbi::Cdecl { unwind: false },
        "cdecl-unwind" => ExternAbi::Cdecl { unwind: true },
        "stdcall" => ExternAbi::Stdcall { unwind: false },
        "stdcall-unwind" => ExternAbi::Stdcall { unwind: true },
        "fastcall" => ExternAbi::Fastcall { unwind: false },
        "fastcall-unwind" => ExternAbi::Fastcall { unwind: true },
        "vectorcall" => ExternAbi::Vectorcall { unwind: false },
        "vectorcall-unwind" => ExternAbi::Vectorcall { unwind: true },
        "aapcs" => ExternAbi::Aapcs { unwind: false },
        "aapcs-unwind" => ExternAbi::Aapcs { unwind: true },
        "win64" => ExternAbi::Win64 { unwind: false },
        "win64-unwind" => ExternAbi::Win64 { unwind: true },
        "sysv64" => ExternAbi::SysV64 { unwind: false },
        "sysv64-unwind" => ExternAbi::SysV64 { unwind: true },
        "efiapi" => ExternAbi::EfiApi,
        _ => ExternAbi::C { unwind: false },
    }
}

pub trait IntegerExt {
    fn to_ty<'tcx>(&self, tcx: TyCtxt<'tcx>, signed: bool) -> Ty<'tcx>;
    fn from_int_ty<C: HasDataLayout>(cx: &C, ity: IntTy) -> Integer;
    fn from_uint_ty<C: HasDataLayout>(cx: &C, uty: UintTy) -> Integer;
    fn discr_range_of_repr(
        dl: &TargetDataLayout,
        repr: &ReprOptions,
        min_negative: i128,
        max_positive: u128,
    ) -> (Integer, bool);
}

impl IntegerExt for Integer {
    #[inline]
    fn to_ty<'tcx>(&self, tcx: TyCtxt<'tcx>, signed: bool) -> Ty<'tcx> {
        match (*self, signed) {
            (Integer::I8, false) => tcx.types.u8,
            (Integer::I16, false) => tcx.types.u16,
            (Integer::I32, false) => tcx.types.u32,
            (Integer::I64, false) => tcx.types.u64,
            (Integer::I128, false) => tcx.types.u128,
            (Integer::I8, true) => tcx.types.i8,
            (Integer::I16, true) => tcx.types.i16,
            (Integer::I32, true) => tcx.types.i32,
            (Integer::I64, true) => tcx.types.i64,
            (Integer::I128, true) => tcx.types.i128,
        }
    }

    #[inline]
    fn from_int_ty<C: HasDataLayout>(cx: &C, ity: IntTy) -> Integer {
        match ity {
            IntTy::I8 => Integer::I8,
            IntTy::I16 => Integer::I16,
            IntTy::I32 => Integer::I32,
            IntTy::I64 => Integer::I64,
            IntTy::I128 => Integer::I128,
            IntTy::Isize => cx.data_layout().ptr_sized_integer(),
        }
    }

    #[inline]
    fn from_uint_ty<C: HasDataLayout>(cx: &C, uty: UintTy) -> Integer {
        match uty {
            UintTy::U8 => Integer::I8,
            UintTy::U16 => Integer::I16,
            UintTy::U32 => Integer::I32,
            UintTy::U64 => Integer::I64,
            UintTy::U128 => Integer::I128,
            UintTy::Usize => cx.data_layout().ptr_sized_integer(),
        }
    }

    fn discr_range_of_repr(
        dl: &TargetDataLayout,
        repr: &ReprOptions,
        min_negative: i128,
        max_positive: u128,
    ) -> (Integer, bool) {
        if let Some(int) = repr.int {
            return match int {
                IntegerType::Pointer(signed) => (dl.ptr_sized_integer(), signed),
                IntegerType::Fixed(size, signed) => (size, signed),
            };
        }
        let unsigned_fit = Integer::fit_unsigned(max_positive);
        let signed_fit = Integer::fit_signed(min_negative).max(if max_positive <= i128::MAX as u128 {
            Integer::fit_signed(max_positive as i128)
        } else {
            Integer::I128
        });
        if let Some(align) = repr.align {
            let align_bytes = align.bytes();
            let _ = align_bytes;
        }
        if repr.c() {
            if min_negative >= 0 && max_positive <= u32::MAX as u128 {
                return (Integer::I32, false);
            }
            if min_negative >= i32::MIN as i128 && max_positive <= i32::MAX as u128 {
                return (Integer::I32, true);
            }
        }
        if min_negative < 0 {
            (signed_fit, true)
        } else {
            (unsigned_fit, false)
        }
    }
}

pub fn float_to_abi(f: FloatTy) -> Float {
    match f {
        FloatTy::F32 => Float::F32,
        FloatTy::F64 => Float::F64,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LayoutError<'tcx> {
    Unknown(Ty<'tcx>),
    SizeOverflow(Ty<'tcx>),
    TooGeneric(Ty<'tcx>),
    InvalidSimd(Ty<'tcx>),
    ReferencesError,
}

impl fmt::Display for LayoutError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::Unknown(ty) => write!(f, "layout of type `{ty}` is unknown"),
            LayoutError::SizeOverflow(ty) => write!(f, "type `{ty}` is too big for target"),
            LayoutError::TooGeneric(ty) => write!(f, "type `{ty}` is too generic for layout"),
            LayoutError::InvalidSimd(ty) => write!(f, "invalid SIMD type `{ty}`"),
            LayoutError::ReferencesError => write!(f, "type references a prior error"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FnAbiError<'tcx> {
    Layout(LayoutError<'tcx>),
    AdjustForForeignAbi(Ty<'tcx>),
}

impl<'tcx> From<LayoutError<'tcx>> for FnAbiError<'tcx> {
    fn from(err: LayoutError<'tcx>) -> Self {
        FnAbiError::Layout(err)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum FnAbiRequest<'tcx> {
    OfFnPtr { sig: PolyFnSig<'tcx>, extra_args: &'tcx [Ty<'tcx>] },
    OfInstance { instance: Instance<'tcx>, extra_args: &'tcx [Ty<'tcx>] },
}

/// Context implementing `HasDataLayout`, `HasTargetSpec`, and `HasTyCtxt` for layout and ABI queries.
#[derive(Clone, Copy)]
pub struct LayoutCx<'tcx> {
    pub tcx: TyCtxt<'tcx>,
}

impl<'tcx> LayoutCx<'tcx> {
    #[inline]
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        LayoutCx { tcx }
    }
}

impl HasDataLayout for LayoutCx<'_> {
    #[inline]
    fn data_layout(&self) -> &TargetDataLayout {
        &self.tcx.data_layout
    }
}

impl HasTargetSpec for LayoutCx<'_> {
    #[inline]
    fn target_spec(&self) -> &Target {
        &self.tcx.target_spec
    }
}

impl HasX86AbiOpt for LayoutCx<'_> {
    #[inline]
    fn x86_abi_opt(&self) -> X86AbiOpt {
        X86AbiOpt::default()
    }
}

impl<'tcx> HasTyCtxt<'tcx> for LayoutCx<'tcx> {
    #[inline]
    fn tcx(&self) -> TyCtxt<'tcx> {
        self.tcx
    }
}

pub trait MaybeResult<T> {
    type Error;
    fn from_ok(x: T) -> Self;
    fn from_err(err: Self::Error) -> Self;
    fn map_err_into<E>(res: Result<T, E>, f: impl FnOnce(E) -> Self::Error) -> Self;
}

impl<T, E> MaybeResult<T> for Result<T, E> {
    type Error = E;
    #[inline]
    fn from_ok(x: T) -> Self {
        Ok(x)
    }
    #[inline]
    fn from_err(err: Self::Error) -> Self {
        Err(err)
    }
    #[inline]
    fn map_err_into<E2>(res: Result<T, E2>, f: impl FnOnce(E2) -> E) -> Self {
        res.map_err(f)
    }
}

impl<T> MaybeResult<T> for T {
    type Error = Infallible;
    #[inline]
    fn from_ok(x: T) -> Self {
        x
    }
    #[inline]
    fn from_err(never: Infallible) -> Self {
        match never {}
    }
    #[inline]
    #[allow(unreachable_code)]
    fn map_err_into<E2>(res: Result<T, E2>, f: impl FnOnce(E2) -> Infallible) -> Self {
        match res {
            Ok(v) => v,
            Err(e) => match f(e) {},
        }
    }
}

pub trait LayoutOfHelpers<'tcx>: HasDataLayout + HasTyCtxt<'tcx> {
    type LayoutOfResult: MaybeResult<TyAndLayout<'tcx>>;

    #[inline]
    fn layout_tcx_at_span(&self) -> Span {
        DUMMY_SP
    }

    fn handle_layout_err(
        &self,
        err: LayoutError<'tcx>,
        span: Span,
        ty: Ty<'tcx>,
    ) -> <Self::LayoutOfResult as MaybeResult<TyAndLayout<'tcx>>>::Error;
}

pub trait LayoutOf<'tcx>: LayoutOfHelpers<'tcx> {
    #[inline]
    fn layout_of(&self, ty: Ty<'tcx>) -> Self::LayoutOfResult {
        self.spanned_layout_of(ty, DUMMY_SP)
    }

    #[inline]
    fn spanned_layout_of(&self, ty: Ty<'tcx>, span: Span) -> Self::LayoutOfResult {
        let span = if !span.is_dummy() { span } else { self.layout_tcx_at_span() };
        Self::LayoutOfResult::map_err_into(self.tcx().layout_of(ty), |err| {
            self.handle_layout_err(err, span, ty)
        })
    }
}

impl<'tcx, C: LayoutOfHelpers<'tcx>> LayoutOf<'tcx> for C {}

impl<'tcx> LayoutOfHelpers<'tcx> for LayoutCx<'tcx> {
    type LayoutOfResult = Result<TyAndLayout<'tcx>, LayoutError<'tcx>>;

    #[inline]
    fn handle_layout_err(
        &self,
        err: LayoutError<'tcx>,
        _span: Span,
        _ty: Ty<'tcx>,
    ) -> LayoutError<'tcx> {
        err
    }
}

pub trait FnAbiOfHelpers<'tcx>: LayoutOfHelpers<'tcx> {
    type FnAbiOfResult: MaybeResult<&'tcx FnAbi<'tcx, Ty<'tcx>>>;

    fn handle_fn_abi_err(
        &self,
        err: FnAbiError<'tcx>,
        span: Span,
        fn_abi_request: FnAbiRequest<'tcx>,
    ) -> <Self::FnAbiOfResult as MaybeResult<&'tcx FnAbi<'tcx, Ty<'tcx>>>>::Error;
}

pub trait FnAbiOf<'tcx>: FnAbiOfHelpers<'tcx> {
    #[inline]
    fn fn_abi_of_fn_ptr(
        &self,
        sig: PolyFnSig<'tcx>,
        extra_args: &'tcx [Ty<'tcx>],
    ) -> Self::FnAbiOfResult {
        let span = self.layout_tcx_at_span();
        Self::FnAbiOfResult::map_err_into(self.tcx().fn_abi_of_fn_ptr((sig, extra_args)), |err| {
            self.handle_fn_abi_err(err, span, FnAbiRequest::OfFnPtr { sig, extra_args })
        })
    }

    #[inline]
    fn fn_abi_of_instance(
        &self,
        instance: Instance<'tcx>,
        extra_args: &'tcx [Ty<'tcx>],
    ) -> Self::FnAbiOfResult {
        let span = self.layout_tcx_at_span();
        Self::FnAbiOfResult::map_err_into(
            self.tcx().fn_abi_of_instance((instance, extra_args)),
            |err| {
                self.handle_fn_abi_err(err, span, FnAbiRequest::OfInstance { instance, extra_args })
            },
        )
    }
}

impl<'tcx, C: FnAbiOfHelpers<'tcx>> FnAbiOf<'tcx> for C {}

impl<'tcx> FnAbiOfHelpers<'tcx> for LayoutCx<'tcx> {
    type FnAbiOfResult = Result<&'tcx FnAbi<'tcx, Ty<'tcx>>, FnAbiError<'tcx>>;

    #[inline]
    fn handle_fn_abi_err(
        &self,
        err: FnAbiError<'tcx>,
        _span: Span,
        _fn_abi_request: FnAbiRequest<'tcx>,
    ) -> FnAbiError<'tcx> {
        err
    }
}

impl<'tcx, C: HasDataLayout + HasTyCtxt<'tcx>> TyAbiInterface<'tcx, C> for Ty<'tcx> {
    fn ty_and_layout_for_variant(
        this: vsc_abi::TyAndLayout<'tcx, Self>,
        _cx: &C,
        variant_index: VariantIdx,
    ) -> vsc_abi::TyAndLayout<'tcx, Self> {
        match this.layout.variants() {
            Variants::Single { index } if *index == variant_index => this,
            Variants::Single { .. } | Variants::Empty => this,
            Variants::Multiple { variants, .. } => {
                vsc_abi::TyAndLayout { ty: this.ty, layout: Layout(&variants[variant_index]) }
            }
        }
    }

    fn ty_and_layout_field(
        this: vsc_abi::TyAndLayout<'tcx, Self>,
        cx: &C,
        i: usize,
    ) -> vsc_abi::TyAndLayout<'tcx, Self> {
        let tcx = cx.tcx();
        let variant_index = match this.layout.variants() {
            Variants::Single { index } => index.as_usize(),
            Variants::Multiple { tag, tag_field, .. } => {
                if i == tag_field.as_usize() {
                    let scalar_ty = match tag.primitive() {
                        Primitive::Int(int, signed) => int.to_ty(tcx, signed),
                        Primitive::Float(Float::F32) => tcx.types.f32,
                        Primitive::Float(Float::F64) => tcx.types.f64,
                        _ => tcx.types.usize,
                    };
                    return LayoutCx::new(tcx)
                        .layout_of(scalar_ty)
                        .expect("scalar tag layout must succeed");
                }
                0
            }
            Variants::Empty => 0,
        };

        let field_ty = match *this.ty.kind() {
            TyKind::Tuple(tys) => tys[i],
            TyKind::Array(elem, _) | TyKind::Slice(elem) => elem,
            TyKind::Adt(def, args) => {
                if tcx.is_lang_item(def.did(), vsc_hir::LangItem::Class) && args.len() == 1 {
                    tcx.mk_ty(TyKind::RawPtr(args[0].expect_ty(), Mutability::Mut))
                } else if tcx.is_lang_item(def.did(), vsc_hir::LangItem::Func) && args.len() == 1 {
                    if i == 0 {
                        tcx.mk_ty(TyKind::RawPtr(tcx.types.u8, Mutability::Not))
                    } else {
                        tcx.mk_ty(TyKind::RawPtr(tcx.types.u8, Mutability::Mut))
                    }
                } else {
                    def.variants()[variant_index].fields[i].ty(tcx, args)
                }
            }
            TyKind::Closure(def_id, _) => {
                let upvars = tcx.closure_upvars(def_id);
                upvars[i]
            }
            TyKind::Coroutine(def_id, args) => {
                let coro = tcx.coroutine_layout(def_id);
                let raw_ty = coro.variant_fields[variant_index][i];
                let parent_args = if args.len() > 1 { &args[..args.len() - 1] } else { &[] };
                tcx.reveal_opaque(tcx.erase_regions(tcx.instantiate(raw_ty, parent_args)))
            }
            TyKind::Ref(_, _, _) | TyKind::RawPtr(_, _) | TyKind::Str => {
                if i == 0 {
                    tcx.mk_ty(TyKind::RawPtr(tcx.types.u8, Mutability::Not))
                } else {
                    tcx.types.usize
                }
            }
            _ => tcx.types.error,
        };

        LayoutCx::new(tcx).layout_of(field_ty).unwrap_or_else(|e| {
            panic!("failed to compute layout of field {i} ({}) on `{}`: {e}", field_ty, this.ty)
        })
    }

    fn ty_and_layout_pointee_info_at(
        this: vsc_abi::TyAndLayout<'tcx, Self>,
        cx: &C,
        offset: Size,
    ) -> Option<PointeeInfo> {
        if offset != Size::ZERO {
            return None;
        }
        let tcx = cx.tcx();
        let (pointee, safe) = match *this.ty.kind() {
            TyKind::Ref(_, inner, Mutability::Not) => (inner, Some(PointerKind::Shared)),
            TyKind::Ref(_, inner, Mutability::Mut) => (inner, Some(PointerKind::Mutable)),
            TyKind::RawPtr(inner, _) => (inner, None),
            TyKind::Adt(def, args)
                if tcx.is_lang_item(def.did(), vsc_hir::LangItem::Class) && args.len() == 1 =>
            {
                (args[0].expect_ty(), None)
            }
            _ => return None,
        };
        let layout = LayoutCx::new(tcx).layout_of(pointee).ok()?;
        if layout.is_sized() {
            Some(PointeeInfo { size: layout.size, align: layout.align, safe })
        } else {
            None
        }
    }

    fn is_adt(this: vsc_abi::TyAndLayout<'tcx, Self>) -> bool {
        matches!(this.ty.kind(), TyKind::Adt(..))
    }

    fn is_never(this: vsc_abi::TyAndLayout<'tcx, Self>) -> bool {
        matches!(this.ty.kind(), TyKind::Never)
    }

    fn is_tuple(this: vsc_abi::TyAndLayout<'tcx, Self>) -> bool {
        matches!(this.ty.kind(), TyKind::Tuple(..))
    }

    fn is_unit(this: vsc_abi::TyAndLayout<'tcx, Self>) -> bool {
        matches!(this.ty.kind(), TyKind::Tuple(tys) if tys.is_empty())
    }
}
