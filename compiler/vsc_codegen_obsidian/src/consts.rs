//! Constant and static variable Obsidian lowering (`vsc_codegen_obsidian::consts`).

use obsidian::ir;
use vsc_abi::{Endian, Primitive, Scalar, TagEncoding, VariantIdx, Variants};
use vsc_codegen_ssa::mir::{OperandRef, OperandValue};
use vsc_const_eval::interpret::{Interpreter, Location, Output, Value};
use vsc_middle::middle::codegen_fn_attrs::CodegenFnAttrFlags;
use vsc_middle::mir::ConstValue;
use vsc_middle::mono::MonoItemData;
use vsc_middle::ty::layout::TyAndLayout;
use vsc_middle::ty::{ConstKind, GenericArgsRef, Instance, Ty, TyKind};
use vsc_span::def_id::DefId;

use crate::common::discriminant_value;
use crate::context::CodegenCx;
use crate::declare::linkage_to_obs;
use crate::type_of::{layout_is_bool, scalar_obs_type};
use crate::value::ObsVal;

struct NullOutput;

impl Output for NullOutput {
    fn stdout(&mut self, _: &str) {}
    fn stderr(&mut self, _: &str) {}
}

/// Unescapes a string literal token via `vsc_middle::thir::unescape_str`.
pub use vsc_middle::thir::unescape_str;

/// Evaluates `const_val` of type `ty` into an `OperandRef` for use inside MIR instructions.
pub fn codegen_const<'tcx>(
    cx: &CodegenCx<'tcx>,
    const_val: ConstValue<'tcx>,
    ty: Ty<'tcx>,
) -> OperandRef<'tcx, ObsVal> {
    let layout = cx.layout_of(ty);
    match const_val {
        ConstValue::ZeroSized => OperandRef {
            val: OperandValue::ZeroSized,
            layout,
        },
        ConstValue::Scalar(bits) => eval_scalar_bits(cx, bits, layout),
        ConstValue::Str(sym) => {
            let unescaped = vsc_middle::thir::unescape_str(sym.as_str());
            let (ptr, len) = cx.const_bytes_global(unescaped.as_bytes());
            OperandRef {
                val: OperandValue::Pair(ptr, cx.const_usize(len as u64)),
                layout,
            }
        }
        ConstValue::ByteStr(bytes) => {
            let (ptr, len) = cx.const_bytes_global(bytes);
            let val = match layout.backend_repr {
                vsc_abi::BackendRepr::ScalarPair { .. } => {
                    OperandValue::Pair(ptr, cx.const_usize(len as u64))
                }
                _ => OperandValue::Immediate(ptr),
            };
            OperandRef { val, layout }
        }
        ConstValue::StaticRef(def_id) => OperandRef {
            val: OperandValue::Immediate(cx.get_static(def_id)),
            layout,
        },
        ConstValue::Ty(c) => match *c.kind() {
            ConstKind::Value(vty, bits) => eval_scalar_bits(cx, bits, cx.layout_of(vty)),
            ConstKind::Unevaluated(def_id, args) => {
                eval_const_item_to_operand(cx, def_id, args, layout)
            }
            ConstKind::Param(_) | ConstKind::Error => OperandRef {
                val: undef_operand_value(cx, layout),
                layout,
            },
        },
        ConstValue::Unevaluated(def_id, args) => {
            eval_const_item_to_operand(cx, def_id, args, layout)
        }
    }
}

