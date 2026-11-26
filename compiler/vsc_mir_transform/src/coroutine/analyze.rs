//! Yield-site discovery and cross-yield liveness analysis for MIR coroutines.

use vsc_data_structures::FxHashSet;
use vsc_hir::LangItem;
use vsc_index::Idx;
use vsc_middle::mir::*;
use vsc_middle::ty::{self, Ty, TyCtxt, TyKind};
use vsc_span::def_id::DefId;

use super::YieldSite;

// ---------------------------------------------------------------------------
// Yield liveness & layout computation
// ---------------------------------------------------------------------------

pub(super) fn analyze_yields<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
    body: &Body<'tcx>,
) -> (Vec<YieldSite>, Vec<Local>, CoroutineLayout<'tcx>) {
    let n_blocks = body.basic_blocks.len();
    let mut use_sets: Vec<FxHashSet<Local>> = vec![FxHashSet::default(); n_blocks];
    let mut def_sets: Vec<FxHashSet<Local>> = vec![FxHashSet::default(); n_blocks];
    let mut addr_taken: FxHashSet<Local> = FxHashSet::default();
    let mut has_self_borrows = false;

    for (bb, data) in body.basic_blocks.iter_enumerated() {
        let i = bb.index();
        for stmt in &data.statements {
            match &stmt.kind {
                StatementKind::Assign(box_pair) => {
                    let (lhs, rv) = &**box_pair;
                    if let Rvalue::Ref(_, p) | Rvalue::RawPtr(_, p) = rv {
                        if p.local.index() >= 3 {
                            addr_taken.insert(p.local);
                        }
                    }
                    collect_rvalue_uses(rv, &def_sets[i], &mut use_sets[i]);
                    if lhs.projection.is_empty() {
                        def_sets[i].insert(lhs.local);
                    } else if !def_sets[i].contains(&lhs.local) {
                        use_sets[i].insert(lhs.local);
                    }
                    collect_place_index_uses(lhs, &def_sets[i], &mut use_sets[i]);
                }
                StatementKind::SetDiscriminant { place, .. } => {
                    if !def_sets[i].contains(&place.local) {
                        use_sets[i].insert(place.local);
                    }
                }
                StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {}
            }
        }
        if let Some(term) = &data.terminator {
            match &term.kind {
                TerminatorKind::SwitchInt { discr, .. } => {
                    collect_operand_uses(discr, &def_sets[i], &mut use_sets[i]);
                }
                TerminatorKind::Call { func, args, destination, .. } => {
                    collect_operand_uses(func, &def_sets[i], &mut use_sets[i]);
                    for a in args {
                        collect_operand_uses(a, &def_sets[i], &mut use_sets[i]);
                    }
                    if destination.projection.is_empty() {
                        def_sets[i].insert(destination.local);
                    } else if !def_sets[i].contains(&destination.local) {
                        use_sets[i].insert(destination.local);
                    }
                    collect_place_index_uses(destination, &def_sets[i], &mut use_sets[i]);
                }
                TerminatorKind::Assert { cond, .. } => {
                    collect_operand_uses(cond, &def_sets[i], &mut use_sets[i]);
                }
                TerminatorKind::Drop { place, .. } => {
                    if !def_sets[i].contains(&place.local) {
                        use_sets[i].insert(place.local);
                    }
                }
                TerminatorKind::InlineAsm { operands, .. } => {
                    for o in operands {
                        collect_operand_uses(o, &def_sets[i], &mut use_sets[i]);
                    }
                }
                TerminatorKind::Yield { value, .. } => {
                    collect_operand_uses(value, &def_sets[i], &mut use_sets[i]);
                }
                TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {}
            }
        }
    }

    // Backward liveness fixpoint: live_in[bb] = use[bb] U (live_out[bb] \ def[bb]).
    let mut live_in: Vec<FxHashSet<Local>> = vec![FxHashSet::default(); n_blocks];
    let mut live_out: Vec<FxHashSet<Local>> = vec![FxHashSet::default(); n_blocks];
    let mut changed = true;
    while changed {
        changed = false;
        for i in (0..n_blocks).rev() {
            let bb = BasicBlock::from_usize(i);
            let mut out = FxHashSet::default();
            if let Some(term) = &body.basic_blocks[bb].terminator {
                for succ in term.kind.successors() {
                    out.extend(live_in[succ.index()].iter().copied());
                }
                // On Yield, `resume_arg` (`_2`) is written on resume, not live across yield.
                if let TerminatorKind::Yield { resume_arg, .. } = &term.kind {
                    if resume_arg.projection.is_empty() {
                        out.remove(&resume_arg.local);
                    }
                }
            }
            let mut inn = use_sets[i].clone();
            for &l in &out {
                if !def_sets[i].contains(&l) {
                    inn.insert(l);
                }
            }
            if inn != live_in[i] || out != live_out[i] {
                live_in[i] = inn;
                live_out[i] = out;
                changed = true;
            }
        }
    }

    // Forward may-be-defined fixpoint: ensures we only save locals that could have been written before yield.
    let mut may_def_in: Vec<FxHashSet<Local>> = vec![FxHashSet::default(); n_blocks];
    let mut may_def_out: Vec<FxHashSet<Local>> = vec![FxHashSet::default(); n_blocks];
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..n_blocks {
            let bb = BasicBlock::from_usize(i);
            let mut out = may_def_in[i].clone();
            out.extend(def_sets[i].iter().copied());
            if out != may_def_out[i] {
                may_def_out[i] = out.clone();
                changed = true;
            }
            if let Some(term) = &body.basic_blocks[bb].terminator {
                for succ in term.kind.successors() {
                    let s = succ.index();
                    let before = may_def_in[s].len();
                    may_def_in[s].extend(may_def_out[i].iter().copied());
                    if may_def_in[s].len() != before {
                        changed = true;
                    }
                }
            }
        }
    }

    // Check if any local live across a yield is a reference (conservative self-borrow marker).
    let upvars = tcx.closure_upvars(def_id).to_vec();
    let mut yields = Vec::new();
    let mut all_saved_set: FxHashSet<Local> = FxHashSet::default();

    for (bb, data) in body.basic_blocks.iter_enumerated() {
        let Some(Terminator { kind: TerminatorKind::Yield { resume, .. }, .. }) = &data.terminator else {
            continue;
        };
        let mut saved: Vec<Local> = live_in[resume.index()]
            .iter()
            .copied()
            .filter(|&l| l.index() >= 3 && may_def_out[bb.index()].contains(&l))
            .collect();
        saved.sort_by_key(|l| l.index());
        for &l in &saved {
            let ty = body.local_decls[l].ty;
            if matches!(ty.kind(), TyKind::Ref(..)) {
                has_self_borrows = true;
            }
            all_saved_set.insert(l);
        }
        // Any address-taken local (e.g. `readBuf` whose raw pointer `&mut readBuf[0]` is live
        // across `.await`) must also live in pinned coroutine storage so pointers never dangle!
        for &l in &addr_taken {
            if l.index() >= 3 && may_def_out[bb.index()].contains(&l) {
                has_self_borrows = true;
                all_saved_set.insert(l);
            }
        }
        yields.push(YieldSite {
            block: bb,
            resume: *resume,
            saved_locals: saved,
        });
    }

    let mut all_saved: Vec<Local> = all_saved_set.into_iter().collect();
    all_saved.sort_by_key(|l| l.index());

    let md_def = tcx.lang_items().get(LangItem::ManuallyDrop);
    let mut variant_fields: Vec<Vec<Ty<'tcx>>> = Vec::with_capacity(3 + yields.len());
    variant_fields.push(upvars); // 0: Unresumed
    variant_fields.push(Vec::new()); // 1: Returned

    if let Some(md_did) = md_def
        && !yields.is_empty()
        && !all_saved.is_empty()
    {
        let md_adt = tcx.adt_def(md_did);
        let wrap_md = |inner_ty: Ty<'tcx>| -> Ty<'tcx> {
            let args = tcx.mk_args(&[ty::GenericArg::Type(inner_ty)]);
            tcx.mk_adt(md_adt, args)
        };
        // Variant 2 (Running / Panicked): holds all saved locals wrapped in `ManuallyDrop<T>`
        // (same layout/offsets as every Suspend variant, with `needs_drop == false`).
        let running_fields: Vec<Ty<'tcx>> = all_saved
            .iter()
            .map(|&l| wrap_md(body.local_decls[l].ty))
            .collect();
        variant_fields.push(running_fields);

        // Variant `3 + k` (Suspend(k)): identical field order/offsets; live locals have type `T`
        // (dropped by `DropGlue` on cancellation), non-live locals have type `ManuallyDrop<T>`.
        for ys in &yields {
            let live_here: FxHashSet<Local> = ys.saved_locals.iter().copied().collect();
            let vfields: Vec<Ty<'tcx>> = all_saved
                .iter()
                .map(|&l| {
                    let ty = body.local_decls[l].ty;
                    if live_here.contains(&l) {
                        ty
                    } else {
                        wrap_md(ty)
                    }
                })
                .collect();
            variant_fields.push(vfields);
        }
    } else {
        variant_fields.push(Vec::new()); // 2: Panicked
        for ys in &yields {
            let fields: Vec<Ty<'tcx>> = ys.saved_locals.iter().map(|&l| body.local_decls[l].ty).collect();
            variant_fields.push(fields);
        }
    }

    (yields, all_saved, CoroutineLayout { variant_fields, has_self_borrows })
}


