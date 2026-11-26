//! Reachability-based monomorphization collector (`rustc_monomorphize::collector`).

use std::collections::VecDeque;

use vsc_data_structures::FxHashSet;
use vsc_hir::LangItem;
use vsc_hir::def::DefKind;
use vsc_middle::middle::codegen_fn_attrs::CodegenFnAttrFlags;
use vsc_middle::mir::*;
use vsc_middle::mono::MonoItem;
use vsc_middle::ty::{AdtKind, Instance, InstanceKind, Ty, TyCtxt, TyKind};
use vsc_mir_dataflow::place_ty;
use vsc_session::config::CrateType;

/// Collects every reachable [`MonoItem`] starting from the crate entry point,
/// exported / `@used` symbols, and (when building libraries) public items.
pub fn collect_crate_mono_items<'tcx>(tcx: TyCtxt<'tcx>) -> FxHashSet<MonoItem<'tcx>> {
    let mut visited = FxHashSet::default();
    let mut worklist = VecDeque::new();

    if let Some((entry_def, _)) = tcx.entry_fn(()) {
        enqueue(tcx, MonoItem::Fn(Instance::mono(tcx, entry_def)), &mut visited, &mut worklist);
        if let Some(local) = entry_def.as_local()
            && matches!(
                tcx.hir_node(local),
                Some(vsc_middle::hir::map::Node::Item(vsc_hir::Item {
                    kind: vsc_hir::ItemKind::Fn { sig, .. },
                    ..
                })) if sig.header.is_async
            )
            && let Some(block_on_def) = tcx.lang_items().get(LangItem::BlockOn)
        {
            let ret_ty = tcx.fn_sig(entry_def).skip_binder().output();
            let fut_ty = tcx.reveal_opaque(tcx.erase_regions(ret_ty));
            let args = tcx.mk_args(&[vsc_middle::ty::GenericArg::Type(fut_ty)]);
            let inst = Instance::try_resolve(tcx, block_on_def, args)
                .unwrap_or_else(|| Instance::new_raw(block_on_def, args));
            enqueue(tcx, MonoItem::Fn(inst), &mut visited, &mut worklist);
        }
    }

    let is_library = tcx.options.crate_types.iter().any(|ct| {
        matches!(ct, CrateType::Rlib | CrateType::Dylib | CrateType::Cdylib | CrateType::Staticlib)
    });

    for (local, info) in tcx.defs.iter_enumerated() {
        let def_id = local.to_def_id();
        let attrs = tcx.codegen_fn_attrs(def_id);
        if attrs.flags.contains(CodegenFnAttrFlags::FOREIGN_ITEM) {
            continue;
        }
        let is_exported = attrs.contains_extern_indicator() || attrs.used();
        let is_pub_non_generic = is_library && matches!(info.kind, DefKind::Fn | DefKind::AssocFn);

        if is_exported || is_pub_non_generic {
            match info.kind {
                DefKind::Fn | DefKind::AssocFn => {
                    if !InstanceKind::Item(def_id).requires_monomorphization(tcx) {
                        enqueue(
                            tcx,
                            MonoItem::Fn(Instance::mono(tcx, def_id)),
                            &mut visited,
                            &mut worklist,
                        );
                    }
                }
                DefKind::Static { .. } => {
                    if is_exported {
                        enqueue(tcx, MonoItem::Static(def_id), &mut visited, &mut worklist);
                    }
                }
                _ => {}
            }
        }
    }

    while let Some(item) = worklist.pop_front() {
        match item {
            MonoItem::Fn(instance) => {
                collect_instance(tcx, instance, &mut visited, &mut worklist);
            }
            MonoItem::Static(def_id) => {
                if let Some(local) = def_id.as_local()
                    && tcx.body_owned_by(local).is_some()
                {
                    collect_body(
                        tcx,
                        Instance::mono(tcx, def_id),
                        tcx.optimized_mir(def_id),
                        &mut visited,
                        &mut worklist,
                    );
                }
            }
            MonoItem::GlobalAsm(..) => {}
        }
    }

    visited
}

fn should_skip_fn(tcx: TyCtxt<'_>, inst: Instance<'_>) -> bool {
    match inst.def {
        InstanceKind::Intrinsic(_) | InstanceKind::Virtual(..) => true,
        InstanceKind::DropGlue(_, None) => true,
        InstanceKind::Item(def_id) => tcx
            .codegen_fn_attrs(def_id)
            .flags
            .contains(CodegenFnAttrFlags::FOREIGN_ITEM),
        _ => false,
    }
}

fn enqueue<'tcx>(
    tcx: TyCtxt<'tcx>,
    item: MonoItem<'tcx>,
    visited: &mut FxHashSet<MonoItem<'tcx>>,
    worklist: &mut VecDeque<MonoItem<'tcx>>,
) {
    if let MonoItem::Fn(inst) = item
        && should_skip_fn(tcx, inst)
    {
        return;
    }
    if let MonoItem::Static(def_id) = item
        && tcx.codegen_fn_attrs(def_id).flags.contains(CodegenFnAttrFlags::FOREIGN_ITEM)
    {
        return;
    }
    if visited.insert(item) {
        worklist.push_back(item);
    }
}