/// Evaluates a constant `u128` bit pattern for `layout` (when scalar or ZST).
pub fn eval_scalar_bits<'tcx>(
    cx: &CodegenCx<'tcx>,
    bits: u128,
    layout: TyAndLayout<'tcx>,
) -> OperandRef<'tcx, ObsVal> {
    if layout.is_zst() {
        return OperandRef {
            val: OperandValue::ZeroSized,
            layout,
        };
    }
    let val = match layout.backend_repr {
        vsc_abi::BackendRepr::Scalar(scalar) => OperandValue::Immediate(
            scalar_bits_to_obs_imm(cx, bits, scalar, layout_is_bool(layout)),
        ),
        _ => {
            let n = layout.size.bytes().max(1) as usize;
            let mut bytes = vec![0u8; n];
            write_uint_bytes(cx.tcx.data_layout.endian, &mut bytes, bits);
            let alloc = ConstAlloc {
                bytes,
                relocs: Vec::new(),
            };
            let g = emit_anon_const_global(cx, &alloc, layout.align.bytes().max(1));
            OperandValue::Ref(vsc_codegen_ssa::mir::PlaceValue::new_sized(g, layout.align))
        }
    };
    OperandRef { val, layout }
}

fn scalar_bits_to_obs_imm<'tcx>(
    cx: &CodegenCx<'tcx>,
    bits: u128,
    scalar: Scalar,
    is_bool: bool,
) -> ObsVal {
    match scalar.primitive() {
        Primitive::Int(int, _) => {
            if is_bool {
                cx.const_bool(bits != 0)
            } else {
                cx.const_uint_big(cx.type_ix(int.size().bits()), bits)
            }
        }
        Primitive::Float(f) => match f {
            vsc_abi::Float::F16 => cx.const_real(cx.type_f16(), (bits as u16) as f64),
            vsc_abi::Float::F32 => {
                cx.const_real(cx.type_f32(), f32::from_bits(bits as u32) as f64)
            }
            vsc_abi::Float::F64 => cx.const_real(cx.type_f64(), f64::from_bits(bits as u64)),
            vsc_abi::Float::X87F80 | vsc_abi::Float::F128 => {
                cx.const_null(scalar_obs_type(cx, scalar))
            }
        },
        Primitive::Pointer(addr) => {
            let ptr_ty = cx.type_ptr_in(addr.0);
            if bits == 0 {
                cx.const_null(ptr_ty)
            } else {
                ObsVal::const_bits(ir::Type::Ptr, bits as u64)
            }
        }
    }
}

fn undef_operand_value<'tcx>(
    cx: &CodegenCx<'tcx>,
    layout: TyAndLayout<'tcx>,
) -> OperandValue<ObsVal> {
    if layout.is_zst() {
        return OperandValue::ZeroSized;
    }
    match layout.backend_repr {
        vsc_abi::BackendRepr::Scalar(_) => {
            OperandValue::Immediate(cx.const_undef(cx.immediate_obs_type(layout)))
        }
        vsc_abi::BackendRepr::ScalarPair { .. } => OperandValue::Pair(
            cx.const_undef(crate::type_of::scalar_pair_element_obs_type(
                cx, layout, 0, true,
            )),
            cx.const_undef(crate::type_of::scalar_pair_element_obs_type(
                cx, layout, 1, true,
            )),
        ),
        _ => {
            let alloc = ConstAlloc {
                bytes: vec![0u8; layout.size.bytes().max(1) as usize],
                relocs: Vec::new(),
            };
            let g = emit_anon_const_global(cx, &alloc, layout.align.bytes().max(1));
            OperandValue::Ref(vsc_codegen_ssa::mir::PlaceValue::new_sized(g, layout.align))
        }
    }
}

/// Evaluates `ConstValue::Unevaluated(def_id, args)` via `vsc_const_eval` and converts it to [`OperandRef`].
pub fn eval_const_item_to_operand<'tcx>(
    cx: &CodegenCx<'tcx>,
    def_id: DefId,
    args: GenericArgsRef<'tcx>,
    layout: TyAndLayout<'tcx>,
) -> OperandRef<'tcx, ObsVal> {
    let mut out = NullOutput;
    let mut interp = Interpreter::new(cx.tcx, &mut out);
    let body = cx.tcx.optimized_mir(def_id);
    let val = match interp.call_body(body, args, Vec::new(), Vec::new()) {
        Ok(v) => v,
        Err(err) => {
            cx.tcx.dcx().fatal(format!(
                "failed to evaluate constant `{}`: {err:?}",
                cx.tcx.def_path_str(def_id)
            ));
        }
    };
    interp_value_to_operand(cx, &interp, &val, layout)
}

