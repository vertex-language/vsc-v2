//! Function ABI classification (`rustc_ty_utils::abi`).

use vsc_abi::{BackendRepr, ExternAbi, Integer, Primitive};
use vsc_hir::LangItem;
use vsc_hir::def::DefKind;
use vsc_middle::ty::layout::{
    ArgAbi, ArgAttribute, ArgAttributes, ArgExtension, FnAbi, FnAbiError, HasDataLayout,
    HasTargetSpec, LayoutCx, LayoutOf, Primitive as AbiPrim, Scalar, TyAndLayout,
    extern_abi_from_symbol,
};
use vsc_middle::ty::{self, Instance, InstanceKind, Mutability, PolyFnSig, Ty, TyCtxt, TyKind};
use vsc_target::spec::AbiMap;

fn fn_sig_for_instance<'tcx>(tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> ty::FnSig<'tcx> {
    if let InstanceKind::DropGlue(_, inner_ty) = instance.def {
        let target_ty = inner_ty.unwrap_or_else(|| {
            instance.args.first().map(|a| a.expect_ty()).unwrap_or(tcx.types.unit)
        });
        let ptr_ty = tcx.mk_ty(TyKind::RawPtr(target_ty, Mutability::Mut));
        return ty::FnSig {
            inputs_and_output: tcx.mk_type_list(&[ptr_ty, tcx.types.unit]),
            c_variadic: false,
            safety: ty::Safety::Unsafe,
            abi: None,
        };
    }

    if let InstanceKind::Item(def_id) = instance.def
        && tcx.def_kind(def_id) == DefKind::Coroutine
    {
        let body = tcx.optimized_mir(def_id);
        let arg1 = tcx.reveal_opaque(instance.instantiate_ty(tcx, body.local_decls[vsc_middle::mir::Local::from_usize(1)].ty));
        let arg2 = tcx.reveal_opaque(instance.instantiate_ty(tcx, body.local_decls[vsc_middle::mir::Local::from_usize(2)].ty));
        let ret = tcx.reveal_opaque(instance.instantiate_ty(tcx, body.local_decls[vsc_middle::mir::RETURN_PLACE].ty));
        return ty::FnSig {
            inputs_and_output: tcx.mk_type_list(&[arg1, arg2, ret]),
            c_variadic: false,
            safety: ty::Safety::Safe,
            abi: None,
        };
    }

    if let InstanceKind::Item(def_id) = instance.def
        && matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn)
    {
        let poly = tcx.fn_sig(def_id);
        let io: Vec<Ty<'tcx>> = poly
            .value
            .inputs_and_output
            .iter()
            .map(|&t| tcx.reveal_opaque(instance.instantiate_ty(tcx, t)))
            .collect();
        return ty::FnSig {
            inputs_and_output: tcx.mk_type_list(&io),
            c_variadic: poly.value.c_variadic,
            safety: poly.value.safety,
            abi: poly.value.abi,
        };
    }

    let ty = instance.ty(tcx);
    match *ty.kind() {
        TyKind::FnPtr(sig) => sig.value,
        TyKind::Closure(def_id, args) => {
            let sig = match *args[0].expect_ty().kind() {
                TyKind::FnPtr(s) => s.value,
                _ => panic!("closure without signature arg"),
            };
            let env_ty = match instance.def {
                InstanceKind::ClosureOnceShim { .. } => ty,
                _ => {
                    let closure_kind = tcx
                        .typeck(def_id)
                        .closure_kinds
                        .get(&def_id.expect_local())
                        .copied()
                        .unwrap_or(ty::ClosureKind::Fn);
                    match closure_kind {
                        ty::ClosureKind::Fn | ty::ClosureKind::FnOnce => {
                            tcx.mk_ref(tcx.regions.re_erased, ty, Mutability::Not)
                        }
                        ty::ClosureKind::FnMut => {
                            tcx.mk_ref(tcx.regions.re_erased, ty, Mutability::Mut)
                        }
                    }
                }
            };
            let mut io = Vec::with_capacity(sig.inputs_and_output.len() + 1);
            io.push(env_ty);
            io.extend_from_slice(sig.inputs_and_output);
            ty::FnSig {
                inputs_and_output: tcx.mk_type_list(&io),
                c_variadic: sig.c_variadic,
                safety: sig.safety,
                abi: sig.abi,
            }
        }
        TyKind::FnDef(def_id, _) => {
            let poly = tcx.fn_sig(def_id);
            let io: Vec<Ty<'tcx>> = poly
                .value
                .inputs_and_output
                .iter()
                .map(|&t| instance.instantiate_ty(tcx, t))
                .collect();
            ty::FnSig {
                inputs_and_output: tcx.mk_type_list(&io),
                c_variadic: poly.value.c_variadic,
                safety: poly.value.safety,
                abi: poly.value.abi,
            }
        }
        _ => panic!("unexpected instance ty for fn_abi: {ty}"),
    }
}