fn collect_operand_uses(op: &Operand<'_>, killed: &FxHashSet<Local>, uses: &mut FxHashSet<Local>) {
    if let Some(p) = op.place() {
        if !killed.contains(&p.local) {
            uses.insert(p.local);
        }
        collect_place_index_uses(p, killed, uses);
    }
}

fn collect_place_index_uses(p: &Place<'_>, killed: &FxHashSet<Local>, uses: &mut FxHashSet<Local>) {
    for elem in &p.projection {
        if let PlaceElem::Index(idx) = *elem {
            if !killed.contains(&idx) {
                uses.insert(idx);
            }
        }
    }
}

fn collect_rvalue_uses(rv: &Rvalue<'_>, killed: &FxHashSet<Local>, uses: &mut FxHashSet<Local>) {
    match rv {
        Rvalue::Use(op) | Rvalue::Repeat(op, _) | Rvalue::Cast(_, op, _) | Rvalue::UnaryOp(_, op) | Rvalue::ClassNew(op) => {
            collect_operand_uses(op, killed, uses);
        }
        Rvalue::Ref(_, p) | Rvalue::RawPtr(_, p) | Rvalue::Len(p) | Rvalue::Discriminant(p) => {
            if !killed.contains(&p.local) {
                uses.insert(p.local);
            }
            collect_place_index_uses(p, killed, uses);
        }
        Rvalue::BinaryOp(_, pair) | Rvalue::CheckedBinaryOp(_, pair) => {
            collect_operand_uses(&pair.0, killed, uses);
            collect_operand_uses(&pair.1, killed, uses);
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops {
                collect_operand_uses(op, killed, uses);
            }
        }
    }
}

