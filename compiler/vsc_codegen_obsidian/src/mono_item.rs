//! Monomorphized item dispatch, closure-once shims, reify shims, and drop glue (`vsc_codegen_obsidian::mono_item`).

use vsc_abi::{FieldsShape, VariantIdx, Variants};
use vsc_codegen_ssa::mir::{OperandRef, OperandValue, PlaceRef, PlaceValue, align_for_offset};
use vsc_middle::mono::{MonoItem, MonoItemData};
use vsc_middle::ty::{ClosureKind, Instance, InstanceKind, Ty, TyKind};

use crate::builder::{Builder, eval_array_len};
use crate::callee::{codegen_return, emit_abi_call, unpack_abi_args_into_places};
use crate::common::discriminant_value;
use crate::context::CodegenCx;
use crate::value::ObsVal;

/// Codegens one `MonoItem` inside its CGU's `CodegenCx`.
pub fn codegen_mono_item<'tcx>(
    cx: &CodegenCx<'tcx>,
    item: MonoItem<'tcx>,
    data: &MonoItemData,
) {
    match item {
        MonoItem::Static(def_id) => crate::consts::codegen_static(cx, def_id, data),
        MonoItem::GlobalAsm(_) => {
            cx.tcx
                .dcx()
                .fatal("global_asm! is not supported yet by the Obsidian backend");
        }
        MonoItem::Fn(instance) => match instance.def {
            InstanceKind::Item(_) => crate::base::codegen_fn(cx, instance, data),
            InstanceKind::ClosureOnceShim { closure, .. } => {
                codegen_closure_once_shim(cx, instance, closure, data);
            }
            InstanceKind::DropGlue(_, opt_ty) => {
                codegen_drop_glue(cx, instance, opt_ty, data);
            }
            InstanceKind::ReifyShim(def_id) => {
                codegen_reify_shim(cx, instance, def_id, data);
            }
            InstanceKind::Intrinsic(_)
            | InstanceKind::VTableShim(_)
            | InstanceKind::FnPtrShim(..)
            | InstanceKind::Virtual(..) => {}
        },
    }
}

fn codegen_closure_once_shim<'tcx>(
    cx: &CodegenCx<'tcx>,
    instance: Instance<'tcx>,
    closure_def: vsc_span::def_id::DefId,
    data: &MonoItemData,
) {
    let llfn = cx.get_fn(instance);
    cx.set_linkage_and_visibility(llfn, data.linkage, data.visibility);
    let fn_abi = cx.fn_abi_of_instance(instance);

    let bb = Builder::append_block(cx, llfn, "entry");
    let mut bx = Builder::build(cx, llfn, bb);

    let mut arg_places = Vec::with_capacity(fn_abi.args.len());
    for (i, arg) in fn_abi.args.iter().enumerate() {
        arg_places.push(bx.alloca_layout(arg.layout, &format!("once_arg_{i}")));
    }
    unpack_abi_args_into_places(&mut bx, fn_abi, &arg_places);

    let target_inst =
        Instance::resolve_closure(cx.tcx, closure_def, instance.args, ClosureKind::Fn);
    let target_fn = cx.get_fn(target_inst);
    let target_abi = cx.fn_abi_of_instance(target_inst);

    let ref_ty = cx.tcx.mk_ref(
        cx.tcx.regions.re_erased,
        arg_places[0].layout.ty,
        vsc_ast::Mutability::Not,
    );
    let env_ref = OperandRef {
        val: OperandValue::Immediate(arg_places[0].val.llval),
        layout: cx.layout_of(ref_ty),
    };
    let mut call_args = Vec::with_capacity(arg_places.len());
    call_args.push(env_ref);
    for &p in &arg_places[1..] {
        call_args.push(bx.load_operand(p));
    }

    let ret_place = bx.alloca_layout(fn_abi.ret.layout, "once_ret");
    emit_abi_call(&mut bx, target_abi, target_fn, &call_args, ret_place);
    if cx.tcx.needs_drop_raw(arg_places[0].layout.ty) {
        emit_drop_in_place_call(&mut bx, arg_places[0].val.llval, arg_places[0].layout.ty);
    }
    codegen_return(&mut bx, ret_place, fn_abi);
}

