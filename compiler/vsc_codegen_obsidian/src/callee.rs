//! Call lowering, function-trait dispatch, `Func` trampolines, and ABI returns (`vsc_codegen_obsidian::callee`).

use obsidian::ir;
use vsc_abi::Align;
use vsc_codegen_ssa::mir::{OperandRef, OperandValue, PlaceRef, PlaceValue, align_for_offset};
use vsc_hir::LangItem;
use vsc_middle::middle::codegen_fn_attrs::Linkage;
use vsc_middle::mono::Visibility;
use vsc_middle::ty::{
    Binder, ClosureKind, FnSig, GenericArgsRef, Instance, PolyFnSig, Ty, TyKind,
};
use vsc_span::def_id::DefId;
use vsc_target::callconv::{FnAbi, PassMode};
use vsc_trait_selection::fn_trait_kind;

use crate::abi::{cast_target_obs_type, fn_abi_obs_type};
use crate::builder::Builder;
use crate::context::CodegenCx;
use crate::type_of::scalar_pair_element_obs_type;
use crate::value::ObsVal;

/// Lowers `TerminatorKind::Return` according to `fn_abi.ret`.
pub fn codegen_return<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    ret_place: PlaceRef<'tcx, ObsVal>,
    fn_abi: &FnAbi<'tcx, Ty<'tcx>>,
) {
    match &fn_abi.ret.mode {
        PassMode::Ignore => bx.ret_void(),
        PassMode::Indirect { .. } => {
            let sret_ptr = bx.cx.get_param(bx.llfn(), 0);
            if ret_place.val.llval != sret_ptr && ret_place.layout.size.bytes() > 0 {
                bx.memcpy(
                    sret_ptr,
                    ret_place.layout.align,
                    ret_place.val.llval,
                    ret_place.val.align,
                    ret_place.layout.size.bytes(),
                );
            }
            bx.ret_void();
        }
        PassMode::Direct(_) => {
            let op = bx.load_operand(ret_place);
            match op.val {
                OperandValue::Immediate(v) => bx.ret(v),
                _ => bx.ret(
                    bx.cx
                        .const_undef(bx.cx.immediate_obs_type(fn_abi.ret.layout)),
                ),
            }
        }
        PassMode::Pair(_, _) => {
            let op = bx.load_operand(ret_place);
            let ret_ty = bx.cx.type_struct(
                &[
                    scalar_pair_element_obs_type(bx.cx, fn_abi.ret.layout, 0, true),
                    scalar_pair_element_obs_type(bx.cx, fn_abi.ret.layout, 1, true),
                ],
                false,
            );
            let OperandValue::Pair(a, b) = op.val else {
                bx.ret(bx.cx.const_undef(ret_ty));
                return;
            };
            let v0 = bx.insert_value(bx.cx.const_undef(ret_ty), a, 0);
            let v1 = bx.insert_value(v0, b, 1);
            bx.ret(v1);
        }
        PassMode::Cast { cast, .. } => {
            let cast_ty = cast_target_obs_type(bx.cx, cast);
            let align = ret_place.layout.align.max(Align::EIGHT);
            let tmp = bx.alloca(cast_ty, align, "cast_ret_tmp");
            if ret_place.layout.size.bytes() > 0 {
                bx.memcpy(
                    tmp,
                    align,
                    ret_place.val.llval,
                    ret_place.val.align,
                    ret_place.layout.size.bytes(),
                );
            }
            let val = bx.load(cast_ty, tmp, align);
            bx.ret(val);
        }
    }
}

