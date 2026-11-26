//! MIR place projection, operand loading/storing, and enum discriminant codegen.

use vsc_abi::{BackendRepr, Primitive, TagEncoding, VariantIdx, Variants};
use vsc_codegen_ssa::common::IntPredicate;
use vsc_codegen_ssa::mir::{OperandRef, OperandValue, PlaceRef, PlaceValue, align_for_offset};
use vsc_hir::LangItem;
use vsc_middle::mir::{Body, ConstValue, Operand, Place, PlaceElem};
use vsc_middle::ty::layout::TyAndLayout;
use vsc_middle::ty::{Instance, TyKind};

use super::{Builder, array_or_slice_stride, eval_array_len, type_is_immediate};
use crate::common::discriminant_value;
use crate::consts::codegen_const;
use crate::type_of::{
    layout_is_bool, scalar_obs_type, scalar_pair_element_is_bool, scalar_pair_element_obs_type,
};
use crate::value::ObsVal;

impl<'a, 'tcx> Builder<'a, 'tcx> {
    /// Converts an in-memory scalar (`i8` for `bool`) to its SSA immediate value (`i1` for `bool`).
    pub fn from_immediate(&self, val: ObsVal, layout: TyAndLayout<'tcx>) -> ObsVal {
        if layout_is_bool(layout) {
            self.trunc(val, self.cx.type_i1())
        } else {
            val
        }
    }

    /// Converts an SSA immediate value (`i1` for `bool`) to its in-memory scalar representation (`i8` for `bool`).
    pub fn to_immediate(&self, val: ObsVal, layout: TyAndLayout<'tcx>) -> ObsVal {
        if layout_is_bool(layout) {
            self.zext(val, self.cx.type_i8())
        } else {
            val
        }
    }

