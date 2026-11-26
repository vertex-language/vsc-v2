//! Drop elaboration (`rustc_mir_transform::elaborate_drops`):
//! inserts [`TerminatorKind::Drop`] before overwrites and function exits when
//! [`TyCtxt::needs_drop_raw`] reports non-trivial drop glue, using dynamic bool
//! drop flags only for conditionally initialized move paths.

use vsc_data_structures::FxHashMap;
use vsc_index::{DenseBitSet, Idx};
use vsc_middle::mir::*;
use vsc_middle::ty::{AdtKind, Mutability, TyCtxt, TyKind};
use vsc_mir_dataflow::framework::{Location, ResultsCursor, iterate_to_fixpoint};
use vsc_mir_dataflow::impls::{MaybeInitializedPlaces, MaybeUninitializedPlaces};
use vsc_mir_dataflow::move_paths::{LookupResult, MoveData, MovePathIndex, has_deref};
use vsc_mir_dataflow::place_ty;
use vsc_span::Span;
use vsc_trait_selection::ParamEnv;

#[derive(Clone, Debug)]
struct PendingDrop<'tcx> {
    place: Place<'tcx>,
    mp: MovePathIndex,
    needs_flag: bool,
    span: Span,
}

#[derive(Clone, Debug)]
enum InsertPoint {
    BeforeStatement { block: BasicBlock, stmt_index: usize },
    BeforeTerminator { block: BasicBlock },
}