/// Lowers any `TerminatorKind::Call` (`FnDef`, `FnPtr`, `Func<F>`).
pub fn codegen_call<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    func_op: OperandRef<'tcx, ObsVal>,
    arg_ops: &[OperandRef<'tcx, ObsVal>],
    dest: PlaceRef<'tcx, ObsVal>,
) {
    let tcx = bx.cx.tcx;
    match *func_op.layout.ty.kind() {
        TyKind::FnDef(def_id, fn_args) => {
            let instance = Instance::try_resolve(tcx, def_id, fn_args)
                .unwrap_or_else(|| Instance::new_raw(def_id, fn_args));

            if let vsc_middle::ty::InstanceKind::Intrinsic(intr_def) = instance.def
                && crate::intrinsic::try_codegen_intrinsic(
                    bx,
                    intr_def,
                    instance.args,
                    arg_ops,
                    dest,
                )
            {
                return;
            }
            if crate::intrinsic::intrinsic_name(bx.cx, instance.def_id()).is_some()
                && crate::intrinsic::try_codegen_intrinsic(
                    bx,
                    instance.def_id(),
                    instance.args,
                    arg_ops,
                    dest,
                )
            {
                return;
            }

            let li = tcx.lang_items();
            if Some(instance.def_id()) == li.get(LangItem::Panic)
                || Some(instance.def_id()) == li.get(LangItem::PanicFmt)
                || Some(instance.def_id()) == li.get(LangItem::AssertFailed)
            {
                bx.call_panic_abort("panicked at runtime");
                return;
            }

            let is_fn_trait = tcx
                .opt_parent(def_id)
                .is_some_and(|p| fn_trait_kind(tcx, p).is_some())
                && arg_ops.len() == 2;
            let effective_args;
            let call_args: &[OperandRef<'tcx, ObsVal>] = if is_fn_trait {
                let mut unpacked = vec![arg_ops[0]];
                unpacked.extend(unpack_tuple_operand(bx, arg_ops[1]));
                effective_args = unpacked;
                &effective_args
            } else {
                arg_ops
            };

            let llfn = bx.cx.get_fn(instance);
            let fn_abi = bx.cx.fn_abi_of_instance(instance);
            emit_abi_call(bx, fn_abi, llfn, call_args, dest);
        }
        TyKind::FnPtr(sig) => {
            let OperandValue::Immediate(fn_ptr) = func_op.val else {
                panic!("FnPtr callee is not immediate: {:?}", func_op.val);
            };
            let fn_abi = bx.cx.fn_abi_of_fn_ptr(sig);
            emit_abi_call(bx, fn_abi, fn_ptr, arg_ops, dest);
        }
        TyKind::Adt(adt, args) if tcx.is_lang_item(adt.did(), LangItem::Func) => {
            codegen_func_value_call(bx, func_op, args[0].expect_ty(), arg_ops, dest);
        }
        other => panic!("unsupported callee type {other:?}"),
    }
}