    /// Loads an `OperandRef` from a memory `PlaceRef`.
    pub fn load_operand(&self, place: PlaceRef<'tcx, ObsVal>) -> OperandRef<'tcx, ObsVal> {
        let layout = place.layout;
        if layout.is_zst() {
            return OperandRef {
                val: OperandValue::ZeroSized,
                layout,
            };
        }
        if type_is_immediate(layout) {
            let mem_ty = self.cx.layout_obs_type(layout);
            let raw = self.load(mem_ty, place.val.llval, place.val.align);
            let imm = self.from_immediate(raw, layout);
            return OperandRef {
                val: OperandValue::Immediate(imm),
                layout,
            };
        }
        if let BackendRepr::ScalarPair { b_offset, .. } = layout.backend_repr {
            let a_ty = scalar_pair_element_obs_type(self.cx, layout, 0, false);
            let b_ty = scalar_pair_element_obs_type(self.cx, layout, 1, false);
            let mut a_val = self.load(a_ty, place.val.llval, place.val.align);
            if scalar_pair_element_is_bool(self.cx, layout, 0) {
                a_val = self.trunc(a_val, self.cx.type_i1());
            }
            let b_ptr = self.ptradd(place.val.llval, b_offset.bytes());
            let mut b_val = self.load(b_ty, b_ptr, align_for_offset(place.val.align, b_offset));
            if scalar_pair_element_is_bool(self.cx, layout, 1) {
                b_val = self.trunc(b_val, self.cx.type_i1());
            }
            return OperandRef {
                val: OperandValue::Pair(a_val, b_val),
                layout,
            };
        }
        OperandRef {
            val: OperandValue::Ref(place.val),
            layout,
        }
    }

    /// Stores `op` into `dst` in memory.
    pub fn store_operand(&self, op: OperandValue<ObsVal>, dst: PlaceRef<'tcx, ObsVal>) {
        if dst.layout.is_zst() {
            return;
        }
        match op {
            OperandValue::ZeroSized => {}
            OperandValue::Immediate(v) => {
                let v = self.to_immediate(v, dst.layout);
                self.store(v, dst.val.llval, dst.val.align);
            }
            OperandValue::Pair(a, b) => {
                let BackendRepr::ScalarPair { b_offset, .. } = dst.layout.backend_repr else {
                    panic!(
                        "storing Pair into non-ScalarPair layout `{}`",
                        dst.layout.ty
                    );
                };
                let a_mem = if scalar_pair_element_is_bool(self.cx, dst.layout, 0) {
                    self.zext(a, self.cx.type_i8())
                } else {
                    a
                };
                let b_mem = if scalar_pair_element_is_bool(self.cx, dst.layout, 1) {
                    self.zext(b, self.cx.type_i8())
                } else {
                    b
                };
                self.store(a_mem, dst.val.llval, dst.val.align);
                let b_ptr = self.ptradd(dst.val.llval, b_offset.bytes());
                self.store(b_mem, b_ptr, align_for_offset(dst.val.align, b_offset));
            }
            OperandValue::Ref(src) => {
                if src.llval != dst.val.llval {
                    self.memcpy(
                        dst.val.llval,
                        dst.val.align,
                        src.llval,
                        src.align,
                        dst.layout.size.bytes(),
                    );
                }
            }
        }
    }

    /// Lowers a `Place<'tcx>` inside `body` to its memory address and layout.
    pub fn codegen_place(
        &self,
        body: &Body<'tcx>,
        locals: &[PlaceRef<'tcx, ObsVal>],
        instance: Instance<'tcx>,
        place: &Place<'tcx>,
    ) -> PlaceRef<'tcx, ObsVal> {
        let tcx = self.cx.tcx;
        let mut cur = locals[place.local.as_usize()];

        for elem in &place.projection {
            match *elem {
                PlaceElem::Deref => {
                    let pointee_ty = match *cur.layout.ty.kind() {
                        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => inner,
                        TyKind::Adt(def, args) if tcx.is_lang_item(def.did(), LangItem::Class) => {
                            args[0].expect_ty()
                        }
                        other => panic!("cannot deref non-pointer type {other:?}"),
                    };
                    let pointee_layout = self.cx.layout_of(pointee_ty);
                    let ptr_op = self.load_operand(cur);
                    cur = match ptr_op.val {
                        OperandValue::Immediate(p) => PlaceRef::new_sized(p, pointee_layout),
                        OperandValue::Pair(p, meta) => PlaceRef {
                            val: PlaceValue::new_unsized(p, meta, pointee_layout.align),
                            layout: pointee_layout,
                        },
                        _ => panic!("unexpected deref operand value {:?}", ptr_op.val),
                    };
                }
                PlaceElem::Downcast(variant_idx) => {
                    cur.layout = cur
                        .layout
                        .for_variant(&self.cx.layout_cx, VariantIdx::from_usize(variant_idx));
                }
                PlaceElem::Field(field_idx, _) => {
                    let offset = cur.layout.fields.offset(field_idx);
                    let flayout = cur.layout.field(&self.cx.layout_cx, field_idx);
                    let llval = self.ptradd(cur.val.llval, offset.bytes());
                    let align = align_for_offset(cur.val.align, offset);
                    cur = PlaceRef {
                        val: PlaceValue {
                            llval,
                            llextra: cur.val.llextra.filter(|_| flayout.is_unsized()),
                            align,
                        },
                        layout: flayout,
                    };
                }
                PlaceElem::Index(idx_local) => {
                    let (elem_ty, stride) = array_or_slice_stride(self.cx, cur.layout);
                    let elem_layout = self.cx.layout_of(elem_ty);
                    let idx_op = self.load_operand(locals[idx_local.as_usize()]);
                    let OperandValue::Immediate(idx_raw) = idx_op.val else {
                        panic!("non-scalar array index");
                    };
                    let idx = self.intcast(idx_raw, self.cx.type_isize(), false);
                    let byte_off = self.mul(idx, self.cx.const_usize(stride));
                    let llval = self.ptradd_dyn(cur.val.llval, byte_off);
                    cur = PlaceRef {
                        val: PlaceValue::new_sized(llval, elem_layout.align),
                        layout: elem_layout,
                    };
                }
                PlaceElem::ConstantIndex {
                    offset,
                    min_length: _,
                    from_end,
                } => {
                    let (elem_ty, stride) = array_or_slice_stride(self.cx, cur.layout);
                    let elem_layout = self.cx.layout_of(elem_ty);
                    let idx = if !from_end {
                        self.cx.const_usize(offset)
                    } else {
                        let len = self.codegen_place_len(cur);
                        self.sub(len, self.cx.const_usize(offset))
                    };
                    let byte_off = self.mul(idx, self.cx.const_usize(stride));
                    let llval = self.ptradd_dyn(cur.val.llval, byte_off);
                    cur = PlaceRef {
                        val: PlaceValue::new_sized(llval, elem_layout.align),
                        layout: elem_layout,
                    };
                }
                PlaceElem::Subslice { from, to, from_end } => {
                    let (elem_ty, stride) = array_or_slice_stride(self.cx, cur.layout);
                    let start_off = self.cx.const_usize(from.saturating_mul(stride));
                    let new_ptr = self.ptradd_dyn(cur.val.llval, start_off);
                    let slice_ty = tcx.mk_ty(TyKind::Slice(elem_ty));
                    let slice_layout = self.cx.layout_of(slice_ty);
                    let orig_len = self.codegen_place_len(cur);
                    let end = if from_end {
                        self.sub(orig_len, self.cx.const_usize(to))
                    } else {
                        self.cx.const_usize(to)
                    };
                    let new_len = self.sub(end, self.cx.const_usize(from));
                    cur = PlaceRef {
                        val: PlaceValue::new_unsized(new_ptr, new_len, cur.val.align),
                        layout: slice_layout,
                    };
                }
            }
        }

        let _ = (body, instance);
        cur
    }

    pub fn codegen_place_len(&self, place: PlaceRef<'tcx, ObsVal>) -> ObsVal {
        match *place.layout.ty.kind() {
            TyKind::Array(_, c) => self.cx.const_usize(eval_array_len(self.cx.tcx, c)),
            TyKind::Slice(_) | TyKind::Str => place
                .val
                .llextra
                .expect("unsized slice place without metadata"),
            other => panic!("Len of non-array/slice type {other:?}"),
        }
    }

    /// Lowers a MIR `Operand<'tcx>` using monomorphizing substitutions `instance`.
    pub fn codegen_operand(
        &self,
        body: &Body<'tcx>,
        locals: &[PlaceRef<'tcx, ObsVal>],
        instance: Instance<'tcx>,
        operand: &Operand<'tcx>,
    ) -> OperandRef<'tcx, ObsVal> {
        match operand {
            Operand::Copy(p) | Operand::Move(p) => {
                let place = self.codegen_place(body, locals, instance, p);
                self.load_operand(place)
            }
            Operand::Constant(c) => {
                let ty = instance.instantiate_ty(self.cx.tcx, c.ty);
                let val = match c.value {
                    ConstValue::Unevaluated(d, args) => {
                        ConstValue::Unevaluated(d, instance.instantiate_args(self.cx.tcx, args))
                    }
                    ConstValue::Ty(cty) => {
                        ConstValue::Ty(instance.instantiate_const(self.cx.tcx, cty))
                    }
                    other => other,
                };
                codegen_const(self.cx, val, ty)
            }
        }
    }

    /// Emits `StatementKind::SetDiscriminant`.
    pub fn codegen_set_discriminant(&self, place: PlaceRef<'tcx, ObsVal>, variant_index: usize) {
        match &place.layout.variants {
            Variants::Empty => self.unreachable(),
            Variants::Single { .. } => {}
            Variants::Multiple {
                tag,
                tag_encoding,
                tag_field,
                ..
            } => {
                let off = place.layout.fields.offset(tag_field.as_usize());
                let ptr = self.ptradd(place.val.llval, off.bytes());
                let align = align_for_offset(place.val.align, off);
                let tag_ty = scalar_obs_type(self.cx, *tag);
                match tag_encoding {
                    TagEncoding::Direct => {
                        let discr = match *place.layout.ty.kind() {
                            TyKind::Adt(adt, _) => {
                                discriminant_value(self.cx.tcx, adt, variant_index) as u128
                            }
                            TyKind::Coroutine(..) => variant_index as u128,
                            _ => panic!("non-ADT with Variants::Multiple"),
                        };
                        self.store(self.cx.const_uint_big(tag_ty, discr), ptr, align);
                    }
                    TagEncoding::Niche {
                        untagged_variant,
                        niche_variants,
                        niche_start,
                    } => {
                        if VariantIdx::from_usize(variant_index) != *untagged_variant {
                            let rel = (variant_index - niche_variants.start().as_usize()) as u128;
                            let val_u128 = niche_start.wrapping_add(rel);
                            let val_ll = if matches!(tag.primitive(), Primitive::Pointer(_)) {
                                self.inttoptr(self.cx.const_usize(val_u128 as u64), tag_ty)
                            } else {
                                self.cx.const_uint_big(tag_ty, val_u128)
                            };
                            self.store(val_ll, ptr, align);
                        }
                    }
                }
            }
        }
    }

    /// Reads an enum's discriminant into `OperandValue::Immediate`.
    pub fn codegen_discriminant(
        &self,
        place: PlaceRef<'tcx, ObsVal>,
        dst_layout: TyAndLayout<'tcx>,
    ) -> OperandValue<ObsVal> {
        let dst_ty = self.cx.immediate_obs_type(dst_layout);
        let val = match &place.layout.variants {
            Variants::Empty => self.cx.const_undef(dst_ty),
            Variants::Single { index } => {
                let d = match *place.layout.ty.kind() {
                    TyKind::Adt(adt, _) => {
                        discriminant_value(self.cx.tcx, adt, index.as_usize()) as u128
                    }
                    _ => index.as_usize() as u128,
                };
                self.cx.const_uint_big(dst_ty, d)
            }
            Variants::Multiple {
                tag,
                tag_encoding,
                tag_field,
                ..
            } => {
                let off = place.layout.fields.offset(tag_field.as_usize());
                let ptr = self.ptradd(place.val.llval, off.bytes());
                let align = align_for_offset(place.val.align, off);
                let tag_ty = scalar_obs_type(self.cx, *tag);
                let mut raw = self.load(tag_ty, ptr, align);
                let (tag_int_ty, tag_signed) = match tag.primitive() {
                    Primitive::Int(_, signed) => (tag_ty, signed),
                    Primitive::Pointer(_) => {
                        raw = self.ptrtoint(raw, self.cx.type_isize());
                        (self.cx.type_isize(), false)
                    }
                    Primitive::Float(_) => panic!("float enum tag"),
                };
                match tag_encoding {
                    TagEncoding::Direct => self.intcast(raw, dst_ty, tag_signed),
                    TagEncoding::Niche {
                        untagged_variant,
                        niche_variants,
                        niche_start,
                    } => {
                        let discr_of = |v_idx: usize| -> u128 {
                            match *place.layout.ty.kind() {
                                TyKind::Adt(adt, _) => {
                                    discriminant_value(self.cx.tcx, adt, v_idx) as u128
                                }
                                TyKind::Coroutine(..) => v_idx as u128,
                                _ => panic!("non-ADT with niche enum layout"),
                            }
                        };
                        let untagged_discr =
                            self.cx.const_uint_big(dst_ty, discr_of(untagged_variant.as_usize()));
                        let mut res = untagged_discr;
                        for v in niche_variants.start().as_usize()..=niche_variants.end().as_usize()
                        {
                            let rel = (v - niche_variants.start().as_usize()) as u128;
                            let target_niche = self
                                .cx
                                .const_uint_big(tag_int_ty, niche_start.wrapping_add(rel));
                            let eq = self.icmp(IntPredicate::IntEQ, raw, target_niche);
                            let vdiscr = self.cx.const_uint_big(dst_ty, discr_of(v));
                            res = self.select(eq, vdiscr, res);
                        }
                        res
                    }
                }
            }
        };
        OperandValue::Immediate(val)
    }
}
