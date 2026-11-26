//! Translates Vertex types and `TyAndLayout` into [`ObsType`] (`vsc_codegen_obsidian::type_of`).

use vsc_abi::{BackendRepr, Float, Primitive, Scalar};
use vsc_middle::ty::layout::TyAndLayout;
use vsc_middle::ty::{Ty, TyKind};

use crate::context::CodegenCx;
use crate::value::ObsType;

/// Translates `Scalar` into its in-memory [`ObsType`].
pub fn scalar_obs_type<'tcx>(cx: &CodegenCx<'tcx>, scalar: Scalar) -> ObsType {
    primitive_obs_type(cx, scalar.primitive())
}

/// Translates `Primitive` into its in-memory [`ObsType`].
pub fn primitive_obs_type<'tcx>(cx: &CodegenCx<'tcx>, prim: Primitive) -> ObsType {
    match prim {
        Primitive::Int(int, _) => cx.type_ix(int.size().bits()),
        Primitive::Float(f) => match f {
            Float::F16 => cx.type_f16(),
            Float::F32 => cx.type_f32(),
            Float::F64 => cx.type_f64(),
            Float::X87F80 => cx.type_x87_f80(),
            Float::F128 => cx.type_f128(),
        },
        Primitive::Pointer(addr) => cx.type_ptr_in(addr.0),
    }
}

/// Checks whether a scalar layout should use `i1` as its SSA immediate type.
#[inline]
pub fn layout_is_bool<'tcx>(layout: TyAndLayout<'tcx>) -> bool {
    matches!(layout.ty.kind(), TyKind::Bool)
}

/// Checks whether element `index` (0 or 1) of a `ScalarPair` layout represents a `bool`.
pub fn scalar_pair_element_is_bool<'tcx>(
    cx: &CodegenCx<'tcx>,
    layout: TyAndLayout<'tcx>,
    index: usize,
) -> bool {
    if matches!(layout.ty.kind(), TyKind::Tuple(_) | TyKind::Closure(..))
        || matches!(layout.ty.kind(), TyKind::Adt(adt, _) if adt.is_struct())
    {
        let mut seen = 0usize;
        for i in layout.fields.fields_by_offset_order() {
            let f = layout.field(&cx.layout_cx, i);
            if f.is_zst() {
                continue;
            }
            if seen == index {
                return layout_is_bool(f);
            }
            seen += 1;
        }
    }
    false
}

/// Returns the [`ObsType`] for element `index` (0 or 1) of a `ScalarPair` layout.
pub fn scalar_pair_element_obs_type<'tcx>(
    cx: &CodegenCx<'tcx>,
    layout: TyAndLayout<'tcx>,
    index: usize,
    immediate: bool,
) -> ObsType {
    let BackendRepr::ScalarPair { a, b, .. } = layout.backend_repr else {
        panic!(
            "expected ScalarPair layout for {}, got {:?}",
            layout.ty, layout.backend_repr
        );
    };
    let scalar = if index == 0 { a } else { b };
    if immediate && scalar_pair_element_is_bool(cx, layout, index) {
        return cx.type_i1();
    }
    scalar_obs_type(cx, scalar)
}

impl<'tcx> CodegenCx<'tcx> {
    /// Returns the in-memory [`ObsType`] for `ty`.
    pub fn obs_type(&self, ty: Ty<'tcx>) -> ObsType {
        if let Some(&cached) = self.type_lowering.borrow().get(&ty) {
            return cached;
        }
        let layout = self.layout_of(ty);
        let ty_val = self.layout_obs_type(layout);
        self.type_lowering.borrow_mut().insert(ty, ty_val);
        ty_val
    }

    /// Returns the in-memory [`ObsType`] for `layout`.
    pub fn layout_obs_type(&self, layout: TyAndLayout<'tcx>) -> ObsType {
        if let Some(&cached) = self.type_lowering.borrow().get(&layout.ty) {
            return cached;
        }
        let ty_val = compute_layout_obs_type(self, layout);
        self.type_lowering.borrow_mut().insert(layout.ty, ty_val);
        ty_val
    }

    /// Returns the SSA immediate [`ObsType`] for `layout` (`i1` for `bool`).
    pub fn immediate_obs_type(&self, layout: TyAndLayout<'tcx>) -> ObsType {
        if let Some(&cached) = self.scalar_type_lowering.borrow().get(&layout.ty) {
            return cached;
        }
        let ty_val = if layout_is_bool(layout) {
            self.type_i1()
        } else {
            match layout.backend_repr {
                BackendRepr::Scalar(s) => scalar_obs_type(self, s),
                BackendRepr::SimdVector { element, count } => {
                    self.type_vector(scalar_obs_type(self, element), count)
                }
                _ => self.layout_obs_type(layout),
            }
        };
        self.scalar_type_lowering
            .borrow_mut()
            .insert(layout.ty, ty_val);
        ty_val
    }
}

fn compute_layout_obs_type<'tcx>(cx: &CodegenCx<'tcx>, layout: TyAndLayout<'tcx>) -> ObsType {
    if layout.is_zst() {
        return cx.type_struct(&[], false);
    }

    match layout.backend_repr {
        BackendRepr::Scalar(scalar) => scalar_obs_type(cx, scalar),
        BackendRepr::ScalarPair { a, b, .. } => {
            let a_ty = scalar_obs_type(cx, a);
            let b_ty = scalar_obs_type(cx, b);
            cx.type_struct(&[a_ty, b_ty], false)
        }
        BackendRepr::SimdVector { element, count } => {
            cx.type_vector(scalar_obs_type(cx, element), count)
        }
        BackendRepr::Memory { .. } => cx.type_array(cx.type_i8(), layout.size.bytes()),
    }
}