/// Lowers `Fn::call` / `FnMut::call_mut` / `FnOnce::call_once` when receiver is `Func`, `FnPtr`, `FnDef`, or closure.
pub fn codegen_fn_trait_call<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    mut recv: OperandRef<'tcx, ObsVal>,
    tuple_arg: OperandRef<'tcx, ObsVal>,
    dest: PlaceRef<'tcx, ObsVal>,
) {
    let tcx = bx.cx.tcx;
    let untupled = unpack_tuple_operand(bx, tuple_arg);

    loop {
        match *recv.layout.ty.kind() {
            TyKind::Ref(_, inner, _) => {
                let OperandValue::Immediate(ptr) = recv.val else {
                    break;
                };
                let inner_layout = bx.cx.layout_of(inner);
                recv = bx.load_operand(PlaceRef::new_sized(ptr, inner_layout));
            }
            _ => break,
        }
    }

    match *recv.layout.ty.kind() {
        TyKind::Adt(adt, args) if tcx.is_lang_item(adt.did(), LangItem::Func) => {
            codegen_func_value_call(bx, recv, args[0].expect_ty(), &untupled, dest);
        }
        TyKind::FnPtr(sig) => {
            let OperandValue::Immediate(fn_ptr) = recv.val else {
                panic!("FnPtr receiver missing pointer");
            };
            let fn_abi = bx.cx.fn_abi_of_fn_ptr(sig);
            emit_abi_call(bx, fn_abi, fn_ptr, &untupled, dest);
        }
        TyKind::FnDef(def_id, fn_args) => {
            let inst = Instance::try_resolve(tcx, def_id, fn_args)
                .unwrap_or_else(|| Instance::new_raw(def_id, fn_args));
            let llfn = bx.cx.get_fn(inst);
            let fn_abi = bx.cx.fn_abi_of_instance(inst);
            emit_abi_call(bx, fn_abi, llfn, &untupled, dest);
        }
        TyKind::Closure(cdef, cargs) => {
            let tmp = bx.alloca_layout(recv.layout, "fn_trait_closure");
            bx.store_operand(recv.val, tmp);
            let ref_ty = tcx.mk_ref(
                tcx.regions.re_erased,
                recv.layout.ty,
                vsc_ast::Mutability::Not,
            );
            let ref_op = OperandRef {
                val: OperandValue::Immediate(tmp.val.llval),
                layout: bx.cx.layout_of(ref_ty),
            };
            let mut full_args = vec![ref_op];
            full_args.extend(untupled);
            let inst = Instance::resolve_closure(tcx, cdef, cargs, ClosureKind::Fn);
            let llfn = bx.cx.get_fn(inst);
            let fn_abi = bx.cx.fn_abi_of_instance(inst);
            emit_abi_call(bx, fn_abi, llfn, &full_args, dest);
        }
        other => panic!("Fn trait call on unsupported receiver type {other:?}"),
    }
}

fn codegen_func_value_call<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    func_op: OperandRef<'tcx, ObsVal>,
    inner_fn_ty: Ty<'tcx>,
    args: &[OperandRef<'tcx, ObsVal>],
    dest: PlaceRef<'tcx, ObsVal>,
) {
    let tcx = bx.cx.tcx;
    let OperandValue::Pair(fn_ptr, env_ptr) = func_op.val else {
        panic!("Func value is not ScalarPair: {:?}", func_op.val);
    };
    let TyKind::FnPtr(sig) = *inner_fn_ty.kind() else {
        panic!("Func inner type is not FnPtr: {inner_fn_ty:?}");
    };
    let env_sig = prepend_env_ptr_to_sig(tcx, sig);
    let fn_abi = bx.cx.fn_abi_of_fn_ptr(env_sig);
    let env_ty = env_sig.value.inputs()[0];
    let env_op = OperandRef {
        val: OperandValue::Immediate(env_ptr),
        layout: bx.cx.layout_of(env_ty),
    };
    let mut all_args = Vec::with_capacity(args.len() + 1);
    all_args.push(env_op);
    all_args.extend_from_slice(args);
    emit_abi_call(bx, fn_abi, fn_ptr, &all_args, dest);
}