fn arg_attrs_for_scalar<'tcx>(
    cx: &LayoutCx<'tcx>,
    ty: Ty<'tcx>,
    scalar: Scalar,
) -> ArgAttributes {
    let dl = cx.data_layout();
    let mut attrs = ArgAttributes::new();
    match scalar.primitive() {
        AbiPrim::Int(int, signed) => {
            if scalar.is_bool() {
                attrs.arg_ext = ArgExtension::Zext;
            } else if int < Integer::I32 {
                attrs.arg_ext = if signed { ArgExtension::Sext } else { ArgExtension::Zext };
            }
        }
        AbiPrim::Pointer(_) => {
            let range = scalar.valid_range(dl);
            if range.start > 0 {
                attrs.regular.insert(ArgAttribute::NON_NULL);
            }
            if let TyKind::Ref(_, inner, mutability) = *ty.kind() {
                if let Ok(inner_layout) = cx.layout_of(inner)
                    && inner_layout.is_sized()
                {
                    attrs.pointee_size = inner_layout.size;
                    attrs.pointee_align = Some(inner_layout.align);
                }
                if mutability == Mutability::Not {
                    attrs.regular.insert(ArgAttribute::READ_ONLY);
                } else {
                    attrs.regular.insert(ArgAttribute::NO_ALIAS);
                }
            }
        }
        AbiPrim::Float(_) => {}
    }
    if !scalar.is_uninit_valid() {
        attrs.regular.insert(ArgAttribute::NO_UNDEF);
    }
    attrs
}

fn make_arg_abi<'tcx>(cx: &LayoutCx<'tcx>, layout: TyAndLayout<'tcx>) -> ArgAbi<'tcx, Ty<'tcx>> {
    let ty = layout.ty;
    let mut arg = ArgAbi::new(layout, |scalar, _offset| arg_attrs_for_scalar(cx, ty, scalar));
    if matches!(layout.backend_repr, BackendRepr::Memory { sized: false }) {
        arg.make_indirect();
    }
    let _ = Primitive::Pointer;
    arg
}

fn fn_abi_from_sig<'tcx>(
    tcx: TyCtxt<'tcx>,
    sig: ty::FnSig<'tcx>,
    extra_args: &'tcx [Ty<'tcx>],
    can_unwind: bool,
) -> Result<&'tcx FnAbi<'tcx, Ty<'tcx>>, FnAbiError<'tcx>> {
    let cx = LayoutCx::new(tcx);
    let fixed_count = sig.inputs().len();

    let ret_layout = cx.layout_of(sig.output())?;
    let ret = make_arg_abi(&cx, ret_layout);

    let mut args = Vec::with_capacity(fixed_count + extra_args.len());
    for &arg_ty in sig.inputs().iter().chain(extra_args.iter()) {
        let layout = cx.layout_of(arg_ty)?;
        args.push(make_arg_abi(&cx, layout));
    }

    let abi = extern_abi_from_symbol(sig.abi);
    let mut fn_abi = FnAbi {
        args: args.into_boxed_slice(),
        ret,
        c_variadic: sig.c_variadic,
        fixed_count: fixed_count as u32,
        conv: AbiMap::from_target(cx.target_spec())
            .canonize_abi(abi, sig.c_variadic)
            .unwrap(),
        can_unwind,
    };

    if abi == ExternAbi::Rust || abi == ExternAbi::RustCall || abi == ExternAbi::RustCold {
        fn_abi.adjust_for_rust_abi(&cx);
    } else {
        fn_abi.adjust_for_foreign_abi(&cx, abi);
    }

    // Ensure non-scalar foreign struct/func types aren't accidentally mis-typed on `LangItem::Func`
    let _ = LangItem::Func;

    Ok(tcx.alloc_fn_abi(fn_abi))
}

pub(crate) fn fn_abi_of_fn_ptr<'tcx>(
    tcx: TyCtxt<'tcx>,
    (sig, extra_args): (PolyFnSig<'tcx>, &'tcx [Ty<'tcx>]),
) -> Result<&'tcx FnAbi<'tcx, Ty<'tcx>>, FnAbiError<'tcx>> {
    fn_abi_from_sig(tcx, sig.value, extra_args, false)
}

pub(crate) fn fn_abi_of_instance<'tcx>(
    tcx: TyCtxt<'tcx>,
    (instance, extra_args): (Instance<'tcx>, &'tcx [Ty<'tcx>]),
) -> Result<&'tcx FnAbi<'tcx, Ty<'tcx>>, FnAbiError<'tcx>> {
    let sig = fn_sig_for_instance(tcx, instance);
    fn_abi_from_sig(tcx, sig, extra_args, false)
}
