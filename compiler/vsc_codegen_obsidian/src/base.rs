//! CGU compilation and function MIR -> Obsidian IR translation (`vsc_codegen_obsidian::base`).

use obsidian::ir;
use vsc_codegen_ssa::ModuleCodegen;
use vsc_codegen_ssa::mir::PlaceRef;
use vsc_hir::LangItem;
use vsc_hir::def::DefKind;
use vsc_middle::middle::codegen_fn_attrs::Linkage;
use vsc_middle::mir::Body;
use vsc_middle::mono::{CodegenUnit, MonoItem, MonoItemData, Visibility};
use vsc_middle::ty::{EntryFnType, Instance, TyCtxt};
use vsc_target::callconv::PassMode;

use crate::builder::Builder;
use crate::callee::unpack_abi_args_into_places;
use crate::context::{CodegenCx, OwnedObsModule};
use crate::mono_item::codegen_mono_item;
use crate::value::ObsVal;

/// Compiles a complete `CodegenUnit` into an [`OwnedObsModule`].
pub fn compile_codegen_unit<'tcx>(
    tcx: TyCtxt<'tcx>,
    cgu: &CodegenUnit<'tcx>,
) -> ModuleCodegen<OwnedObsModule> {
    let cx = CodegenCx::new(tcx, cgu.name().as_str());
    let items = cgu.items_in_deterministic_order(tcx);

    // Emit statics first so global initializers are established cleanly before function codegen.
    for &(item, ref data) in &items {
        if matches!(item, MonoItem::Static(_)) {
            codegen_mono_item(&cx, item, data);
        }
    }

    for &(item, ref data) in &items {
        if !matches!(item, MonoItem::Static(_)) {
            codegen_mono_item(&cx, item, data);
        }
    }

    if cgu.is_primary() {
        maybe_codegen_entry_wrapper(&cx);
    }

    ModuleCodegen::new_regular(cgu.name().as_str(), cx.into_module())
}

/// Lowers one function `Instance` with an available MIR body into `.obs` IR.
pub fn codegen_fn<'tcx>(
    cx: &CodegenCx<'tcx>,
    instance: Instance<'tcx>,
    data: &MonoItemData,
) {
    let def_id = instance.def_id();
    let has_body = if let Some(local) = def_id.as_local() {
        cx.tcx.body_owned_by(local).is_some()
            || matches!(cx.tcx.def_kind(def_id), DefKind::Closure | DefKind::Coroutine)
    } else {
        false
    };
    let llfn = cx.get_fn(instance);
    if !has_body {
        return;
    }

    let attrs = cx.tcx.codegen_instance_attrs(instance.def);
    let linkage = attrs.linkage.unwrap_or(data.linkage);
    cx.set_linkage_and_visibility(llfn, linkage, data.visibility);

    let body: &Body<'tcx> = cx.tcx.optimized_mir(def_id);
    let fn_abi = cx.fn_abi_of_instance(instance);

    let entry_bb = Builder::append_block(cx, llfn, "start");
    let mut block_bbs = Vec::with_capacity(body.basic_blocks.len());
    for idx in 0..body.basic_blocks.len() {
        block_bbs.push(Builder::append_block(cx, llfn, &format!("bb{idx}")));
    }

    let mut bx = Builder::build(cx, llfn, entry_bb);

    // Allocate stack slots for all MIR locals (`local 0` is the return place).
    let mut locals: Vec<PlaceRef<'tcx, ObsVal>> = Vec::with_capacity(body.local_decls.len());
    for (i, decl) in body.local_decls.iter_enumerated() {
        let mono_ty = instance.instantiate_ty(cx.tcx, decl.ty);
        let layout = cx.layout_of(mono_ty);
        if i.as_usize() == 0 && matches!(fn_abi.ret.mode, PassMode::Indirect { .. }) {
            let sret_ptr = cx.get_param(llfn, 0);
            locals.push(PlaceRef::new_sized(sret_ptr, layout));
        } else {
            let name = format!("_{}", i.as_usize());
            locals.push(bx.alloca_layout(layout, &name));
        }
    }

    // Store incoming parameters into locals `1..=body.arg_count`.
    if body.arg_count > 0 && locals.len() > body.arg_count {
        unpack_abi_args_into_places(&mut bx, fn_abi, &locals[1..=body.arg_count]);
    }

    if !block_bbs.is_empty() {
        bx.br(block_bbs[0]);
    } else {
        bx.ret_void();
    }

    for (bb, bb_data) in body.basic_blocks.iter_enumerated() {
        bx.position_at_end(block_bbs[bb.as_usize()]);
        for stmt in &bb_data.statements {
            bx.codegen_statement(body, &locals, instance, &stmt.kind);
        }
        bx.codegen_terminator(
            body,
            &locals,
            instance,
            fn_abi,
            &block_bbs,
            &bb_data.terminator().kind,
        );
    }
}