/// Emits a `static` item (`MonoItem::Static`) with its evaluated constant initializer.
pub fn codegen_static<'tcx>(cx: &CodegenCx<'tcx>, def_id: DefId, data: &MonoItemData) {
    let attrs = cx.tcx.codegen_fn_attrs(def_id);
    let is_foreign = cx
        .tcx
        .opt_parent(def_id)
        .is_some_and(|p| cx.tcx.def_kind(p) == vsc_hir::def::DefKind::ForeignMod);
    let g = cx.get_static(def_id);
    if is_foreign {
        return;
    }

    let ty = cx.tcx.type_of(def_id);
    let layout = cx.layout_of(ty);
    let mut out = NullOutput;
    let mut interp = Interpreter::new(cx.tcx, &mut out);
    let body = cx.tcx.optimized_mir(def_id);
    let val = match interp.call_body(body, &[], Vec::new(), Vec::new()) {
        Ok(v) => v,
        Err(err) => {
            cx.tcx.dcx().fatal(format!(
                "failed to evaluate static `{}`: {err:?}",
                cx.tcx.def_path_str(def_id)
            ));
        }
    };

    let alloc = interp_value_to_alloc(cx, &interp, &val, layout);
    let mutable = matches!(
        cx.tcx.def_kind(def_id),
        vsc_hir::def::DefKind::Static { mutability } if mutability.is_mut()
    );
    let linkage = linkage_to_obs(attrs.linkage.unwrap_or(data.linkage));
    let (sym, _, _) = g.as_sym().expect("static ObsVal must be SymAddr");
    let align = layout.align.bytes().max(1) as u32;
    if attrs.flags.contains(CodegenFnAttrFlags::THREAD_LOCAL) {
        cx.module
            .borrow_mut()
            .define_tls(sym, &alloc.bytes, align, &alloc.relocs, linkage);
    } else {
        // Statics containing pointer/function relocations (such as vtables) or
        // interior mutability (`UnsafeCell` / atomics) must be placed in writable
        // data (`__DATA`) rather than read-only `__TEXT`/`__const`.
        let readonly = !mutable
            && alloc.relocs.is_empty()
            && !has_interior_mutability(cx.tcx, layout.ty);
        cx.module.borrow_mut().define_static(
            sym,
            &alloc.bytes,
            align,
            &alloc.relocs,
            linkage,
            readonly,
        );
    }
}

fn has_interior_mutability<'tcx>(tcx: vsc_middle::ty::TyCtxt<'tcx>, ty: Ty<'tcx>) -> bool {
    let mut visited = vsc_data_structures::FxHashSet::default();
    fn visit<'tcx>(
        tcx: vsc_middle::ty::TyCtxt<'tcx>,
        ty: Ty<'tcx>,
        visited: &mut vsc_data_structures::FxHashSet<Ty<'tcx>>,
    ) -> bool {
        let ty = tcx.erase_regions(ty);
        if !visited.insert(ty) {
            return false;
        }
        match *ty.kind() {
            TyKind::Adt(adt, args) => {
                if tcx.is_lang_item(adt.did(), vsc_hir::LangItem::UnsafeCell) {
                    return true;
                }
                adt.variants().iter().any(|v| {
                    v.fields.iter().any(|f| visit(tcx, f.ty(tcx, args), visited))
                })
            }
            TyKind::Tuple(elems) => elems.iter().any(|&e| visit(tcx, e, visited)),
            TyKind::Array(elem, _) | TyKind::Slice(elem) => visit(tcx, elem, visited),
            _ => false,
        }
    }
    visit(tcx, ty, &mut visited)
}

struct ConstAlloc {
    bytes: Vec<u8>,
    relocs: Vec<ir::DataReloc>,
}