pub fn run<'tcx>(tcx: TyCtxt<'tcx>, param_env: &ParamEnv<'tcx>, body: &mut Body<'tcx>) {
    if body.basic_blocks.is_empty() {
        return;
    }

    let move_data = MoveData::gather_moves(body, tcx, param_env);
    let maybe_init = iterate_to_fixpoint(MaybeInitializedPlaces::new(&move_data), body);
    let maybe_uninit = iterate_to_fixpoint(MaybeUninitializedPlaces::new(&move_data), body);

    let mut init_cursor = ResultsCursor::new(&maybe_init, body);
    let mut uninit_cursor = ResultsCursor::new(&maybe_uninit, body);

    let mut insertions: Vec<(InsertPoint, Vec<PendingDrop<'tcx>>)> = Vec::new();
    let mut flagged_paths: FxHashMap<MovePathIndex, Local> = FxHashMap::default();

    for (bb, data) in body.basic_blocks.iter_enumerated() {
        if !maybe_init.reachable.contains(bb) {
            continue;
        }
        // Check overwrites of places with existing initialized values.
        for (stmt_idx, stmt) in data.statements.iter().enumerate() {
            let loc = Location::new(bb, stmt_idx);
            if let StatementKind::Assign(box_pair) = &stmt.kind {
                let (lhs, rvalue) = &**box_pair;
                if has_deref(lhs) || lhs.local == RETURN_PLACE || rvalue_mentions_local(rvalue, lhs.local) {
                    continue;
                }
                init_cursor.seek_before_primary_effect(loc);
                uninit_cursor.seek_before_primary_effect(loc);
                let mut init_after_moves = init_cursor.get().clone();
                let mut uninit_after_moves = uninit_cursor.get().clone();
                apply_moves_at(&move_data, loc, &mut init_after_moves, &mut uninit_after_moves);

                if let Some(root_mp) = lookup_mp(&move_data, lhs) {
                    let mut drops = Vec::new();
                    collect_drops_for_path(
                        tcx,
                        body,
                        &move_data,
                        root_mp,
                        &init_after_moves,
                        &uninit_after_moves,
                        stmt.span,
                        &mut drops,
                    );
                    if !drops.is_empty() {
                        for d in &drops {
                            if d.needs_flag {
                                flagged_paths.entry(d.mp).or_insert(RETURN_PLACE);
                            }
                        }
                        insertions.push((
                            InsertPoint::BeforeStatement { block: bb, stmt_index: stmt_idx },
                            drops,
                        ));
                    }
                }
            }
        }

        // Before `Return`, drop all initialized non-return locals in reverse order.
        let term = data.terminator();
        if matches!(term.kind, TerminatorKind::Return) {
            let term_loc = Location::new(bb, data.statements.len());
            init_cursor.seek_before_primary_effect(term_loc);
            uninit_cursor.seek_before_primary_effect(term_loc);
            let init_set = init_cursor.get().clone();
            let uninit_set = uninit_cursor.get().clone();

            let mut drops = Vec::new();
            for l_idx in (1..body.local_decls.len()).rev() {
                let l = Local::from_usize(l_idx);
                let root_mp = move_data.local_paths[l];
                collect_drops_for_path(
                    tcx,
                    body,
                    &move_data,
                    root_mp,
                    &init_set,
                    &uninit_set,
                    term.span,
                    &mut drops,
                );
            }
            if !drops.is_empty() {
                for d in &drops {
                    if d.needs_flag {
                        flagged_paths.entry(d.mp).or_insert(RETURN_PLACE);
                    }
                }
                insertions.push((InsertPoint::BeforeTerminator { block: bb }, drops));
            }
        }
    }

    if insertions.is_empty() {
        return;
    }

    // Allocate bool drop-flag locals for paths needing runtime drop flags.
    let mut flag_keys: Vec<MovePathIndex> = flagged_paths.keys().copied().collect();
    flag_keys.sort_by_key(|mp| mp.index());
    for mp in &flag_keys {
        let l = body.local_decls.push(LocalDecl {
            mutability: Mutability::Mut,
            ty: tcx.types.bool,
            span: body.span,
            kind: LocalKind::Temp,
        });
        flagged_paths.insert(*mp, l);
    }

    // Emit drop-flag updates before applying block-splitting insertions.
    if !flag_keys.is_empty() {
        emit_flag_updates(tcx, &move_data, &flagged_paths, &flag_keys, body);
    }

    // Apply insertions in reverse block / reverse statement order so earlier statement
    // indices in the same block remain valid.
    insertions.sort_by(|a, b| insert_point_ord(&b.0).cmp(&insert_point_ord(&a.0)));

    for (point, drops) in insertions {
        match point {
            InsertPoint::BeforeStatement { block, stmt_index } => {
                apply_drops_before_statement(block, stmt_index, &drops, &flagged_paths, body);
            }
            InsertPoint::BeforeTerminator { block } => {
                let stmt_index = body.basic_blocks[block].statements.len();
                apply_drops_before_terminator(block, stmt_index, &drops, &flagged_paths, body);
            }
        }
    }
}

fn insert_point_ord(p: &InsertPoint) -> (usize, usize) {
    match p {
        InsertPoint::BeforeTerminator { block } => (block.index(), usize::MAX),
        InsertPoint::BeforeStatement { block, stmt_index } => (block.index(), *stmt_index),
    }
}

fn lookup_mp(move_data: &MoveData<'_>, place: &Place<'_>) -> Option<MovePathIndex> {
    match move_data.lookup(place) {
        LookupResult::Exact(mp) => Some(mp),
        LookupResult::Parent(mp) => mp,
    }
}

fn apply_moves_at(
    move_data: &MoveData<'_>,
    loc: Location,
    init: &mut DenseBitSet<MovePathIndex>,
    uninit: &mut DenseBitSet<MovePathIndex>,
) {
    if let Some(moves) = move_data.loc_map.get(&loc) {
        for &mo in moves {
            let mp = move_data.moves[mo].path;
            move_data.for_each_child_inclusive(mp, |child| {
                init.remove(child);
                uninit.insert(child);
            });
        }
    }
}

fn collect_drops_for_path<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    move_data: &MoveData<'tcx>,
    mp: MovePathIndex,
    may_init: &DenseBitSet<MovePathIndex>,
    may_uninit: &DenseBitSet<MovePathIndex>,
    span: Span,
    out: &mut Vec<PendingDrop<'tcx>>,
) {
    if !may_init.contains(mp) {
        return;
    }
    let place = &move_data.move_paths[mp].place;
    let ty = place_ty(tcx, body, place);
    if !tcx.needs_drop_raw(ty) {
        return;
    }

    let has_dtor = match *ty.kind() {
        TyKind::Adt(adt, _) => adt.destructor(tcx).is_some() || adt.kind() == AdtKind::Enum,
        _ => false,
    };

    if !may_uninit.contains(mp) {
        out.push(PendingDrop { place: place.clone(), mp, needs_flag: false, span });
        return;
    }

    // Conditionally/partially initialized path: if aggregate without custom Drop and children
    // track finer fields, descend into children first.
    if !has_dtor && let Some(first_child) = move_data.move_paths[mp].first_child {
        let mut any_child_distinct = false;
        let mut child = Some(first_child);
        while let Some(c) = child {
            if may_init.contains(c) != may_init.contains(mp)
                || may_uninit.contains(c) != may_uninit.contains(mp)
            {
                any_child_distinct = true;
                break;
            }
            child = move_data.move_paths[c].next_sibling;
        }
        if any_child_distinct {
            let mut c = Some(first_child);
            while let Some(ch) = c {
                collect_drops_for_path(tcx, body, move_data, ch, may_init, may_uninit, span, out);
                c = move_data.move_paths[ch].next_sibling;
            }
            return;
        }
    }

    out.push(PendingDrop { place: place.clone(), mp, needs_flag: true, span });
}

