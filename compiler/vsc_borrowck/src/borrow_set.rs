//! Collects every borrow (`Rvalue::Ref`) in a MIR body, tracks two-phase call
//! reservations, and computes flow-insensitive borrow holder propagation +
//! forward active-borrows dataflow analysis.

use vsc_data_structures::FxHashMap;
use vsc_index::{DenseBitSet, Idx, IndexVec};
use vsc_middle::mir::*;
use vsc_middle::ty::{TyCtxt, TyKind};
use vsc_mir_dataflow::framework::{Analysis, CallReturnPlaces, Forward, Location};
use vsc_span::Span;

vsc_index::newtype_index! {
    pub struct BorrowIndex { DEBUG_FORMAT = "bw{}" }
}

#[derive(Clone, Debug)]
pub struct BorrowData<'tcx> {
    pub assigned_place: Place<'tcx>,
    pub kind: BorrowKind,
    pub borrowed_place: Place<'tcx>,
    pub reserve_location: Location,
    /// When `Some(call_loc)`, this mutable borrow is created in a `Temp` purely
    /// as an argument to `call_loc`, so shared reads before `call_loc` do not
    /// conflict until activation at `call_loc` (two-phase borrow).
    pub two_phase_call: Option<Location>,
    pub span: Span,
}

pub struct BorrowSet<'tcx> {
    pub borrows: IndexVec<BorrowIndex, BorrowData<'tcx>>,
    pub location_map: FxHashMap<Location, BorrowIndex>,
    /// For each `BorrowIndex`, the set of locals that may directly or
    /// transitively hold references originating from that borrow.
    pub holders: IndexVec<BorrowIndex, DenseBitSet<Local>>,
    /// Maps each `Local` to the `BorrowIndex`es whose values may reside in it.
    pub local_held_borrows: IndexVec<Local, DenseBitSet<BorrowIndex>>,
}

