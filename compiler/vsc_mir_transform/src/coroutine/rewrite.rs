//! Place rewriting (for pinned self-borrows) and local remapping for MIR coroutines.

use vsc_middle::mir::*;
use vsc_middle::ty::Mutability;

pub(super) fn rewrite_place_in_place<'tcx>(
    p: &mut Place<'tcx>,
    pre_stmts: &mut Vec<Statement<'tcx>>,
    local_decls: &mut vsc_index::IndexVec<Local, LocalDecl<'tcx>>,
    span: vsc_span::Span,
    make_pinned: &impl Fn(Local) -> Option<Place<'tcx>>,
) {
    // First materialize any in-place `PlaceElem::Index(idx)` into a stack temp:
    for elem in &mut p.projection {
        if let PlaceElem::Index(idx) = elem {
            if let Some(pinned_idx) = make_pinned(*idx) {
                let idx_ty = local_decls[*idx].ty;
                let tmp = local_decls.push(LocalDecl {
                    mutability: Mutability::Mut,
                    ty: idx_ty,
                    span,
                    kind: LocalKind::Temp,
                });
                pre_stmts.push(Statement {
                    span,
                    kind: StatementKind::Assign(Box::new((
                        Place::from_local(tmp),
                        Rvalue::Use(Operand::Copy(pinned_idx)),
                    ))),
                });
                *idx = tmp;
            }
        }
    }
    if let Some(mut base) = make_pinned(p.local) {
        base.projection.extend(p.projection.iter().cloned());
        *p = base;
    }
}

pub(super) fn rewrite_operand_places<'tcx>(
    op: &mut Operand<'tcx>,
    pre_stmts: &mut Vec<Statement<'tcx>>,
    local_decls: &mut vsc_index::IndexVec<Local, LocalDecl<'tcx>>,
    span: vsc_span::Span,
    make_pinned: &impl Fn(Local) -> Option<Place<'tcx>>,
) {
    match op {
        Operand::Copy(p) | Operand::Move(p) => {
            rewrite_place_in_place(p, pre_stmts, local_decls, span, make_pinned);
        }
        Operand::Constant(_) => {}
    }
}

pub(super) fn rewrite_rvalue_places<'tcx>(
    rv: &mut Rvalue<'tcx>,
    pre_stmts: &mut Vec<Statement<'tcx>>,
    local_decls: &mut vsc_index::IndexVec<Local, LocalDecl<'tcx>>,
    span: vsc_span::Span,
    make_pinned: &impl Fn(Local) -> Option<Place<'tcx>>,
) {
    match rv {
        Rvalue::Use(op)
        | Rvalue::Repeat(op, _)
        | Rvalue::Cast(_, op, _)
        | Rvalue::UnaryOp(_, op)
        | Rvalue::ClassNew(op) => {
            rewrite_operand_places(op, pre_stmts, local_decls, span, make_pinned);
        }
        Rvalue::Ref(_, p) | Rvalue::RawPtr(_, p) | Rvalue::Len(p) | Rvalue::Discriminant(p) => {
            rewrite_place_in_place(p, pre_stmts, local_decls, span, make_pinned);
        }
        Rvalue::BinaryOp(_, pair) | Rvalue::CheckedBinaryOp(_, pair) => {
            rewrite_operand_places(&mut pair.0, pre_stmts, local_decls, span, make_pinned);
            rewrite_operand_places(&mut pair.1, pre_stmts, local_decls, span, make_pinned);
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops {
                rewrite_operand_places(op, pre_stmts, local_decls, span, make_pinned);
            }
        }
    }
}

pub(super) fn rewrite_stmt_places<'tcx>(
    stmt: &mut Statement<'tcx>,
    pre_stmts: &mut Vec<Statement<'tcx>>,
    local_decls: &mut vsc_index::IndexVec<Local, LocalDecl<'tcx>>,
    make_pinned: &impl Fn(Local) -> Option<Place<'tcx>>,
) {
    let span = stmt.span;
    match &mut stmt.kind {
        StatementKind::Assign(box_pair) => {
            rewrite_place_in_place(&mut box_pair.0, pre_stmts, local_decls, span, make_pinned);
            rewrite_rvalue_places(&mut box_pair.1, pre_stmts, local_decls, span, make_pinned);
        }
        StatementKind::SetDiscriminant { place, .. } => {
            rewrite_place_in_place(place, pre_stmts, local_decls, span, make_pinned);
        }
        StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {}
    }
}

