//! MIR Coroutine State-Machine Transformation (`rustc_mir_transform::coroutine`).
//!
//! Transforms a pre-transform `DefKind::Coroutine` MIR body containing
//! `TerminatorKind::Yield` suspension points into a `Future.poll` state machine:
//!
//! - Variant `0` (`Unresumed`): holds the captured upvars.
//! - Variant `1` (`Returned`): empty (completed normally).
//! - Variant `2` (`Panicked` / `Poisoned`): empty (entered while polling / on unwind).
//! - Variant `3 + k` (`Suspend(k)`): holds the locals live across suspension point `k`.
//!
//! The transformed MIR body has the exact signature of `Future::poll`:
//!   `(_1: Pin<&mut Coroutine>, _2: &mut Context) -> Poll<Output>`
//!
//! Per-state `DropGlue` is automatically emitted by `vsc_codegen_obsidian` from
//! the coroutine's `Variants::Multiple` layout.

mod analyze;
mod rewrite;

use vsc_hir::LangItem;
use vsc_index::Idx;
use vsc_middle::mir::*;
use vsc_middle::ty::{self, Mutability, Ty, TyCtxt, TyKind};
use vsc_span::def_id::DefId;

use self::analyze::analyze_yields;
use self::rewrite::{
    remap_stmt, remap_terminator, rewrite_stmt_places, rewrite_terminator_places,
};

#[derive(Clone, Debug)]
struct YieldSite {
    block: BasicBlock,
    resume: BasicBlock,
    saved_locals: Vec<Local>,
}

/// Computes the `CoroutineLayout` of `def_id` from its drop-elaborated pre-transform MIR.
pub fn compute_layout<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId, body: &Body<'tcx>) -> CoroutineLayout<'tcx> {
    let (_, _, layout) = analyze_yields(tcx, def_id, body);
    layout
}