fn collect_instance<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    visited: &mut FxHashSet<MonoItem<'tcx>>,
    worklist: &mut VecDeque<MonoItem<'tcx>>,
) {
    match instance.def {
        InstanceKind::DropGlue(_, Some(ty)) => {
            collect_drop_glue(tcx, ty, visited, worklist);
        }
        InstanceKind::ClosureOnceShim { closure, .. } => {
            let closure_inst = Instance::new_raw(closure, instance.args);
            enqueue(tcx, MonoItem::Fn(closure_inst), visited, worklist);
        }
        InstanceKind::Item(def_id)
        | InstanceKind::FnPtrShim(def_id, _)
        | InstanceKind::ReifyShim(def_id)
        | InstanceKind::VTableShim(def_id) => {
            if let Some(local) = def_id.as_local()
                && (tcx.body_owned_by(local).is_some()
                    || matches!(tcx.defs[local].kind, DefKind::Closure | DefKind::Coroutine))
            {
                let body = tcx.optimized_mir(def_id);
                collect_body(tcx, instance, body, visited, worklist);
            }
        }
        InstanceKind::DropGlue(_, None)
        | InstanceKind::Intrinsic(_)
        | InstanceKind::Virtual(..) => {}
    }
}

fn collect_drop_glue<'tcx>(
    tcx: TyCtxt<'tcx>,
    ty: Ty<'tcx>,
    visited: &mut FxHashSet<MonoItem<'tcx>>,
    worklist: &mut VecDeque<MonoItem<'tcx>>,
) {
    match *ty.kind() {
        TyKind::Adt(adt, args) => {
            if tcx.is_lang_item(adt.did(), LangItem::ManuallyDrop)
                || tcx.is_lang_item(adt.did(), LangItem::PhantomData)
                || adt.kind() == AdtKind::Union
            {
                return;
            }
            if let Some(dtor) = adt.destructor(tcx) {
                enqueue(tcx, MonoItem::Fn(Instance::new_raw(dtor, args)), visited, worklist);
            }
            for variant in adt.variants() {
                for field in &variant.fields {
                    let fty = field.ty(tcx, args);
                    if tcx.needs_drop_raw(fty) {
                        enqueue(
                            tcx,
                            MonoItem::Fn(Instance::resolve_drop_in_place(tcx, fty)),
                            visited,
                            worklist,
                        );
                    }
                }
            }
        }
        TyKind::Tuple(elems) => {
            for &e in elems {
                if tcx.needs_drop_raw(e) {
                    enqueue(
                        tcx,
                        MonoItem::Fn(Instance::resolve_drop_in_place(tcx, e)),
                        visited,
                        worklist,
                    );
                }
            }
        }
        TyKind::Array(elem, _) | TyKind::Slice(elem) => {
            if tcx.needs_drop_raw(elem) {
                enqueue(
                    tcx,
                    MonoItem::Fn(Instance::resolve_drop_in_place(tcx, elem)),
                    visited,
                    worklist,
                );
            }
        }
        TyKind::Closure(def_id, args) => {
            for &upvar in tcx.closure_upvars(def_id) {
                let uty = tcx.erase_regions(tcx.instantiate(upvar, args));
                if tcx.needs_drop_raw(uty) {
                    enqueue(
                        tcx,
                        MonoItem::Fn(Instance::resolve_drop_in_place(tcx, uty)),
                        visited,
                        worklist,
                    );
                }
            }
        }
        TyKind::Coroutine(def_id, args) => {
            let parent_args = if args.len() > 1 { &args[..args.len() - 1] } else { &[] };
            let coro = tcx.coroutine_layout(def_id);
            for vfields in &coro.variant_fields {
                for &raw_ty in vfields {
                    let fty = tcx.reveal_opaque(tcx.erase_regions(tcx.instantiate(raw_ty, parent_args)));
                    if tcx.needs_drop_raw(fty) {
                        enqueue(
                            tcx,
                            MonoItem::Fn(Instance::resolve_drop_in_place(tcx, fty)),
                            visited,
                            worklist,
                        );
                    }
                }
            }
        }
        TyKind::Opaque(..) => {
            let revealed = tcx.reveal_opaque(ty);
            if revealed != ty {
                collect_drop_glue(tcx, revealed, visited, worklist);
            }
        }
        _ => {}
    }
}