impl<'tcx> BorrowSet<'tcx> {
    pub fn build(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> Self {
        let mut borrows = IndexVec::new();
        let mut location_map = FxHashMap::default();
        let mut edges: Vec<(Local, Local)> = Vec::new();
        let mut local_use_counts = vec![0usize; body.local_decls.len()];
        let mut local_call_sites: Vec<Vec<Location>> = vec![Vec::new(); body.local_decls.len()];

        for (bb, data) in body.basic_blocks.iter_enumerated() {
            for (stmt_idx, stmt) in data.statements.iter().enumerate() {
                let loc = Location::new(bb, stmt_idx);
                if let StatementKind::Assign(box_pair) = &stmt.kind {
                    let (lhs, rvalue) = &**box_pair;
                    match rvalue {
                        Rvalue::Ref(kind, borrowed_place) => {
                            let idx = borrows.push(BorrowData {
                                assigned_place: lhs.clone(),
                                kind: *kind,
                                borrowed_place: borrowed_place.clone(),
                                reserve_location: loc,
                                two_phase_call: None,
                                span: stmt.span,
                            });
                            location_map.insert(loc, idx);
                            if !borrowed_place.projection.is_empty()
                                && matches!(borrowed_place.projection[0], PlaceElem::Deref)
                                && matches!(
                                    body.local_decls[borrowed_place.local].ty.kind(),
                                    TyKind::Ref(..)
                                )
                            {
                                edges.push((lhs.local, borrowed_place.local));
                            }
                        }
                        _ => collect_value_flow_edges(rvalue, lhs.local, &mut edges),
                    }
                    count_rvalue_local_uses(rvalue, &mut local_use_counts);
                    count_place_proj_uses(lhs, &mut local_use_counts);
                }
            }
            let term_loc = Location::new(bb, data.statements.len());
            let term = data.terminator();
            match &term.kind {
                TerminatorKind::Call { func, args, .. } => {
                    count_operand_uses(func, &mut local_use_counts);
                    for arg in args {
                        count_operand_uses(arg, &mut local_use_counts);
                        if let Some(p) = arg.place()
                            && p.projection.is_empty()
                        {
                            local_call_sites[p.local.index()].push(term_loc);
                        }
                    }
                }
                TerminatorKind::SwitchInt { discr, .. } | TerminatorKind::Assert { cond: discr, .. } => {
                    count_operand_uses(discr, &mut local_use_counts);
                }
                TerminatorKind::Drop { place, .. } => {
                    local_use_counts[place.local.index()] += 1;
                }
                TerminatorKind::InlineAsm { operands, .. } => {
                    for op in operands {
                        count_operand_uses(op, &mut local_use_counts);
                    }
                }
                TerminatorKind::Yield { value, .. } => {
                    count_operand_uses(value, &mut local_use_counts);
                }
                TerminatorKind::Goto { .. } | TerminatorKind::Return | TerminatorKind::Unreachable => {
                }
            }
        }

        // Mark single-use `Temp` mutable borrows passed directly into a Call in
        // the same block as two-phase borrows.
        for data in borrows.iter_mut() {
            if data.kind == BorrowKind::Mut && data.assigned_place.projection.is_empty() {
                let l = data.assigned_place.local;
                if matches!(body.local_decls[l].kind, LocalKind::Temp)
                    && local_use_counts[l.index()] == 1
                    && local_call_sites[l.index()].len() == 1
                {
                    let call_loc = local_call_sites[l.index()][0];
                    data.two_phase_call = Some(call_loc);
                }
            }
        }

        let num_borrows = borrows.len();
        let num_locals = body.local_decls.len();
        let mut holders = IndexVec::from_raw(vec![DenseBitSet::new_empty(num_locals); num_borrows]);
        let mut local_held_borrows =
            IndexVec::from_raw(vec![DenseBitSet::new_empty(num_borrows); num_locals]);

        for (b_idx, b) in borrows.iter_enumerated() {
            holders[b_idx].insert(b.assigned_place.local);
            local_held_borrows[b.assigned_place.local].insert(b_idx);
        }

        // Propagate `lhs <- rhs` edges until fixpoint so closure/tuple/return moves
        // preserve borrow liveness accurately.
        let mut changed = true;
        while changed {
            changed = false;
            for &(dst, src) in &edges {
                let src_borrows = local_held_borrows[src].clone();
                if local_held_borrows[dst].union(&src_borrows) {
                    changed = true;
                }
            }
        }
        for (local_idx, held) in local_held_borrows.iter_enumerated() {
            for b_idx in borrows.indices() {
                if held.contains(b_idx) {
                    holders[b_idx].insert(local_idx);
                }
            }
        }

        let _ = tcx;
        BorrowSet { borrows, location_map, holders, local_held_borrows }
    }
}

fn collect_value_flow_edges(rvalue: &Rvalue<'_>, dst: Local, edges: &mut Vec<(Local, Local)>) {
    let mut add_op = |op: &Operand<'_>| {
        if let Some(p) = op.place() {
            edges.push((dst, p.local));
        }
    };
    match rvalue {
        Rvalue::Use(op) | Rvalue::Repeat(op, _) | Rvalue::Cast(_, op, _) | Rvalue::ClassNew(op) => {
            add_op(op);
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops {
                add_op(op);
            }
        }
        Rvalue::Ref(..)
        | Rvalue::RawPtr(..)
        | Rvalue::Len(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::CheckedBinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Discriminant(_) => {}
    }
}

fn count_place_proj_uses(place: &Place<'_>, counts: &mut [usize]) {
    for elem in &place.projection {
        if let PlaceElem::Index(idx) = *elem {
            counts[idx.index()] += 1;
        }
    }
}

fn count_operand_uses(op: &Operand<'_>, counts: &mut [usize]) {
    if let Some(p) = op.place() {
        counts[p.local.index()] += 1;
        count_place_proj_uses(p, counts);
    }
}

fn count_rvalue_local_uses(rv: &Rvalue<'_>, counts: &mut [usize]) {
    match rv {
        Rvalue::Use(op)
        | Rvalue::Repeat(op, _)
        | Rvalue::Cast(_, op, _)
        | Rvalue::UnaryOp(_, op)
        | Rvalue::ClassNew(op) => count_operand_uses(op, counts),
        Rvalue::Ref(_, p) | Rvalue::RawPtr(_, p) | Rvalue::Len(p) | Rvalue::Discriminant(p) => {
            count_place_proj_uses(p, counts);
        }
        Rvalue::BinaryOp(_, ops) | Rvalue::CheckedBinaryOp(_, ops) => {
            count_operand_uses(&ops.0, counts);
            count_operand_uses(&ops.1, counts);
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops {
                count_operand_uses(op, counts);
            }
        }
    }
}

/// Forward dataflow analysis computing the set of borrows generated along some
/// path reaching each program point that have not been explicitly overwritten.
pub struct Borrows<'a, 'tcx> {
    borrow_set: &'a BorrowSet<'tcx>,
}

impl<'a, 'tcx> Borrows<'a, 'tcx> {
    pub fn new(borrow_set: &'a BorrowSet<'tcx>) -> Self {
        Borrows { borrow_set }
    }
}

impl<'a, 'tcx> Analysis<'tcx> for Borrows<'a, 'tcx> {
    type Domain = DenseBitSet<BorrowIndex>;
    type Direction = Forward;
    const NAME: &'static str = "borrows";