/// Transforms `body` (a drop-elaborated pre-transform coroutine MIR) in-place into
/// the `Future::poll(self: Pin<&mut Coroutine>, cx: &mut Context) -> Poll<Output>` state machine.
pub fn transform<'tcx>(tcx: TyCtxt<'tcx>, def_id: DefId, body: &mut Body<'tcx>) {
    let (yields, all_saved, layout) = analyze_yields(tcx, def_id, body);
    let span = body.span;
    let md_def = tcx.lang_items().get(LangItem::ManuallyDrop);
    let use_in_place = md_def.is_some() && !yields.is_empty() && !all_saved.is_empty();

    // Raw output type `R` (was `_0`) and coroutine type `Coro` (was `*_1`).
    let raw_ret_ty = body.local_decls[RETURN_PLACE].ty;
    let env_ref_ty = body.local_decls[Local::from_usize(1)].ty;
    let coro_ty = match *env_ref_ty.kind() {
        TyKind::Ref(_, inner, _) => inner,
        _ => env_ref_ty,
    };

    // Build `Poll<R>`, `Pin<&mut Coroutine>`, and `&mut Context` types from lang items.
    let poll_def = tcx.lang_items().get(LangItem::Poll).expect("Poll lang item for coroutine transform");
    let poll_args = tcx.mk_args(&[ty::GenericArg::Type(raw_ret_ty)]);
    let poll_ty = tcx.mk_adt(tcx.adt_def(poll_def), poll_args);

    let pin_def = tcx.lang_items().get(LangItem::Pin).expect("Pin lang item for coroutine transform");
    let pin_args = tcx.mk_args(&[ty::GenericArg::Type(env_ref_ty)]);
    let pin_ty = tcx.mk_adt(tcx.adt_def(pin_def), pin_args);

    let cx_ref_ty = if let Some(poll_fn) = tcx.lang_items().get(LangItem::FuturePoll) {
        let sig = tcx.fn_sig(poll_fn).skip_binder();
        let args = tcx.mk_args(&[ty::GenericArg::Type(coro_ty)]);
        tcx.erase_regions(tcx.instantiate(sig.inputs()[1], args))
    } else {
        tcx.mk_ref(tcx.regions.re_erased, tcx.types.unit, Mutability::Mut)
    };
    let cx_raw_ty = tcx.mk_ty(TyKind::RawPtr(tcx.types.unit, Mutability::Mut));

    // Allocate new temp locals for the original `_0` (raw return), `_1` (`self_ref: &mut Coroutine`),
    // and `_2` (`cx_raw: *mut ()`), so `_0`, `_1`, `_2` can take on `Future::poll`'s ABI.
    let raw_ret_local = body.local_decls.push(LocalDecl {
        mutability: Mutability::Mut,
        ty: raw_ret_ty,
        span,
        kind: LocalKind::Temp,
    });
    let self_ref_local = body.local_decls.push(LocalDecl {
        mutability: Mutability::Mut,
        ty: env_ref_ty,
        span,
        kind: LocalKind::Temp,
    });
    let cx_raw_local = body.local_decls.push(LocalDecl {
        mutability: Mutability::Mut,
        ty: cx_raw_ty,
        span,
        kind: LocalKind::Temp,
    });

    // Update `_0`, `_1`, `_2` declarations to `Future::poll`'s signature.
    body.local_decls[RETURN_PLACE] = LocalDecl {
        mutability: Mutability::Mut,
        ty: poll_ty,
        span,
        kind: LocalKind::ReturnPointer,
    };
    body.local_decls[Local::from_usize(1)] = LocalDecl {
        mutability: Mutability::Mut,
        ty: pin_ty,
        span,
        kind: LocalKind::Arg,
    };
    body.local_decls[Local::from_usize(2)] = LocalDecl {
        mutability: Mutability::Mut,
        ty: cx_ref_ty,
        span,
        kind: LocalKind::Arg,
    };
    body.arg_count = 2;

    // Remap all existing references to `_0`, `_1`, `_2` in the original blocks
    // to `raw_ret_local`, `self_ref_local`, `cx_raw_local`.
    let remap_local = |l: &mut Local| {
        if *l == RETURN_PLACE {
            *l = raw_ret_local;
        } else if *l == Local::from_usize(1) {
            *l = self_ref_local;
        } else if *l == Local::from_usize(2) {
            *l = cx_raw_local;
        }
    };
    for bb_data in body.basic_blocks.iter_mut() {
        for stmt in &mut bb_data.statements {
            remap_stmt(stmt, &remap_local);
        }
        if let Some(term) = &mut bb_data.terminator {
            remap_terminator(term, &remap_local);
        }
    }

    // Rewrite every original `TerminatorKind::Return` to:
    //   SetDiscriminant(*self_ref_local, 1);
    //   _0 = Poll::Ready(move raw_ret_local);
    //   Return;
    let self_deref = Place::from_local(self_ref_local).deref();
    for bb_data in body.basic_blocks.iter_mut() {
        if matches!(bb_data.terminator.as_ref().map(|t| &t.kind), Some(TerminatorKind::Return)) {
            let tspan = bb_data.terminator.as_ref().unwrap().span;
            bb_data.statements.push(Statement {
                span: tspan,
                kind: StatementKind::SetDiscriminant {
                    place: Box::new(self_deref.clone()),
                    variant_index: 1,
                },
            });
            bb_data.statements.push(Statement {
                span: tspan,
                kind: StatementKind::Assign(Box::new((
                    Place::from_local(RETURN_PLACE),
                    Rvalue::Aggregate(
                        Box::new(AggregateKind::Adt {
                            def_id: poll_def,
                            variant_index: 0, // Poll::Ready
                            args: poll_args,
                            active_field: None,
                        }),
                        vec![Operand::Move(Place::from_local(raw_ret_local))],
                    ),
                ))),
            });
        }
    }

    // Rewrite each `Yield` block `k` (variant `3 + k`) and create its resume trampoline block.
    let mut resume_cases: Vec<(u128, BasicBlock)> = Vec::new();
    for (k, ys) in yields.iter().enumerate() {
        let variant_idx = 3 + k;
        let tspan = body.basic_blocks[ys.block].terminator.as_ref().map_or(span, |t| t.span);

        // At yield point: if using in-place pinned storage, all saved locals already live in
        // `(*self_ref as Variant(2/3+k))` at identical field offsets, so ZERO copies are needed!
        // Otherwise fall back to explicit stack <-> self copies.
        let yield_bb = &mut body.basic_blocks[ys.block];
        if !use_in_place {
            for (field_i, &local) in ys.saved_locals.iter().enumerate() {
                let fty = layout.variant_fields[variant_idx][field_i];
                let dst = self_deref.clone().project(PlaceElem::Downcast(variant_idx)).field(field_i, fty);
                yield_bb.statements.push(Statement {
                    span: tspan,
                    kind: StatementKind::Assign(Box::new((dst, Rvalue::Use(Operand::Move(Place::from_local(local)))))),
                });
            }
        }
        yield_bb.statements.push(Statement {
            span: tspan,
            kind: StatementKind::SetDiscriminant {
                place: Box::new(self_deref.clone()),
                variant_index: variant_idx,
            },
        });
        yield_bb.statements.push(Statement {
            span: tspan,
            kind: StatementKind::Assign(Box::new((
                Place::from_local(RETURN_PLACE),
                Rvalue::Aggregate(
                    Box::new(AggregateKind::Adt {
                        def_id: poll_def,
                        variant_index: 1, // Poll::Pending
                        args: poll_args,
                        active_field: None,
                    }),
                    Vec::new(),
                ),
            ))),
        });
        yield_bb.terminator = Some(Terminator { span: tspan, kind: TerminatorKind::Return });

        // Create resume trampoline: set discriminant to `2` (Running/Panicked) and jump to `ys.resume`.
        let mut stmts = Vec::new();
        if !use_in_place {
            for (field_i, &local) in ys.saved_locals.iter().enumerate() {
                let fty = layout.variant_fields[variant_idx][field_i];
                let src = self_deref.clone().project(PlaceElem::Downcast(variant_idx)).field(field_i, fty);
                stmts.push(Statement {
                    span: tspan,
                    kind: StatementKind::Assign(Box::new((
                        Place::from_local(local),
                        Rvalue::Use(Operand::Move(src)),
                    ))),
                });
            }
        }
        stmts.push(Statement {
            span: tspan,
            kind: StatementKind::SetDiscriminant {
                place: Box::new(self_deref.clone()),
                variant_index: 2,
            },
        });
        let resume_bb = body.basic_blocks.push(BasicBlockData {
            statements: stmts,
            terminator: Some(Terminator {
                span: tspan,
                kind: TerminatorKind::Goto { target: ys.resume },
            }),
        });
        resume_cases.push((variant_idx as u128, resume_bb));
    }

    // Create the `panic_bb` for polling after completion / poison (`1` or `2`).
    let panic_bb = if let Some(panic_fn) = tcx.lang_items().get(LangItem::Panic) {
        let msg_ty = tcx.mk_ref(tcx.regions.re_erased, tcx.types.str_, Mutability::Not);
        let msg_op = Operand::Constant(Box::new(ConstOperand {
            span,
            ty: msg_ty,
            value: ConstValue::Str(vsc_span::Symbol::intern("async function resumed after completion")),
        }));
        let fn_ty = tcx.mk_fn_def(panic_fn, &[]);
        let fn_op = Operand::Constant(Box::new(ConstOperand {
            span,
            ty: fn_ty,
            value: ConstValue::ZeroSized,
        }));
        let dummy_dst = body.local_decls.push(LocalDecl {
            mutability: Mutability::Mut,
            ty: tcx.types.never,
            span,
            kind: LocalKind::Temp,
        });
        body.basic_blocks.push(BasicBlockData {
            statements: Vec::new(),
            terminator: Some(Terminator {
                span,
                kind: TerminatorKind::Call {
                    func: fn_op,
                    args: vec![msg_op],
                    destination: Place::from_local(dummy_dst),
                    target: None,
                    fn_span: span,
                },
            }),
        })
    } else {
        body.basic_blocks.push(BasicBlockData {
            statements: Vec::new(),
            terminator: Some(Terminator { span, kind: TerminatorKind::Unreachable }),
        })
    };

    // Move original `START_BLOCK` (bb0) to a new block index so the new dispatch block
    // occupies `START_BLOCK` (bb0), since MIR execution always enters at `bb0`.
    let old_start_data = std::mem::take(&mut body.basic_blocks[START_BLOCK]);
    let old_start_moved = body.basic_blocks.push(old_start_data);
    // Redirect any jumps in the original body that targeted `START_BLOCK` (e.g. loop at entry)
    // to `old_start_moved`!
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        let _ = (bb, data);
    }
    for bb_idx in 1..body.basic_blocks.len() {
        let bb = BasicBlock::from_usize(bb_idx);
        if let Some(term) = &mut body.basic_blocks[bb].terminator {
            for succ in term.kind.successors_mut() {
                if *succ == START_BLOCK {
                    *succ = old_start_moved;
                }
            }
        }
    }
    for (_, target) in &mut resume_cases {
        if *target == START_BLOCK {
            *target = old_start_moved;
        }
    }

    // Create `unresumed_bb`: sets discriminant to `2` after upvars are unpacked out of Variant 0.
    // When using in-place pinned storage, Variant 0 overlaps Variant 2 in memory, so we first
    // move upvars out of Variant 0 into temporary stack locals, set discriminant to 2, then
    // rewrite all in-place saved locals to `(*self_ref as Variant(2)).field_i.0`!
    let upvar_count = layout.variant_fields[0].len();
    if use_in_place {
        let mut saved_slot: Vec<Option<(usize, Ty<'tcx>, Ty<'tcx>)>> = vec![None; body.local_decls.len()];
        for (field_i, &local) in all_saved.iter().enumerate() {
            let inner_ty = body.local_decls[local].ty;
            let md_ty = layout.variant_fields[2][field_i];
            saved_slot[local.index()] = Some((field_i, md_ty, inner_ty));
        }

        // Partition `old_start_moved` so ALL `Downcast(0)` upvar unpacks execute FIRST into
        // stack temps, followed immediately by `SetDiscriminant(*self_ref, 2)` and stores into
        // Variant 2, followed by all other statements (including `elaborate_drops` flag inits).
        // This guarantees Variant 0 is completely read before any byte of Variant 2 is written!
        let old_stmts = std::mem::take(&mut body.basic_blocks[old_start_moved].statements);
        let mut unpack_stmts = Vec::new();
        let mut staged_upvar_moves: Vec<(Local, Local)> = Vec::new();
        let mut rest_stmts = Vec::with_capacity(old_stmts.len());
        for mut stmt in old_stmts {
            let reads_variant0 = matches!(
                &stmt.kind,
                StatementKind::Assign(box_pair)
                    if matches!(
                        &box_pair.1,
                        Rvalue::Use(Operand::Move(src) | Operand::Copy(src))
                            if src.local == self_ref_local
                                && src.projection.iter().any(|e| matches!(e, PlaceElem::Downcast(0)))
                    )
            );
            if reads_variant0 {
                if let StatementKind::Assign(ref mut box_pair) = stmt.kind {
                    if box_pair.0.projection.is_empty() {
                        let orig_l = box_pair.0.local;
                        if orig_l.index() < saved_slot.len() && saved_slot[orig_l.index()].is_some() {
                            let orig_ty = body.local_decls[orig_l].ty;
                            let tmp_l = body.local_decls.push(LocalDecl {
                                mutability: Mutability::Mut,
                                ty: orig_ty,
                                span,
                                kind: LocalKind::Temp,
                            });
                            box_pair.0 = Place::from_local(tmp_l);
                            staged_upvar_moves.push((orig_l, tmp_l));
                        }
                    }
                }
                unpack_stmts.push(stmt);
            } else {
                rest_stmts.push(stmt);
            }
        }
        let mut reordered = Vec::with_capacity(unpack_stmts.len() + 1 + staged_upvar_moves.len() + rest_stmts.len());
        reordered.extend(unpack_stmts);
        reordered.push(Statement {
            span,
            kind: StatementKind::SetDiscriminant {
                place: Box::new(self_deref.clone()),
                variant_index: 2,
            },
        });
        for (orig_l, tmp_l) in staged_upvar_moves {
            reordered.push(Statement {
                span,
                kind: StatementKind::Assign(Box::new((
                    Place::from_local(orig_l),
                    Rvalue::Use(Operand::Move(Place::from_local(tmp_l))),
                ))),
            });
        }
        reordered.extend(rest_stmts);
        body.basic_blocks[old_start_moved].statements = reordered;

        // Rewrite every reference to any `local` in `all_saved` across all blocks directly to
        // `(*self_ref as Variant(2)).field(field_i, ManuallyDrop<T>).field(0, T)`.
        let make_pinned_place = |local: Local| -> Option<Place<'tcx>> {
            let (field_i, md_ty, inner_ty) = *saved_slot.get(local.index())?.as_ref()?;
            Some(
                self_deref
                    .clone()
                    .project(PlaceElem::Downcast(2))
                    .field(field_i, md_ty)
                    .field(0, inner_ty),
            )
        };
        let n_bbs = body.basic_blocks.len();
        for bb_idx in 0..n_bbs {
            let bb = BasicBlock::from_usize(bb_idx);
            let old_stmts = std::mem::take(&mut body.basic_blocks[bb].statements);
            let mut new_stmts = Vec::with_capacity(old_stmts.len());
            for mut stmt in old_stmts {
                if matches!(stmt.kind, StatementKind::StorageLive(l) | StatementKind::StorageDead(l) if l.index() < saved_slot.len() && saved_slot[l.index()].is_some()) {
                    continue;
                }
                rewrite_stmt_places(
                    &mut stmt,
                    &mut new_stmts,
                    &mut body.local_decls,
                    &make_pinned_place,
                );
                new_stmts.push(stmt);
            }
            if let Some(mut term) = body.basic_blocks[bb].terminator.take() {
                rewrite_terminator_places(
                    &mut term,
                    &mut new_stmts,
                    &mut body.local_decls,
                    &make_pinned_place,
                );
                body.basic_blocks[bb].terminator = Some(term);
            }
            body.basic_blocks[bb].statements = new_stmts;
        }
        // Clear stack storage for all in-place locals so codegen allocates 0 stack bytes for them!
        for &local in &all_saved {
            body.local_decls[local].ty = tcx.types.unit;
        }
    } else {
        let old_bb = &mut body.basic_blocks[old_start_moved];
        let insert_pos = upvar_count.min(old_bb.statements.len());
        old_bb.statements.insert(
            insert_pos,
            Statement {
                span,
                kind: StatementKind::SetDiscriminant {
                    place: Box::new(self_deref.clone()),
                    variant_index: 2,
                },
            },
        );
    }

    // Build the entry dispatch block at `START_BLOCK` (bb0):
    //   self_ref_local = move _1.0;        // Pin.pointer
    //   cx_raw_tmp = &raw mut (*_2);       // *mut Context
    //   cx_raw_local = move cx_raw_tmp as *mut ();
    //   discr = discriminant(*self_ref_local);
    //   switchInt(discr) -> [0: old_start_moved, 3+k: resume_k, otherwise: panic_bb]
    let cx_inner_ty = match *cx_ref_ty.kind() {
        TyKind::Ref(_, inner, _) => inner,
        _ => tcx.types.unit,
    };
    let cx_typed_raw = body.local_decls.push(LocalDecl {
        mutability: Mutability::Mut,
        ty: tcx.mk_ty(TyKind::RawPtr(cx_inner_ty, Mutability::Mut)),
        span,
        kind: LocalKind::Temp,
    });
    let discr_local = body.local_decls.push(LocalDecl {
        mutability: Mutability::Mut,
        ty: tcx.types.isize,
        span,
        kind: LocalKind::Temp,
    });

    let mut switch_values = Vec::with_capacity(1 + resume_cases.len());
    switch_values.push((0u128, old_start_moved));
    switch_values.extend(resume_cases);

    body.basic_blocks[START_BLOCK] = BasicBlockData {
        statements: vec![
            Statement {
                span,
                kind: StatementKind::Assign(Box::new((
                    Place::from_local(self_ref_local),
                    Rvalue::Use(Operand::Move(Place::from_local(Local::from_usize(1)).field(0, env_ref_ty))),
                ))),
            },
            Statement {
                span,
                kind: StatementKind::Assign(Box::new((
                    Place::from_local(cx_typed_raw),
                    Rvalue::RawPtr(Mutability::Mut, Place::from_local(Local::from_usize(2)).deref()),
                ))),
            },
            Statement {
                span,
                kind: StatementKind::Assign(Box::new((
                    Place::from_local(cx_raw_local),
                    Rvalue::Cast(CastKind::PtrToPtr, Operand::Move(Place::from_local(cx_typed_raw)), cx_raw_ty),
                ))),
            },
            Statement {
                span,
                kind: StatementKind::Assign(Box::new((
                    Place::from_local(discr_local),
                    Rvalue::Discriminant(self_deref),
                ))),
            },
        ],
        terminator: Some(Terminator {
            span,
            kind: TerminatorKind::SwitchInt {
                discr: Operand::Copy(Place::from_local(discr_local)),
                targets: SwitchTargets { values: switch_values, otherwise: panic_bb },
            },
        }),
    };
}