fn rvalue_mentions_local(rv: &Rvalue<'_>, local: Local) -> bool {
    let op_has = |op: &Operand<'_>| op.place().is_some_and(|p| p.local == local);
    match rv {
        Rvalue::Use(op)
        | Rvalue::Repeat(op, _)
        | Rvalue::Cast(_, op, _)
        | Rvalue::UnaryOp(_, op)
        | Rvalue::ClassNew(op) => op_has(op),
        Rvalue::Ref(_, p) | Rvalue::RawPtr(_, p) | Rvalue::Len(p) | Rvalue::Discriminant(p) => {
            p.local == local
        }
        Rvalue::BinaryOp(_, ops) | Rvalue::CheckedBinaryOp(_, ops) => {
            op_has(&ops.0) || op_has(&ops.1)
        }
        Rvalue::Aggregate(_, ops) => ops.iter().any(op_has),
    }
}

fn emit_flag_updates<'tcx>(
    tcx: TyCtxt<'tcx>,
    move_data: &MoveData<'tcx>,
    flagged_paths: &FxHashMap<MovePathIndex, Local>,
    flag_keys: &[MovePathIndex],
    body: &mut Body<'tcx>,
) {
    let span = body.span;
    let bool_const = |b: bool| {
        Operand::Constant(Box::new(ConstOperand {
            span,
            ty: tcx.types.bool,
            value: ConstValue::Scalar(b as u128),
        }))
    };

    // Initialize drop flags at START_BLOCK.
    let mut start_updates = Vec::new();
    for &mp in flag_keys {
        let flag_local = flagged_paths[&mp];
        let place = &move_data.move_paths[mp].place;
        let initial = place.local.index() >= 1 && place.local.index() <= body.arg_count;
        start_updates.push(Statement {
            span,
            kind: StatementKind::Assign(Box::new((
                Place::from_local(flag_local),
                Rvalue::Use(bool_const(initial)),
            ))),
        });
    }

    let emit_clear_flags = |mp: MovePathIndex, sspan: Span, out: &mut Vec<Statement<'tcx>>| {
        let mut clear = |p: MovePathIndex| {
            if let Some(&fl) = flagged_paths.get(&p) {
                out.push(Statement {
                    span: sspan,
                    kind: StatementKind::Assign(Box::new((
                        Place::from_local(fl),
                        Rvalue::Use(Operand::Constant(Box::new(ConstOperand {
                            span: sspan,
                            ty: tcx.types.bool,
                            value: ConstValue::Scalar(0),
                        }))),
                    ))),
                });
            }
        };
        move_data.for_each_child_inclusive(mp, &mut clear);
        // Moving out of a variant/field of a whole-tracked path (e.g. `Option.Some(w)`)
        // deinitializes the parent move path whose drop flag guards the aggregate.
        let mut anc = move_data.move_paths[mp].parent;
        while let Some(p) = anc {
            clear(p);
            anc = move_data.move_paths[p].parent;
        }
    };

    let num_blocks = body.basic_blocks.len();
    let mut target_inits: Vec<(BasicBlock, Local, Span)> = Vec::new();
    for bb_idx in 0..num_blocks {
        let bb = BasicBlock::from_usize(bb_idx);
        let orig_stmts = std::mem::take(&mut body.basic_blocks[bb].statements);
        let stmt_count = orig_stmts.len();
        let mut new_stmts = Vec::with_capacity(stmt_count);
        if bb == START_BLOCK {
            new_stmts.extend(start_updates.drain(..));
        }

        for (stmt_idx, stmt) in orig_stmts.into_iter().enumerate() {
            let loc = Location::new(bb, stmt_idx);
            let sspan = stmt.span;
            if let Some(moves) = move_data.loc_map.get(&loc) {
                for &mo in moves {
                    let mp = move_data.moves[mo].path;
                    emit_clear_flags(mp, sspan, &mut new_stmts);
                }
            }
            new_stmts.push(stmt);
            if let Some(inits) = move_data.init_loc_map.get(&loc) {
                for &init_idx in inits {
                    let mp = move_data.inits[init_idx].path;
                    move_data.for_each_child_inclusive(mp, |child| {
                        if let Some(&fl) = flagged_paths.get(&child) {
                            new_stmts.push(Statement {
                                span: sspan,
                                kind: StatementKind::Assign(Box::new((
                                    Place::from_local(fl),
                                    Rvalue::Use(Operand::Constant(Box::new(ConstOperand {
                                        span: sspan,
                                        ty: tcx.types.bool,
                                        value: ConstValue::Scalar(1),
                                    }))),
                                ))),
                            });
                        }
                    });
                }
            }
        }

        // Also handle moves and initializations occurring at the block's terminator
        // (e.g. `Call { args: [Move(x)], destination: y, target: Some(tgt) }`).
        let term_loc = Location::new(bb, stmt_count);
        let tspan = body.basic_blocks[bb].terminator().span;
        if let Some(moves) = move_data.loc_map.get(&term_loc) {
            for &mo in moves {
                let mp = move_data.moves[mo].path;
                emit_clear_flags(mp, tspan, &mut new_stmts);
            }
        }
        if let Some(inits) = move_data.init_loc_map.get(&term_loc)
            && let TerminatorKind::Call { target: Some(tgt), .. } =
                body.basic_blocks[bb].terminator().kind
        {
            for &init_idx in inits {
                let mp = move_data.inits[init_idx].path;
                move_data.for_each_child_inclusive(mp, |child| {
                    if let Some(&fl) = flagged_paths.get(&child) {
                        target_inits.push((tgt, fl, tspan));
                    }
                });
            }
        }

        body.basic_blocks[bb].statements = new_stmts;
    }

    for (tgt, fl, tspan) in target_inits {
        body.basic_blocks[tgt].statements.insert(
            0,
            Statement {
                span: tspan,
                kind: StatementKind::Assign(Box::new((
                    Place::from_local(fl),
                    Rvalue::Use(Operand::Constant(Box::new(ConstOperand {
                        span: tspan,
                        ty: tcx.types.bool,
                        value: ConstValue::Scalar(1),
                    }))),
                ))),
            },
        );
    }
}