    fn bottom_value(&self, _body: &Body<'tcx>) -> Self::Domain {
        DenseBitSet::new_empty(self.borrow_set.borrows.len())
    }

    fn initialize_start_block(&self, _body: &Body<'tcx>, _state: &mut Self::Domain) {}

    fn apply_statement_effect(
        &self,
        state: &mut Self::Domain,
        stmt: &Statement<'tcx>,
        loc: Location,
    ) {
        match &stmt.kind {
            StatementKind::Assign(box_pair) => {
                let (lhs, _) = &**box_pair;
                if lhs.projection.is_empty() {
                    // Kill borrows stored solely inside `lhs` when `lhs` is overwritten.
                    for (b_idx, b) in self.borrow_set.borrows.iter_enumerated() {
                        if b.assigned_place.projection.is_empty()
                            && b.assigned_place.local == lhs.local
                            && self.borrow_set.holders[b_idx].count() == 1
                        {
                            state.remove(b_idx);
                        }
                    }
                }
                if let Some(&b_idx) = self.borrow_set.location_map.get(&loc) {
                    state.insert(b_idx);
                }
            }
            StatementKind::StorageDead(local) | StatementKind::StorageLive(local) => {
                for (b_idx, b) in self.borrow_set.borrows.iter_enumerated() {
                    if b.borrowed_place.local == *local {
                        state.remove(b_idx);
                    }
                    if b.assigned_place.local == *local
                        && self.borrow_set.holders[b_idx].count() == 1
                    {
                        state.remove(b_idx);
                    }
                }
            }
            StatementKind::SetDiscriminant { .. } | StatementKind::Nop => {}
        }
    }

    fn apply_terminator_effect(
        &self,
        _state: &mut Self::Domain,
        _terminator: &Terminator<'tcx>,
        _loc: Location,
    ) {
    }

    fn apply_call_return_effect(
        &self,
        state: &mut Self::Domain,
        _block: BasicBlock,
        return_places: CallReturnPlaces<'_, 'tcx>,
    ) {
        let CallReturnPlaces::Call(dest) = return_places;
        if dest.projection.is_empty() {
            for (b_idx, b) in self.borrow_set.borrows.iter_enumerated() {
                if b.assigned_place.projection.is_empty()
                    && b.assigned_place.local == dest.local
                    && self.borrow_set.holders[b_idx].count() == 1
                {
                    state.remove(b_idx);
                }
            }
        }
    }
}