pub(super) fn rewrite_terminator_places<'tcx>(
    term: &mut Terminator<'tcx>,
    pre_stmts: &mut Vec<Statement<'tcx>>,
    local_decls: &mut vsc_index::IndexVec<Local, LocalDecl<'tcx>>,
    make_pinned: &impl Fn(Local) -> Option<Place<'tcx>>,
) {
    let span = term.span;
    match &mut term.kind {
        TerminatorKind::SwitchInt { discr, .. } => {
            rewrite_operand_places(discr, pre_stmts, local_decls, span, make_pinned);
        }
        TerminatorKind::Call { func, args, destination, .. } => {
            rewrite_operand_places(func, pre_stmts, local_decls, span, make_pinned);
            for a in args {
                rewrite_operand_places(a, pre_stmts, local_decls, span, make_pinned);
            }
            rewrite_place_in_place(destination, pre_stmts, local_decls, span, make_pinned);
        }
        TerminatorKind::Assert { cond, msg, .. } => {
            rewrite_operand_places(cond, pre_stmts, local_decls, span, make_pinned);
            match &mut **msg {
                AssertKind::BoundsCheck { len, index } => {
                    rewrite_operand_places(len, pre_stmts, local_decls, span, make_pinned);
                    rewrite_operand_places(index, pre_stmts, local_decls, span, make_pinned);
                }
                AssertKind::Overflow(_, a, b) => {
                    rewrite_operand_places(a, pre_stmts, local_decls, span, make_pinned);
                    rewrite_operand_places(b, pre_stmts, local_decls, span, make_pinned);
                }
                AssertKind::DivisionByZero(a) | AssertKind::RemainderByZero(a) => {
                    rewrite_operand_places(a, pre_stmts, local_decls, span, make_pinned);
                }
            }
        }
        TerminatorKind::Drop { place, .. } => {
            rewrite_place_in_place(place, pre_stmts, local_decls, span, make_pinned);
        }
        TerminatorKind::InlineAsm { operands, .. } => {
            for op in operands {
                rewrite_operand_places(op, pre_stmts, local_decls, span, make_pinned);
            }
        }
        TerminatorKind::Yield { value, resume_arg, .. } => {
            rewrite_operand_places(value, pre_stmts, local_decls, span, make_pinned);
            rewrite_place_in_place(resume_arg, pre_stmts, local_decls, span, make_pinned);
        }
        TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {}
    }
}


pub(super) fn remap_place(p: &mut Place<'_>, f: &impl Fn(&mut Local)) {
    f(&mut p.local);
    for elem in &mut p.projection {
        if let PlaceElem::Index(idx) = elem {
            f(idx);
        }
    }
}

pub(super) fn remap_operand(op: &mut Operand<'_>, f: &impl Fn(&mut Local)) {
    match op {
        Operand::Copy(p) | Operand::Move(p) => remap_place(p, f),
        Operand::Constant(_) => {}
    }
}

pub(super) fn remap_rvalue(rv: &mut Rvalue<'_>, f: &impl Fn(&mut Local)) {
    match rv {
        Rvalue::Use(op) | Rvalue::Repeat(op, _) | Rvalue::Cast(_, op, _) | Rvalue::UnaryOp(_, op) | Rvalue::ClassNew(op) => {
            remap_operand(op, f);
        }
        Rvalue::Ref(_, p) | Rvalue::RawPtr(_, p) | Rvalue::Len(p) | Rvalue::Discriminant(p) => {
            remap_place(p, f);
        }
        Rvalue::BinaryOp(_, pair) | Rvalue::CheckedBinaryOp(_, pair) => {
            remap_operand(&mut pair.0, f);
            remap_operand(&mut pair.1, f);
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops {
                remap_operand(op, f);
            }
        }
    }
}

pub(super) fn remap_stmt(stmt: &mut Statement<'_>, f: &impl Fn(&mut Local)) {
    match &mut stmt.kind {
        StatementKind::Assign(box_pair) => {
            remap_place(&mut box_pair.0, f);
            remap_rvalue(&mut box_pair.1, f);
        }
        StatementKind::SetDiscriminant { place, .. } => remap_place(place, f),
        StatementKind::StorageLive(l) | StatementKind::StorageDead(l) => f(l),
        StatementKind::Nop => {}
    }
}

pub(super) fn remap_terminator(term: &mut Terminator<'_>, f: &impl Fn(&mut Local)) {
    match &mut term.kind {
        TerminatorKind::SwitchInt { discr, .. } => remap_operand(discr, f),
        TerminatorKind::Call { func, args, destination, .. } => {
            remap_operand(func, f);
            for a in args {
                remap_operand(a, f);
            }
            remap_place(destination, f);
        }
        TerminatorKind::Assert { cond, msg, .. } => {
            remap_operand(cond, f);
            match &mut **msg {
                AssertKind::BoundsCheck { len, index } => {
                    remap_operand(len, f);
                    remap_operand(index, f);
                }
                AssertKind::Overflow(_, a, b) => {
                    remap_operand(a, f);
                    remap_operand(b, f);
                }
                AssertKind::DivisionByZero(a) | AssertKind::RemainderByZero(a) => remap_operand(a, f),
            }
        }
        TerminatorKind::Drop { place, .. } => remap_place(place, f),
        TerminatorKind::InlineAsm { operands, .. } => {
            for op in operands {
                remap_operand(op, f);
            }
        }
        TerminatorKind::Yield { value, resume_arg, .. } => {
            remap_operand(value, f);
            remap_place(resume_arg, f);
        }
        TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {}
    }
}