fn collect_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    body: &Body<'tcx>,
    visited: &mut FxHashSet<MonoItem<'tcx>>,
    worklist: &mut VecDeque<MonoItem<'tcx>>,
) {
    for data in &body.basic_blocks {
        for stmt in &data.statements {
            if let StatementKind::Assign(box_pair) = &stmt.kind {
                let (_, rv) = &**box_pair;
                visit_rvalue(tcx, instance, rv, visited, worklist);
            }
        }
        let term = data.terminator();
        match &term.kind {
            TerminatorKind::Call { func, args, .. } => {
                visit_operand(tcx, instance, func, visited, worklist);
                let raw_func_ty = match func {
                    Operand::Copy(p) | Operand::Move(p) => place_ty(tcx, body, p),
                    Operand::Constant(c) => c.ty,
                };
                let mono_func_ty = instance.instantiate_ty(tcx, raw_func_ty);
                if let TyKind::FnDef(callee_def, callee_args) = *mono_func_ty.kind() {
                    if let Some(callee_inst) = Instance::try_resolve(tcx, callee_def, callee_args) {
                        enqueue(tcx, MonoItem::Fn(callee_inst), visited, worklist);
                    }
                } else if let TyKind::Closure(c_def, c_args) = *mono_func_ty.kind() {
                    enqueue(
                        tcx,
                        MonoItem::Fn(Instance::new_raw(c_def, c_args)),
                        visited,
                        worklist,
                    );
                }
                for a in args {
                    visit_operand(tcx, instance, a, visited, worklist);
                }
            }
            TerminatorKind::Drop { place, .. } => {
                let raw_ty = place_ty(tcx, body, place);
                let mono_ty = tcx.reveal_opaque(instance.instantiate_ty(tcx, raw_ty));
                if tcx.needs_drop_raw(mono_ty) {
                    let glue = Instance::resolve_drop_in_place(tcx, mono_ty);
                    enqueue(tcx, MonoItem::Fn(glue), visited, worklist);
                }
            }
            TerminatorKind::SwitchInt { discr, .. } | TerminatorKind::Assert { cond: discr, .. } => {
                visit_operand(tcx, instance, discr, visited, worklist);
            }
            TerminatorKind::InlineAsm { operands, .. } => {
                for op in operands {
                    visit_operand(tcx, instance, op, visited, worklist);
                }
            }
            TerminatorKind::Yield { value, .. } => {
                visit_operand(tcx, instance, value, visited, worklist);
            }
            TerminatorKind::Goto { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => {}
        }
    }
}

fn visit_rvalue<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    rv: &Rvalue<'tcx>,
    visited: &mut FxHashSet<MonoItem<'tcx>>,
    worklist: &mut VecDeque<MonoItem<'tcx>>,
) {
    match rv {
        Rvalue::Use(op)
        | Rvalue::Repeat(op, _)
        | Rvalue::UnaryOp(_, op)
        | Rvalue::ClassNew(op) => visit_operand(tcx, instance, op, visited, worklist),
        Rvalue::Cast(_, op, _) => {
            visit_operand(tcx, instance, op, visited, worklist);
        }
        Rvalue::BinaryOp(_, ops) | Rvalue::CheckedBinaryOp(_, ops) => {
            visit_operand(tcx, instance, &ops.0, visited, worklist);
            visit_operand(tcx, instance, &ops.1, visited, worklist);
        }
        Rvalue::Aggregate(kind, ops) => {
            if let AggregateKind::Closure(c_def, c_args) = &**kind {
                let mono_args = instance.instantiate_args(tcx, c_args);
                enqueue(tcx, MonoItem::Fn(Instance::new_raw(*c_def, mono_args)), visited, worklist);
            } else if let AggregateKind::Coroutine(c_def, c_args) = &**kind {
                let mono_args = instance.instantiate_args(tcx, c_args);
                let parent_args = if mono_args.len() > 1 { &mono_args[..mono_args.len() - 1] } else { &[] };
                enqueue(tcx, MonoItem::Fn(Instance::new_raw(*c_def, tcx.mk_args(parent_args))), visited, worklist);
            }
            for op in ops {
                visit_operand(tcx, instance, op, visited, worklist);
            }
        }
        Rvalue::Ref(..) | Rvalue::RawPtr(..) | Rvalue::Len(_) | Rvalue::Discriminant(_) => {}
    }
}

fn visit_operand<'tcx>(
    tcx: TyCtxt<'tcx>,
    instance: Instance<'tcx>,
    op: &Operand<'tcx>,
    visited: &mut FxHashSet<MonoItem<'tcx>>,
    worklist: &mut VecDeque<MonoItem<'tcx>>,
) {
    let Operand::Constant(c) = op else {
        return;
    };
    let mono_ty = instance.instantiate_ty(tcx, c.ty);
    if let TyKind::FnDef(def_id, args) = *mono_ty.kind()
        && let Some(resolved) = Instance::try_resolve(tcx, def_id, args)
    {
        enqueue(tcx, MonoItem::Fn(resolved), visited, worklist);
    }
    if let ConstValue::StaticRef(static_def) = c.value {
        enqueue(tcx, MonoItem::Static(static_def), visited, worklist);
    }
}