fn apply_drops_before_statement<'tcx>(
    block: BasicBlock,
    stmt_index: usize,
    drops: &[PendingDrop<'tcx>],
    flagged_paths: &FxHashMap<MovePathIndex, Local>,
    body: &mut Body<'tcx>,
) {
    let tail_stmts = body.basic_blocks[block].statements.split_off(stmt_index);
    let tail_term = body.basic_blocks[block].terminator.take().unwrap();

    let cont_bb = body.basic_blocks.push(BasicBlockData {
        statements: tail_stmts,
        terminator: Some(tail_term),
    });

    chain_drops(block, cont_bb, drops, flagged_paths, body);
}

fn apply_drops_before_terminator<'tcx>(
    block: BasicBlock,
    stmt_index: usize,
    drops: &[PendingDrop<'tcx>],
    flagged_paths: &FxHashMap<MovePathIndex, Local>,
    body: &mut Body<'tcx>,
) {
    let mut after_dead = body.basic_blocks[block].statements.split_off(stmt_index);
    // Move any trailing StorageDead markers placed before Return to after the drops.
    let mut pre_dead = Vec::new();
    let mut tail_dead = Vec::new();
    for st in body.basic_blocks[block].statements.drain(..) {
        if matches!(st.kind, StatementKind::StorageDead(_)) {
            tail_dead.push(st);
        } else {
            pre_dead.push(st);
        }
    }
    body.basic_blocks[block].statements = pre_dead;
    tail_dead.append(&mut after_dead);

    let term = body.basic_blocks[block].terminator.take().unwrap();
    let cont_bb = body.basic_blocks.push(BasicBlockData {
        statements: tail_dead,
        terminator: Some(term),
    });

    chain_drops(block, cont_bb, drops, flagged_paths, body);
}

fn chain_drops<'tcx>(
    mut cur_bb: BasicBlock,
    final_cont: BasicBlock,
    drops: &[PendingDrop<'tcx>],
    flagged_paths: &FxHashMap<MovePathIndex, Local>,
    body: &mut Body<'tcx>,
) {
    for (idx, d) in drops.iter().enumerate() {
        let next_bb = if idx + 1 == drops.len() {
            final_cont
        } else {
            body.basic_blocks.push(BasicBlockData::default())
        };

        if d.needs_flag
            && let Some(&flag_local) = flagged_paths.get(&d.mp)
        {
            let drop_bb = body.basic_blocks.push(BasicBlockData {
                statements: Vec::new(),
                terminator: Some(Terminator {
                    span: d.span,
                    kind: TerminatorKind::Drop {
                        place: d.place.clone(),
                        target: next_bb,
                        unwind: UnwindAction::Continue,
                    },
                }),
            });
            body.basic_blocks[cur_bb].terminator = Some(Terminator {
                span: d.span,
                kind: TerminatorKind::SwitchInt {
                    discr: Operand::Copy(Place::from_local(flag_local)),
                    targets: SwitchTargets { values: vec![(1, drop_bb)], otherwise: next_bb },
                },
            });
        } else {
            body.basic_blocks[cur_bb].terminator = Some(Terminator {
                span: d.span,
                kind: TerminatorKind::Drop {
                    place: d.place.clone(),
                    target: next_bb,
                    unwind: UnwindAction::Continue,
                },
            });
        }

        cur_bb = next_bb;
    }
}