fn codegen_reify_shim<'tcx>(
    cx: &CodegenCx<'tcx>,
    instance: Instance<'tcx>,
    def_id: vsc_span::def_id::DefId,
    data: &MonoItemData,
) {
    let llfn = cx.get_fn(instance);
    cx.set_linkage_and_visibility(llfn, data.linkage, data.visibility);
    let fn_abi = cx.fn_abi_of_instance(instance);
    let bb = Builder::append_block(cx, llfn, "entry");
    let mut bx = Builder::build(cx, llfn, bb);

    let mut arg_places = Vec::with_capacity(fn_abi.args.len());
    for (i, a) in fn_abi.args.iter().enumerate() {
        arg_places.push(bx.alloca_layout(a.layout, &format!("reify_arg_{i}")));
    }
    unpack_abi_args_into_places(&mut bx, fn_abi, &arg_places);

    let target_inst = Instance::try_resolve(cx.tcx, def_id, instance.args)
        .unwrap_or_else(|| Instance::new_raw(def_id, instance.args));
    let target_fn = cx.get_fn(target_inst);
    let target_abi = cx.fn_abi_of_instance(target_inst);
    let forward: Vec<_> = arg_places.iter().map(|&p| bx.load_operand(p)).collect();
    let ret_place = bx.alloca_layout(fn_abi.ret.layout, "reify_ret");
    emit_abi_call(&mut bx, target_abi, target_fn, &forward, ret_place);
    codegen_return(&mut bx, ret_place, fn_abi);
}