/// Emits a standard C `main(i32 %argc, ptr %argv) -> i32` wrapper forwarding to Vertex's entry function.
fn maybe_codegen_entry_wrapper<'tcx>(cx: &CodegenCx<'tcx>) {
    let tcx = cx.tcx;
    if !tcx
        .options
        .effective_crate_types()
        .contains(&vsc_session::config::CrateType::Executable)
    {
        return;
    }
    let Some((entry_def_id, entry_kind)) = tcx.entry_fn(()) else {
        return;
    };

    let entry_instance = Instance::mono(tcx, entry_def_id);
    let entry_sym = tcx.symbol_name(entry_instance).name;
    if entry_sym == "main" {
        return;
    }

    let entry_llfn = cx.get_fn(entry_instance);
    let entry_abi = cx.fn_abi_of_instance(entry_instance);
    let c_main_ty = cx.type_func(&[cx.type_i32(), cx.type_ptr()], cx.type_i32(), false);
    let c_main = cx.declare_raw_fn("main", ir::CallConv::C, c_main_ty);
    cx.set_linkage_and_visibility(c_main, Linkage::External, Visibility::Default);

    let entry_bb = Builder::append_block(cx, c_main, "entry");
    let mut bx = Builder::build(cx, c_main, entry_bb);

    let argc_i32 = cx.get_param(c_main, 0);
    let argv_ptr = cx.get_param(c_main, 1);

    match entry_kind {
        EntryFnType::Start => {
            let entry_fn_ty = crate::abi::fn_abi_obs_type(cx, entry_abi);
            let mut call_args = Vec::new();
            if entry_abi.args.len() >= 2 {
                let argc_isize = bx.sext(argc_i32, cx.type_isize());
                call_args.push(argc_isize);
                call_args.push(argv_ptr);
            }
            let res = bx.call_raw(entry_fn_ty, entry_llfn, &mut call_args);
            if matches!(entry_abi.ret.mode, PassMode::Direct(_)) && !entry_abi.ret.layout.is_zst() {
                bx.ret(bx.intcast(res, cx.type_i32(), true));
            } else {
                bx.ret(cx.const_i32(0));
            }
        }
        EntryFnType::Main { .. } => {
            let is_async_main = entry_def_id.as_local().is_some_and(|local| {
                matches!(
                    tcx.hir_node(local),
                    Some(vsc_middle::hir::map::Node::Item(vsc_hir::Item {
                        kind: vsc_hir::ItemKind::Fn { sig, .. },
                        ..
                    })) if sig.header.is_async
                )
            });

            if is_async_main
                && let Some(block_on_def) = tcx.lang_items().get(LangItem::BlockOn)
            {
                let ret_ty = tcx.fn_sig(entry_def_id).skip_binder().output();
                let fut_ty = tcx.reveal_opaque(tcx.erase_regions(ret_ty));
                let block_on_args = tcx.mk_args(&[vsc_middle::ty::GenericArg::Type(fut_ty)]);
                let block_on_inst = Instance::try_resolve(tcx, block_on_def, block_on_args)
                    .unwrap_or_else(|| Instance::new_raw(block_on_def, block_on_args));
                let block_on_llfn = cx.get_fn(block_on_inst);
                let block_on_abi = cx.fn_abi_of_instance(block_on_inst);

                let fut_place = bx.alloca_layout(entry_abi.ret.layout, "async_main_fut");
                crate::callee::emit_abi_call(&mut bx, entry_abi, entry_llfn, &[], fut_place);
                let fut_op = bx.load_operand(fut_place);

                let out_place = bx.alloca_layout(block_on_abi.ret.layout, "async_main_out");
                crate::callee::emit_abi_call(
                    &mut bx,
                    block_on_abi,
                    block_on_llfn,
                    &[fut_op],
                    out_place,
                );

                let out_ty = block_on_abi.ret.layout.ty;
                let is_int = matches!(
                    out_ty.kind(),
                    vsc_middle::ty::TyKind::Int(_) | vsc_middle::ty::TyKind::Uint(_)
                );
                if is_int && !block_on_abi.ret.layout.is_zst() {
                    let out_op = bx.load_operand(out_place);
                    if let vsc_codegen_ssa::mir::OperandValue::Immediate(v) = out_op.val {
                        bx.ret(bx.intcast(
                            v,
                            cx.type_i32(),
                            crate::builder::ty_is_signed(out_ty),
                        ));
                    } else {
                        bx.ret(cx.const_i32(0));
                    }
                } else {
                    bx.ret(cx.const_i32(0));
                }
                return;
            }
            if let Some(start_def) = tcx.lang_items().get(LangItem::Start)
                && entry_abi.args.is_empty()
            {
                let start_inst = Instance::mono(tcx, start_def);
                let start_abi = cx.fn_abi_of_instance(start_inst);
                if start_abi.args.len() >= 3 {
                    let start_llfn = cx.get_fn(start_inst);
                    let start_ty = crate::abi::fn_abi_obs_type(cx, start_abi);
                    let argc_isize = bx.sext(argc_i32, cx.type_isize());
                    let res = bx.call_raw(
                        start_ty,
                        start_llfn,
                        &mut [entry_llfn, argc_isize, argv_ptr],
                    );
                    if !matches!(start_abi.ret.mode, PassMode::Ignore) {
                        bx.ret(bx.intcast(res, cx.type_i32(), true));
                    } else {
                        bx.ret(cx.const_i32(0));
                    }
                    return;
                }
            }

            let entry_fn_ty = crate::abi::fn_abi_obs_type(cx, entry_abi);
            let res = bx.call_raw(entry_fn_ty, entry_llfn, &mut []);
            if matches!(entry_abi.ret.mode, PassMode::Direct(_)) && !entry_abi.ret.layout.is_zst() {
                let is_int = matches!(
                    entry_abi.ret.layout.ty.kind(),
                    vsc_middle::ty::TyKind::Int(_) | vsc_middle::ty::TyKind::Uint(_)
                );
                if is_int {
                    bx.ret(bx.intcast(
                        res,
                        cx.type_i32(),
                        crate::builder::ty_is_signed(entry_abi.ret.layout.ty),
                    ));
                } else {
                    bx.ret(cx.const_i32(0));
                }
            } else {
                bx.ret(cx.const_i32(0));
            }
        }
    }
}