/// Emits a call instruction conforming to `fn_abi` and writes its result into `dest`.
pub fn emit_abi_call<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    fn_abi: &FnAbi<'tcx, Ty<'tcx>>,
    llcallee: ObsVal,
    args: &[OperandRef<'tcx, ObsVal>],
    dest: PlaceRef<'tcx, ObsVal>,
) {
    let mut llargs = Vec::with_capacity(args.len() + 2);
    let mut sret_tmp: Option<PlaceRef<'tcx, ObsVal>> = None;

    if let PassMode::Indirect { .. } = fn_abi.ret.mode {
        let tmp = bx.alloca_layout(dest.layout, "sret_tmp");
        llargs.push(tmp.val.llval);
        sret_tmp = Some(tmp);
    }

    for (arg_abi, arg_op) in fn_abi.args.iter().zip(args.iter()) {
        match &arg_abi.mode {
            PassMode::Ignore => {}
            PassMode::Direct(_) => {
                let val = operand_to_immediate(bx, *arg_op);
                llargs.push(val);
            }
            PassMode::Pair(_, _) => match arg_op.val {
                OperandValue::Pair(a, b) => {
                    llargs.push(a);
                    llargs.push(b);
                }
                OperandValue::Ref(p) => {
                    let loaded = bx.load_operand(PlaceRef {
                        val: p,
                        layout: arg_op.layout,
                    });
                    let OperandValue::Pair(a, b) = loaded.val else {
                        unreachable!();
                    };
                    llargs.push(a);
                    llargs.push(b);
                }
                _ => panic!("expected Pair for PassMode::Pair"),
            },
            PassMode::Cast {
                pad_i32_count,
                cast,
            } => {
                for _ in 0..*pad_i32_count {
                    llargs.push(bx.cx.const_undef(bx.cx.type_i32()));
                }
                let cast_ty = cast_target_obs_type(bx.cx, cast);
                let align = arg_op.layout.align.max(Align::EIGHT);
                let tmp = bx.alloca(cast_ty, align, "cast_arg_tmp");
                bx.store_operand(arg_op.val, PlaceRef::new_sized(tmp, arg_op.layout));
                let loaded = bx.load(cast_ty, tmp, align);
                match loaded {
                    ObsVal::Pair(a, b) => {
                        llargs.push(ObsVal::Scalar(a));
                        llargs.push(ObsVal::Scalar(b));
                    }
                    other => llargs.push(other),
                }
            }
            PassMode::Indirect { .. } => {
                let ptr = match arg_op.val {
                    OperandValue::Ref(p) => p.llval,
                    _ => {
                        let tmp = bx.alloca_layout(arg_op.layout, "ind_arg_tmp");
                        bx.store_operand(arg_op.val, tmp);
                        tmp.val.llval
                    }
                };
                llargs.push(ptr);
            }
        }
    }

    if fn_abi.c_variadic && args.len() > fn_abi.args.len() {
        for arg_op in &args[fn_abi.args.len()..] {
            match arg_op.val {
                OperandValue::ZeroSized => {}
                OperandValue::Immediate(v) => {
                    // C default argument promotions for variadic arguments:
                    // `f32` -> `f64`; integers narrower than `i32` -> `i32`.
                    let promoted = if v.ty() == crate::value::ObsType::Prim(ir::Type::F32) {
                        bx.float_promote(v)
                    } else if matches!(
                        v.ty(),
                        crate::value::ObsType::Prim(ir::Type::I1 | ir::Type::I8 | ir::Type::I16)
                    ) {
                        bx.intcast(
                            v,
                            bx.cx.type_i32(),
                            crate::builder::ty_is_signed(arg_op.layout.ty),
                        )
                    } else {
                        v
                    };
                    llargs.push(promoted);
                }
                OperandValue::Pair(a, b) => {
                    llargs.push(a);
                    llargs.push(b);
                }
                OperandValue::Ref(p) => {
                    llargs.push(p.llval);
                }
            }
        }
    }

    let fn_ty = fn_abi_obs_type(bx.cx, fn_abi);
    let call_val = bx.call_raw(fn_ty, llcallee, &mut llargs);

    match &fn_abi.ret.mode {
        PassMode::Ignore => {}
        PassMode::Indirect { .. } => {
            if let Some(tmp) = sret_tmp
                && dest.layout.size.bytes() > 0
            {
                bx.memcpy(
                    dest.val.llval,
                    dest.val.align,
                    tmp.val.llval,
                    tmp.val.align,
                    dest.layout.size.bytes(),
                );
            }
        }
        PassMode::Direct(_) => {
            bx.store_operand(OperandValue::Immediate(call_val), dest);
        }
        PassMode::Pair(_, _) => {
            let a = bx.extract_value(call_val, 0);
            let b = bx.extract_value(call_val, 1);
            bx.store_operand(OperandValue::Pair(a, b), dest);
        }
        PassMode::Cast { cast, .. } => {
            let cast_ty = cast_target_obs_type(bx.cx, cast);
            let align = dest.layout.align.max(Align::EIGHT);
            let tmp = bx.alloca(cast_ty, align, "cast_ret_store");
            bx.store(call_val, tmp, align);
            if dest.layout.size.bytes() > 0 {
                bx.memcpy(
                    dest.val.llval,
                    dest.val.align,
                    tmp,
                    align,
                    dest.layout.size.bytes(),
                );
            }
        }
    }
}