fn codegen_drop_glue<'tcx>(
    cx: &CodegenCx<'tcx>,
    instance: Instance<'tcx>,
    opt_ty: Option<Ty<'tcx>>,
    data: &MonoItemData,
) {
    let llfn = cx.get_fn(instance);
    cx.set_linkage_and_visibility(llfn, data.linkage, data.visibility);
    let fn_abi = cx.fn_abi_of_instance(instance);
    let bb = Builder::append_block(cx, llfn, "entry");
    let mut bx = Builder::build(cx, llfn, bb);

    let Some(ty) = opt_ty else {
        bx.ret_void();
        return;
    };
    if !cx.tcx.needs_drop_raw(ty) {
        bx.ret_void();
        return;
    }

    let mut ptr_place = vec![bx.alloca_layout(fn_abi.args[0].layout, "drop_ptr_slot")];
    unpack_abi_args_into_places(&mut bx, fn_abi, &ptr_place);
    let OperandValue::Immediate(target_ptr) = bx.load_operand(ptr_place.pop().unwrap()).val else {
        bx.ret_void();
        return;
    };
    let layout = cx.layout_of(ty);
    let target_place = PlaceRef::new_sized(target_ptr, layout);

    match *ty.kind() {
        TyKind::Adt(adt, args) => {
            if let Some(drop_fn_def) = cx.tcx.adt_destructor(adt.did()) {
                let drop_inst = Instance::try_resolve(cx.tcx, drop_fn_def, args)
                    .unwrap_or_else(|| Instance::new_raw(drop_fn_def, args));
                let drop_llfn = cx.get_fn(drop_inst);
                let drop_abi = cx.fn_abi_of_instance(drop_inst);
                let mut_ref =
                    cx.tcx
                        .mk_ref(cx.tcx.regions.re_erased, ty, vsc_ast::Mutability::Mut);
                let self_op = OperandRef {
                    val: OperandValue::Immediate(target_ptr),
                    layout: cx.layout_of(mut_ref),
                };
                let ret_dummy = bx.alloca_layout(drop_abi.ret.layout, "drop_ret");
                emit_abi_call(&mut bx, drop_abi, drop_llfn, &[self_op], ret_dummy);
            }
            if !adt.is_union() {
                if adt.is_enum() && matches!(layout.variants, Variants::Multiple { .. }) {
                    let discr_ty = cx.tcx.types.isize;
                    let discr_layout = cx.layout_of(discr_ty);
                    let OperandValue::Immediate(discr_val) =
                        bx.codegen_discriminant(target_place, discr_layout)
                    else {
                        bx.ret_void();
                        return;
                    };
                    let done_bb = Builder::append_block(cx, llfn, "drop_enum_done");
                    let mut cases = Vec::new();
                    let mut variant_blocks = Vec::new();
                    for v_idx in 0..adt.variants().len() {
                        let vlayout =
                            layout.for_variant(&cx.layout_cx, VariantIdx::from_usize(v_idx));
                        let has_drops = (0..vlayout.fields.count()).any(|f| {
                            cx.tcx.needs_drop_raw(vlayout.field(&cx.layout_cx, f).ty)
                        });
                        if !has_drops {
                            continue;
                        }
                        let vbb = Builder::append_block(cx, llfn, &format!("drop_v{v_idx}"));
                        let discr = discriminant_value(cx.tcx, adt, v_idx) as u64;
                        cases.push((discr, vbb));
                        variant_blocks.push((vbb, vlayout));
                    }
                    bx.switch_int(discr_val, done_bb, &cases);
                    for (vbb, vlayout) in variant_blocks {
                        bx.position_at_end(vbb);
                        drop_fields(&mut bx, target_place, vlayout);
                        bx.br(done_bb);
                    }
                    bx.position_at_end(done_bb);
                } else {
                    drop_fields(&mut bx, target_place, layout);
                }
            }
        }
        TyKind::Tuple(_) | TyKind::Closure(..) => {
            drop_fields(&mut bx, target_place, layout);
        }
        TyKind::Coroutine(def_id, _) => {
            if matches!(layout.variants, Variants::Multiple { .. }) {
                let discr_ty = cx.tcx.types.isize;
                let discr_layout = cx.layout_of(discr_ty);
                let OperandValue::Immediate(discr_val) =
                    bx.codegen_discriminant(target_place, discr_layout)
                else {
                    bx.ret_void();
                    return;
                };
                let coro = cx.tcx.coroutine_layout(def_id);
                let done_bb = Builder::append_block(cx, llfn, "drop_coro_done");
                let mut cases = Vec::new();
                let mut variant_blocks = Vec::new();
                for v_idx in 0..coro.variant_fields.len() {
                    let vlayout = layout.for_variant(&cx.layout_cx, VariantIdx::from_usize(v_idx));
                    let has_drops = (0..vlayout.fields.count()).any(|f| {
                        cx.tcx.needs_drop_raw(vlayout.field(&cx.layout_cx, f).ty)
                    });
                    if !has_drops {
                        continue;
                    }
                    let vbb = Builder::append_block(cx, llfn, &format!("drop_coro_v{v_idx}"));
                    cases.push((v_idx as u64, vbb));
                    variant_blocks.push((vbb, vlayout));
                }
                bx.switch_int(discr_val, done_bb, &cases);
                for (vbb, vlayout) in variant_blocks {
                    bx.position_at_end(vbb);
                    drop_fields(&mut bx, target_place, vlayout);
                    bx.br(done_bb);
                }
                bx.position_at_end(done_bb);
            }
        }
        TyKind::Array(elem_ty, count_const) => {
            if cx.tcx.needs_drop_raw(elem_ty) {
                let len = eval_array_len(cx.tcx, count_const);
                let elem_layout = cx.layout_of(elem_ty);
                let stride = match layout.fields {
                    FieldsShape::Array { stride, .. } => stride.bytes(),
                    _ => elem_layout.size.align_to(elem_layout.align).bytes(),
                };
                for i in 0..len {
                    let elem_ptr = bx.ptradd(target_ptr, i * stride);
                    emit_drop_in_place_call(&mut bx, elem_ptr, elem_ty);
                }
            }
        }
        _ => {}
    }

    bx.ret_void();
}

fn drop_fields<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    base: PlaceRef<'tcx, ObsVal>,
    layout: vsc_middle::ty::layout::TyAndLayout<'tcx>,
) {
    for i in 0..layout.fields.count() {
        let flayout = layout.field(&bx.cx.layout_cx, i);
        if !bx.cx.tcx.needs_drop_raw(flayout.ty) {
            continue;
        }
        let off = layout.fields.offset(i);
        let fptr = bx.ptradd(base.val.llval, off.bytes());
        let _ = PlaceValue::new_sized(fptr, align_for_offset(base.val.align, off));
        emit_drop_in_place_call(bx, fptr, flayout.ty);
    }
}

fn emit_drop_in_place_call<'a, 'tcx>(bx: &mut Builder<'a, 'tcx>, ptr: ObsVal, ty: Ty<'tcx>) {
    let inst = Instance::resolve_drop_in_place(bx.cx.tcx, ty);
    let llfn = bx.cx.get_fn(inst);
    let abi = bx.cx.fn_abi_of_drop_glue(ty);
    let fn_ty = crate::abi::fn_abi_obs_type(bx.cx, abi);
    bx.call_raw(fn_ty, llfn, &mut [ptr]);
}