fn interp_value_to_operand<'tcx>(
    cx: &CodegenCx<'tcx>,
    interp: &Interpreter<'_, 'tcx>,
    val: &Value<'tcx>,
    layout: TyAndLayout<'tcx>,
) -> OperandRef<'tcx, ObsVal> {
    if layout.is_zst() {
        return OperandRef {
            val: OperandValue::ZeroSized,
            layout,
        };
    }
    if let vsc_abi::BackendRepr::Scalar(scalar) = layout.backend_repr
        && let Some(imm) = interp_scalar_to_obs(cx, interp, val, scalar, layout)
    {
        return OperandRef {
            val: OperandValue::Immediate(imm),
            layout,
        };
    }
    if let vsc_abi::BackendRepr::ScalarPair { .. } = layout.backend_repr
        && let Value::SlicePtr { array, start, len } = val
    {
        let pointee_elem = match *layout.ty.kind() {
            TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => match *inner.kind() {
                TyKind::Str => Some(cx.tcx.types.u8),
                TyKind::Slice(e) => Some(e),
                _ => None,
            },
            _ => None,
        };
        if let Some(elem_ty) = pointee_elem {
            let ptr = emit_slice_ptr_global(cx, interp, array, *start, *len, elem_ty);
            return OperandRef {
                val: OperandValue::Pair(ptr, cx.const_usize(*len as u64)),
                layout,
            };
        }
    }

    let alloc = interp_value_to_alloc(cx, interp, val, layout);
    let g = emit_anon_const_global(cx, &alloc, layout.align.bytes().max(1));
    OperandRef {
        val: OperandValue::Ref(vsc_codegen_ssa::mir::PlaceValue::new_sized(g, layout.align)),
        layout,
    }
}

fn interp_scalar_to_obs<'tcx>(
    cx: &CodegenCx<'tcx>,
    interp: &Interpreter<'_, 'tcx>,
    val: &Value<'tcx>,
    scalar: Scalar,
    layout: TyAndLayout<'tcx>,
) -> Option<ObsVal> {
    match val {
        Value::Bool(b) => Some(cx.const_bool(*b)),
        Value::Char(c) => Some(cx.const_u32(*c as u32)),
        Value::Int(i) => Some(scalar_bits_to_obs_imm(
            cx,
            i.bits,
            scalar,
            layout_is_bool(layout),
        )),
        Value::F32(f) => Some(cx.const_real(cx.type_f32(), *f as f64)),
        Value::F64(f) => Some(cx.const_real(cx.type_f64(), *f)),
        Value::FnDef(def, args) => {
            if matches!(scalar.primitive(), Primitive::Pointer(_)) {
                let inst = Instance::try_resolve(cx.tcx, *def, args)
                    .unwrap_or_else(|| Instance::new_raw(*def, args));
                Some(cx.get_fn(inst))
            } else {
                None
            }
        }
        Value::Ptr(ptr) => {
            let ptee_ty = match *layout.ty.kind() {
                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => inner,
                _ => return None,
            };
            let target_val = interp
                .read(&Location {
                    alloc: ptr.alloc,
                    path: ptr.path.clone(),
                    slice: None,
                })
                .ok()?;
            let ptee_layout = cx.layout_of(ptee_ty);
            let alloc = interp_value_to_alloc(cx, interp, &target_val, ptee_layout);
            Some(emit_anon_const_global(
                cx,
                &alloc,
                ptee_layout.align.bytes().max(1),
            ))
        }
        _ => None,
    }
}

fn interp_value_to_alloc<'tcx>(
    cx: &CodegenCx<'tcx>,
    interp: &Interpreter<'_, 'tcx>,
    val: &Value<'tcx>,
    layout: TyAndLayout<'tcx>,
) -> ConstAlloc {
    if layout.is_zst() {
        return ConstAlloc {
            bytes: vec![0u8; 1],
            relocs: Vec::new(),
        };
    }
    let mut alloc = ConstAlloc {
        bytes: vec![0u8; layout.size.bytes().max(1) as usize],
        relocs: Vec::new(),
    };
    write_interp_value(cx, interp, &mut alloc, 0, val, layout);
    alloc
}