/// Loads each argument of `fn_abi` into a slice of operand places starting at `llfn` param 0/1.
pub fn unpack_abi_args_into_places<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    fn_abi: &FnAbi<'tcx, Ty<'tcx>>,
    dst_places: &[PlaceRef<'tcx, ObsVal>],
) {
    let llfn = bx.llfn();
    let mut ll_idx: u32 = match fn_abi.ret.mode {
        PassMode::Indirect { .. } => 1,
        _ => 0,
    };

    for (arg_abi, &dst) in fn_abi.args.iter().zip(dst_places.iter()) {
        match &arg_abi.mode {
            PassMode::Ignore => {}
            PassMode::Direct(_) => {
                let imm_ty = bx.cx.immediate_obs_type(arg_abi.layout);
                let param = match imm_ty {
                    crate::value::ObsType::Pair(_, _) => {
                        let p0 = bx.cx.get_param(llfn, ll_idx);
                        let p1 = bx.cx.get_param(llfn, ll_idx + 1);
                        ll_idx += 2;
                        ObsVal::Pair(p0.as_scalar(), p1.as_scalar())
                    }
                    _ => {
                        let p = bx.cx.get_param(llfn, ll_idx);
                        ll_idx += 1;
                        p
                    }
                };
                bx.store_operand(OperandValue::Immediate(param), dst);
            }
            PassMode::Pair(_, _) => {
                let t0 = scalar_pair_element_obs_type(bx.cx, arg_abi.layout, 0, true);
                let t1 = scalar_pair_element_obs_type(bx.cx, arg_abi.layout, 1, true);
                let p0 = match t0 {
                    crate::value::ObsType::Pair(_, _) => {
                        let a = bx.cx.get_param(llfn, ll_idx);
                        let b = bx.cx.get_param(llfn, ll_idx + 1);
                        ll_idx += 2;
                        ObsVal::Pair(a.as_scalar(), b.as_scalar())
                    }
                    _ => {
                        let a = bx.cx.get_param(llfn, ll_idx);
                        ll_idx += 1;
                        a
                    }
                };
                let p1 = match t1 {
                    crate::value::ObsType::Pair(_, _) => {
                        let a = bx.cx.get_param(llfn, ll_idx);
                        let b = bx.cx.get_param(llfn, ll_idx + 1);
                        ll_idx += 2;
                        ObsVal::Pair(a.as_scalar(), b.as_scalar())
                    }
                    _ => {
                        let a = bx.cx.get_param(llfn, ll_idx);
                        ll_idx += 1;
                        a
                    }
                };
                bx.store_operand(OperandValue::Pair(p0, p1), dst);
            }
            PassMode::Cast {
                pad_i32_count,
                cast,
            } => {
                ll_idx += u32::from(*pad_i32_count);
                let cast_ty = cast_target_obs_type(bx.cx, cast);
                let align = dst.layout.align.max(Align::EIGHT);
                let tmp = bx.alloca(cast_ty, align, "cast_param_tmp");
                match cast_ty {
                    crate::value::ObsType::Pair(_, _) => {
                        let p0 = bx.cx.get_param(llfn, ll_idx);
                        let p1 = bx.cx.get_param(llfn, ll_idx + 1);
                        ll_idx += 2;
                        bx.store(ObsVal::Pair(p0.as_scalar(), p1.as_scalar()), tmp, align);
                    }
                    _ => {
                        let param = bx.cx.get_param(llfn, ll_idx);
                        ll_idx += 1;
                        bx.store(param, tmp, align);
                    }
                }
                if dst.layout.size.bytes() > 0 {
                    bx.memcpy(
                        dst.val.llval,
                        dst.val.align,
                        tmp,
                        align,
                        dst.layout.size.bytes(),
                    );
                }
            }
            PassMode::Indirect { .. } => {
                let param = bx.cx.get_param(llfn, ll_idx);
                ll_idx += 1;
                if dst.layout.size.bytes() > 0 {
                    bx.memcpy(
                        dst.val.llval,
                        dst.val.align,
                        param,
                        dst.layout.align,
                        dst.layout.size.bytes(),
                    );
                }
            }
        }
    }
}

/// Emits or returns an `internal` trampoline `(ptr %_env, Args...) -> Ret` calling a `FnDef`.
pub fn get_or_emit_fn_def_trampoline<'tcx>(
    cx: &CodegenCx<'tcx>,
    def_id: DefId,
    fn_args: GenericArgsRef<'tcx>,
) -> ObsVal {
    if let Some(&tramp) = cx.fn_def_trampolines.borrow().get(&(def_id, fn_args)) {
        return tramp;
    }

    let tcx = cx.tcx;
    let target_inst = Instance::try_resolve(tcx, def_id, fn_args)
        .unwrap_or_else(|| Instance::new_raw(def_id, fn_args));
    let target_fn = cx.get_fn(target_inst);
    let target_abi = cx.fn_abi_of_instance(target_inst);

    let poly_sig = tcx.fn_sig(def_id);
    let inst_io: Vec<_> = poly_sig
        .value
        .inputs_and_output
        .iter()
        .map(|&t| target_inst.instantiate_ty(tcx, t))
        .collect();
    let concrete_sig = Binder::dummy(FnSig {
        inputs_and_output: tcx.mk_type_list(&inst_io),
        c_variadic: poly_sig.value.c_variadic,
        safety: poly_sig.value.safety,
        abi: poly_sig.value.abi,
    });
    let env_sig = prepend_env_ptr_to_sig(tcx, concrete_sig);
    let env_abi = cx.fn_abi_of_fn_ptr(env_sig);

    let name = cx.fresh_const_name("__vsc_fndef_tramp");
    let tramp = cx.declare_fn(&name, env_abi);
    cx.set_linkage_and_visibility(tramp, Linkage::Internal, Visibility::Default);
    cx.fn_def_trampolines
        .borrow_mut()
        .insert((def_id, fn_args), tramp);

    emit_trampoline_body(cx, tramp, env_abi, target_abi, target_fn, false);
    tramp
}

/// Emits or returns an `internal` trampoline `(ptr %fn_ptr, Args...) -> Ret` calling a `FnPtr`.
pub fn get_or_emit_fn_ptr_trampoline<'tcx>(
    cx: &CodegenCx<'tcx>,
    fn_ptr_ty: Ty<'tcx>,
    sig: PolyFnSig<'tcx>,
) -> ObsVal {
    if let Some(&tramp) = cx.fn_ptr_trampolines.borrow().get(&fn_ptr_ty) {
        return tramp;
    }

    let tcx = cx.tcx;
    let target_abi = cx.fn_abi_of_fn_ptr(sig);
    let env_sig = prepend_env_ptr_to_sig(tcx, sig);
    let env_abi = cx.fn_abi_of_fn_ptr(env_sig);

    let name = cx.fresh_const_name("__vsc_fnptr_tramp");
    let tramp = cx.declare_fn(&name, env_abi);
    cx.set_linkage_and_visibility(tramp, Linkage::Internal, Visibility::Default);
    cx.fn_ptr_trampolines.borrow_mut().insert(fn_ptr_ty, tramp);

    emit_trampoline_body(
        cx,
        tramp,
        env_abi,
        target_abi,
        ObsVal::undef(ir::Type::Ptr),
        true,
    );
    tramp
}