fn push_reloc(alloc: &mut ConstAlloc, offset: u64, target: ObsVal) {
    if let Some((sym, _, addend)) = target.as_sym() {
        alloc.relocs.push(ir::DataReloc {
            offset: offset as u32,
            sym,
            addend,
        });
    }
}

fn write_interp_value<'tcx>(
    cx: &CodegenCx<'tcx>,
    interp: &Interpreter<'_, 'tcx>,
    alloc: &mut ConstAlloc,
    offset: u64,
    val: &Value<'tcx>,
    layout: TyAndLayout<'tcx>,
) {
    if layout.is_zst() {
        return;
    }
    let endian = cx.tcx.data_layout.endian;
    let size = layout.size.bytes() as usize;
    match val {
        Value::Uninit => {}
        Value::Bool(b) => {
            if let Some(slot) = alloc.bytes.get_mut(offset as usize) {
                *slot = u8::from(*b);
            }
        }
        Value::Char(c) => write_uint_bytes(
            endian,
            &mut alloc.bytes[offset as usize..offset as usize + size],
            *c as u32 as u128,
        ),
        Value::Int(i) => write_uint_bytes(
            endian,
            &mut alloc.bytes[offset as usize..offset as usize + size],
            i.bits,
        ),
        Value::F32(f) => write_uint_bytes(
            endian,
            &mut alloc.bytes[offset as usize..offset as usize + 4],
            f.to_bits() as u128,
        ),
        Value::F64(f) => write_uint_bytes(
            endian,
            &mut alloc.bytes[offset as usize..offset as usize + 8],
            f.to_bits() as u128,
        ),
        Value::FnDef(def, args) => {
            if size > 0 {
                let inst = Instance::try_resolve(cx.tcx, *def, args)
                    .unwrap_or_else(|| Instance::new_raw(*def, args));
                push_reloc(alloc, offset, cx.get_fn(inst));
            }
        }
        Value::Ptr(ptr) => {
            let ptee_ty = match *layout.ty.kind() {
                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => inner,
                _ => return,
            };
            if let Ok(target_val) = interp.read(&Location {
                alloc: ptr.alloc,
                path: ptr.path.clone(),
                slice: None,
            }) {
                let ptee_layout = cx.layout_of(ptee_ty);
                let inner_alloc = interp_value_to_alloc(cx, interp, &target_val, ptee_layout);
                let g = emit_anon_const_global(cx, &inner_alloc, ptee_layout.align.bytes().max(1));
                push_reloc(alloc, offset, g);
            }
        }
        Value::SlicePtr { array, start, len } => {
            let elem_ty = match *layout.ty.kind() {
                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => match *inner.kind() {
                    TyKind::Str => cx.tcx.types.u8,
                    TyKind::Slice(e) => e,
                    _ => return,
                },
                _ => return,
            };
            let ptr = emit_slice_ptr_global(cx, interp, array, *start, *len, elem_ty);
            let ptr_bytes = (cx.tcx.target.pointer_width / 8) as usize;
            push_reloc(alloc, offset, ptr);
            write_uint_bytes(
                endian,
                &mut alloc.bytes[offset as usize + ptr_bytes..offset as usize + size],
                *len as u128,
            );
        }
        Value::Tuple(fields) | Value::Array(fields) => {
            for (i, f) in fields.iter().enumerate() {
                let flayout = layout.field(&cx.layout_cx, i);
                let foff = layout.fields.offset(i).bytes();
                write_interp_value(cx, interp, alloc, offset + foff, f, flayout);
            }
        }
        Value::Closure { captures, .. } => {
            for (i, f) in captures.iter().enumerate() {
                let flayout = layout.field(&cx.layout_cx, i);
                let foff = layout.fields.offset(i).bytes();
                write_interp_value(cx, interp, alloc, offset + foff, f, flayout);
            }
        }
        Value::Adt { variant, fields } => {
            let vlayout = layout.for_variant(&cx.layout_cx, VariantIdx::from_usize(*variant));
            for (i, f) in fields.iter().enumerate() {
                if matches!(f, Value::Uninit) {
                    continue;
                }
                let flayout = vlayout.field(&cx.layout_cx, i);
                let foff = vlayout.fields.offset(i).bytes();
                write_interp_value(cx, interp, alloc, offset + foff, f, flayout);
            }
            if let Variants::Multiple {
                tag,
                tag_encoding,
                tag_field,
                ..
            } = &layout.variants
            {
                let tag_off = layout.fields.offset(tag_field.as_usize()).bytes();
                let tag_bytes = tag.size(&cx.tcx.data_layout).bytes() as usize;
                let tag_bits: Option<u128> = match tag_encoding {
                    TagEncoding::Direct => match *layout.ty.kind() {
                        TyKind::Adt(adt, _) => {
                            Some(discriminant_value(cx.tcx, adt, *variant) as u128)
                        }
                        _ => None,
                    },
                    TagEncoding::Niche {
                        untagged_variant,
                        niche_variants,
                        niche_start,
                    } => {
                        let vidx = VariantIdx::from_usize(*variant);
                        if vidx == *untagged_variant {
                            None
                        } else {
                            let rel = (*variant - niche_variants.start().as_usize()) as u128;
                            Some(niche_start.wrapping_add(rel))
                        }
                    }
                };
                if let Some(bits) = tag_bits {
                    let start = offset as usize + tag_off as usize;
                    write_uint_bytes(endian, &mut alloc.bytes[start..start + tag_bytes], bits);
                }
            }
        }
        _ => {}
    }
}

fn emit_slice_ptr_global<'tcx>(
    cx: &CodegenCx<'tcx>,
    interp: &Interpreter<'_, 'tcx>,
    array: &vsc_const_eval::interpret::Pointer,
    start: usize,
    len: usize,
    elem_ty: Ty<'tcx>,
) -> ObsVal {
    let arr_val = interp.read(&Location {
        alloc: array.alloc,
        path: array.path.clone(),
        slice: None,
    });
    let Ok(Value::Array(elems)) = arr_val else {
        return cx.zst_env_ptr();
    };
    let slice_elems = elems.get(start..start + len).unwrap_or(&[]);
    if elem_ty == cx.tcx.types.u8 {
        let bytes: Vec<u8> = slice_elems
            .iter()
            .map(|v| match v {
                Value::Int(i) => i.bits as u8,
                _ => 0,
            })
            .collect();
        return cx.const_bytes_global(&bytes).0;
    }
    let elem_layout = cx.layout_of(elem_ty);
    let stride = elem_layout.size.align_to(elem_layout.align).bytes() as usize;
    let total_bytes = (stride * len).max(1);
    let mut alloc = ConstAlloc {
        bytes: vec![0u8; total_bytes],
        relocs: Vec::new(),
    };
    for (i, ev) in slice_elems.iter().enumerate() {
        write_interp_value(cx, interp, &mut alloc, (i * stride) as u64, ev, elem_layout);
    }
    emit_anon_const_global(cx, &alloc, elem_layout.align.bytes().max(1))
}

fn emit_anon_const_global<'tcx>(cx: &CodegenCx<'tcx>, alloc: &ConstAlloc, align: u64) -> ObsVal {
    let name = cx.fresh_const_name("__vsc_const");
    let sym = cx.module.borrow_mut().add_const_with_relocs(
        &name,
        &alloc.bytes,
        align.max(1) as u32,
        &alloc.relocs,
    );
    ObsVal::sym_addr(sym, true)
}

fn write_uint_bytes(endian: Endian, dst: &mut [u8], val: u128) {
    let raw = match endian {
        Endian::Little => val.to_le_bytes(),
        Endian::Big => val.to_be_bytes(),
    };
    match endian {
        Endian::Little => {
            let n = dst.len().min(16);
            dst[..n].copy_from_slice(&raw[..n]);
        }
        Endian::Big => {
            let n = dst.len().min(16);
            dst[..n].copy_from_slice(&raw[16 - n..]);
        }
    }
}