fn emit_trampoline_body<'tcx>(
    cx: &CodegenCx<'tcx>,
    tramp: ObsVal,
    env_abi: &FnAbi<'tcx, Ty<'tcx>>,
    target_abi: &FnAbi<'tcx, Ty<'tcx>>,
    static_target: ObsVal,
    target_is_env: bool,
) {
    let bb = Builder::append_block(cx, tramp, "entry");
    let mut bx = Builder::build(cx, tramp, bb);
    let mut arg_places = Vec::with_capacity(env_abi.args.len());
    for (i, a) in env_abi.args.iter().enumerate() {
        arg_places.push(bx.alloca_layout(a.layout, &format!("targ_{i}")));
    }
    unpack_abi_args_into_places(&mut bx, env_abi, &arg_places);

    let callee_val = if target_is_env {
        let OperandValue::Immediate(p) = bx.load_operand(arg_places[0]).val else {
            panic!("FnPtr env ptr missing");
        };
        p
    } else {
        static_target
    };
    let forward_ops: Vec<_> = arg_places[1..]
        .iter()
        .map(|&p| bx.load_operand(p))
        .collect();
    let ret_place = bx.alloca_layout(env_abi.ret.layout, "tramp_ret");
    emit_abi_call(&mut bx, target_abi, callee_val, &forward_ops, ret_place);
    codegen_return(&mut bx, ret_place, env_abi);
}

pub fn prepend_env_ptr_to_sig<'tcx>(
    tcx: vsc_middle::ty::TyCtxt<'tcx>,
    sig: PolyFnSig<'tcx>,
) -> PolyFnSig<'tcx> {
    let env_ptr = tcx.mk_ty(TyKind::RawPtr(tcx.types.u8, vsc_ast::Mutability::Not));
    let mut io = Vec::with_capacity(sig.value.inputs_and_output.len() + 1);
    io.push(env_ptr);
    io.extend_from_slice(sig.value.inputs_and_output);
    Binder::dummy(FnSig {
        inputs_and_output: tcx.mk_type_list(&io),
        c_variadic: sig.value.c_variadic,
        safety: sig.value.safety,
        abi: sig.value.abi,
    })
}

pub fn unpack_tuple_operand<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    tuple: OperandRef<'tcx, ObsVal>,
) -> Vec<OperandRef<'tcx, ObsVal>> {
    let count = tuple.layout.fields.count();
    if count == 0 || tuple.layout.is_zst() {
        return (0..count)
            .map(|i| OperandRef {
                val: OperandValue::ZeroSized,
                layout: tuple.layout.field(&bx.cx.layout_cx, i),
            })
            .collect();
    }
    let tmp = match tuple.val {
        OperandValue::Ref(p) => PlaceRef {
            val: p,
            layout: tuple.layout,
        },
        _ => {
            let alloc = bx.alloca_layout(tuple.layout, "tuple_unpack");
            bx.store_operand(tuple.val, alloc);
            alloc
        }
    };
    (0..count)
        .map(|i| {
            let off = tuple.layout.fields.offset(i);
            let flayout = tuple.layout.field(&bx.cx.layout_cx, i);
            let ptr = bx.ptradd(tmp.val.llval, off.bytes());
            bx.load_operand(PlaceRef {
                val: PlaceValue::new_sized(ptr, align_for_offset(tmp.val.align, off)),
                layout: flayout,
            })
        })
        .collect()
}

fn operand_to_immediate<'a, 'tcx>(
    bx: &Builder<'a, 'tcx>,
    op: OperandRef<'tcx, ObsVal>,
) -> ObsVal {
    match op.val {
        OperandValue::Immediate(v) => v,
        OperandValue::Ref(p) => {
            let loaded = bx.load_operand(PlaceRef {
                val: p,
                layout: op.layout,
            });
            match loaded.val {
                OperandValue::Immediate(v) => v,
                _ => bx.cx.const_undef(bx.cx.immediate_obs_type(op.layout)),
            }
        }
        OperandValue::ZeroSized => bx.cx.const_undef(bx.cx.immediate_obs_type(op.layout)),
        OperandValue::Pair(a, b) => {
            let _ = (a, b);
            panic!(
                "unexpected Pair for PassMode::Direct argument `{}`",
                op.layout.ty
            );
        }
    }
}
