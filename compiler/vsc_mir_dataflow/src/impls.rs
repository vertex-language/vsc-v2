//! Standard MIR dataflow analyses: initialization, deinitialization, and liveness.

use vsc_index::{DenseBitSet, IndexVec};
use vsc_middle::mir::*;

use crate::framework::{
    Analysis, Backward, CallReturnPlaces, Forward, Location,
};
use crate::move_paths::{InitIndex, LookupResult, MoveData, MovePathIndex, has_deref};

/// Forward analysis tracking which [`MovePathIndex`]es may be initialized.
pub struct MaybeInitializedPlaces<'a, 'tcx> {
    move_data: &'a MoveData<'tcx>,
}

impl<'a, 'tcx> MaybeInitializedPlaces<'a, 'tcx> {
    pub fn new(move_data: &'a MoveData<'tcx>) -> Self {
        MaybeInitializedPlaces { move_data }
    }

    fn kill_with_descendants(&self, state: &mut DenseBitSet<MovePathIndex>, mp: MovePathIndex) {
        self.move_data.for_each_child_inclusive(mp, |child| {
            state.remove(child);
        });
    }

    fn gen_with_descendants(&self, state: &mut DenseBitSet<MovePathIndex>, mp: MovePathIndex) {
        self.move_data.for_each_child_inclusive(mp, |child| {
            state.insert(child);
        });
        // Mark ancestor paths as maybe-initialized so parent inspection works.
        let mut cur = self.move_data.move_paths[mp].parent;
        while let Some(p) = cur {
            state.insert(p);
            cur = self.move_data.move_paths[p].parent;
        }
    }

    fn apply_moves(&self, state: &mut DenseBitSet<MovePathIndex>, loc: Location) {
        if let Some(moves) = self.move_data.loc_map.get(&loc) {
            for &mo in moves {
                let mp = self.move_data.moves[mo].path;
                self.kill_with_descendants(state, mp);
            }
        }
    }

    fn apply_inits(&self, state: &mut DenseBitSet<MovePathIndex>, loc: Location) {
        if let Some(inits) = self.move_data.init_loc_map.get(&loc) {
            for &init in inits {
                let mp = self.move_data.inits[init].path;
                self.gen_with_descendants(state, mp);
            }
        }
    }
}

impl<'a, 'tcx> Analysis<'tcx> for MaybeInitializedPlaces<'a, 'tcx> {
    type Domain = DenseBitSet<MovePathIndex>;
    type Direction = Forward;
    const NAME: &'static str = "maybe_init";

    fn bottom_value(&self, _body: &Body<'tcx>) -> Self::Domain {
        DenseBitSet::new_empty(self.move_data.move_paths.len())
    }

    fn initialize_start_block(&self, body: &Body<'tcx>, state: &mut Self::Domain) {
        for arg in body.args_iter() {
            let mp = self.move_data.local_paths[arg];
            self.gen_with_descendants(state, mp);
        }
    }

    fn apply_statement_effect(
        &self,
        state: &mut Self::Domain,
        stmt: &Statement<'tcx>,
        loc: Location,
    ) {
        self.apply_moves(state, loc);
        match stmt.kind {
            StatementKind::StorageLive(l) | StatementKind::StorageDead(l) => {
                self.kill_with_descendants(state, self.move_data.local_paths[l]);
            }
            StatementKind::Assign(..) | StatementKind::SetDiscriminant { .. } | StatementKind::Nop => {
                self.apply_inits(state, loc);
            }
        }
    }

    fn apply_terminator_effect(
        &self,
        state: &mut Self::Domain,
        terminator: &Terminator<'tcx>,
        loc: Location,
    ) {
        self.apply_moves(state, loc);
        if let TerminatorKind::Drop { place, .. } = &terminator.kind {
            if !has_deref(place) {
                if let LookupResult::Exact(mp) = self.move_data.lookup(place) {
                    self.kill_with_descendants(state, mp);
                }
            }
        }
    }

    fn apply_call_return_effect(
        &self,
        state: &mut Self::Domain,
        block: BasicBlock,
        return_places: CallReturnPlaces<'_, 'tcx>,
    ) {
        let CallReturnPlaces::Call(dest) = return_places;
        if !has_deref(dest) {
            let _ = block;
            match self.move_data.lookup(dest) {
                LookupResult::Exact(mp) => self.gen_with_descendants(state, mp),
                LookupResult::Parent(Some(mp)) => self.gen_with_descendants(state, mp),
                LookupResult::Parent(None) => {}
            }
        }
    }
}

/// Forward analysis tracking which [`MovePathIndex`]es may be uninitialized
/// (or partially moved out).
pub struct MaybeUninitializedPlaces<'a, 'tcx> {
    move_data: &'a MoveData<'tcx>,
}

impl<'a, 'tcx> MaybeUninitializedPlaces<'a, 'tcx> {
    pub fn new(move_data: &'a MoveData<'tcx>) -> Self {
        MaybeUninitializedPlaces { move_data }
    }

    fn set_uninit_with_descendants_and_ancestors(
        &self,
        state: &mut DenseBitSet<MovePathIndex>,
        mp: MovePathIndex,
    ) {
        self.move_data.for_each_child_inclusive(mp, |child| {
            state.insert(child);
        });
        let mut cur = self.move_data.move_paths[mp].parent;
        while let Some(p) = cur {
            state.insert(p);
            cur = self.move_data.move_paths[p].parent;
        }
    }

    fn clear_init_with_descendants(
        &self,
        state: &mut DenseBitSet<MovePathIndex>,
        mp: MovePathIndex,
    ) {
        self.move_data.for_each_child_inclusive(mp, |child| {
            state.remove(child);
        });
        // If every child of an ancestor is now initialized, clear the ancestor too.
        let mut cur = self.move_data.move_paths[mp].parent;
        while let Some(p) = cur {
            let mut all_init = true;
            let mut child = self.move_data.move_paths[p].first_child;
            while let Some(c) = child {
                if state.contains(c) {
                    all_init = false;
                    break;
                }
                child = self.move_data.move_paths[c].next_sibling;
            }
            if all_init {
                state.remove(p);
                cur = self.move_data.move_paths[p].parent;
            } else {
                break;
            }
        }
    }

    fn apply_moves(&self, state: &mut DenseBitSet<MovePathIndex>, loc: Location) {
        if let Some(moves) = self.move_data.loc_map.get(&loc) {
            for &mo in moves {
                let mp = self.move_data.moves[mo].path;
                self.set_uninit_with_descendants_and_ancestors(state, mp);
            }
        }
    }

    fn apply_inits(&self, state: &mut DenseBitSet<MovePathIndex>, loc: Location) {
        if let Some(inits) = self.move_data.init_loc_map.get(&loc) {
            for &init in inits {
                let mp = self.move_data.inits[init].path;
                self.clear_init_with_descendants(state, mp);
            }
        }
    }
}

impl<'a, 'tcx> Analysis<'tcx> for MaybeUninitializedPlaces<'a, 'tcx> {
    type Domain = DenseBitSet<MovePathIndex>;
    type Direction = Forward;
    const NAME: &'static str = "maybe_uninit";

    fn bottom_value(&self, _body: &Body<'tcx>) -> Self::Domain {
        DenseBitSet::new_empty(self.move_data.move_paths.len())
    }

    fn initialize_start_block(&self, body: &Body<'tcx>, state: &mut Self::Domain) {
        for i in 0..self.move_data.move_paths.len() {
            state.insert(MovePathIndex::from_usize(i));
        }
        for arg in body.args_iter() {
            let mp = self.move_data.local_paths[arg];
            self.clear_init_with_descendants(state, mp);
        }
    }

    fn apply_statement_effect(
        &self,
        state: &mut Self::Domain,
        stmt: &Statement<'tcx>,
        loc: Location,
    ) {
        self.apply_moves(state, loc);
        match stmt.kind {
            StatementKind::StorageLive(l) | StatementKind::StorageDead(l) => {
                self.set_uninit_with_descendants_and_ancestors(
                    state,
                    self.move_data.local_paths[l],
                );
            }
            StatementKind::Assign(..) | StatementKind::SetDiscriminant { .. } | StatementKind::Nop => {
                self.apply_inits(state, loc);
            }
        }
    }

    fn apply_terminator_effect(
        &self,
        state: &mut Self::Domain,
        terminator: &Terminator<'tcx>,
        loc: Location,
    ) {
        self.apply_moves(state, loc);
        if let TerminatorKind::Drop { place, .. } = &terminator.kind {
            if !has_deref(place) {
                if let LookupResult::Exact(mp) = self.move_data.lookup(place) {
                    self.set_uninit_with_descendants_and_ancestors(state, mp);
                }
            }
        }
    }

    fn apply_call_return_effect(
        &self,
        state: &mut Self::Domain,
        _block: BasicBlock,
        return_places: CallReturnPlaces<'_, 'tcx>,
    ) {
        let CallReturnPlaces::Call(dest) = return_places;
        if !has_deref(dest) {
            if let LookupResult::Exact(mp) = self.move_data.lookup(dest) {
                self.clear_init_with_descendants(state, mp);
            }
        }
    }
}

/// Forward analysis tracking [`InitIndex`]es that may have occurred on any path
/// reaching the current point (used to forbid double assignment to immutable locals).
pub struct EverInitializedPlaces<'a, 'tcx> {
    move_data: &'a MoveData<'tcx>,
    block_resets: Option<IndexVec<BasicBlock, Vec<Local>>>,
}

impl<'a, 'tcx> EverInitializedPlaces<'a, 'tcx> {
    pub fn new(move_data: &'a MoveData<'tcx>) -> Self {
        EverInitializedPlaces { move_data, block_resets: None }
    }

    pub fn with_block_resets(
        move_data: &'a MoveData<'tcx>,
        block_resets: IndexVec<BasicBlock, Vec<Local>>,
    ) -> Self {
        EverInitializedPlaces { move_data, block_resets: Some(block_resets) }
    }

    fn kill_for_local(&self, state: &mut DenseBitSet<InitIndex>, local: Local) {
        let root = self.move_data.local_paths[local];
        self.move_data.for_each_child_inclusive(root, |mp| {
            for &init in &self.move_data.init_path_map[mp] {
                state.remove(init);
            }
        });
    }
}

impl<'a, 'tcx> Analysis<'tcx> for EverInitializedPlaces<'a, 'tcx> {
    type Domain = DenseBitSet<InitIndex>;
    type Direction = Forward;
    const NAME: &'static str = "ever_init";

    fn bottom_value(&self, _body: &Body<'tcx>) -> Self::Domain {
        DenseBitSet::new_empty(self.move_data.inits.len())
    }

    fn initialize_start_block(&self, _body: &Body<'tcx>, state: &mut Self::Domain) {
        for (idx, init) in self.move_data.inits.iter_enumerated() {
            if matches!(init.location, crate::move_paths::InitLocation::Argument(_)) {
                state.insert(idx);
            }
        }
    }

    fn apply_block_start_effect(&self, state: &mut Self::Domain, block: BasicBlock) {
        if let Some(ref resets) = self.block_resets {
            for &local in &resets[block] {
                self.kill_for_local(state, local);
            }
        }
    }

    fn apply_statement_effect(
        &self,
        state: &mut Self::Domain,
        stmt: &Statement<'tcx>,
        loc: Location,
    ) {
        if let StatementKind::StorageLive(l) = stmt.kind {
            self.kill_for_local(state, l);
        }
        if let Some(inits) = self.move_data.init_loc_map.get(&loc) {
            for &init in inits {
                state.insert(init);
            }
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
        block: BasicBlock,
        _return_places: CallReturnPlaces<'_, 'tcx>,
    ) {
        let _ = block;
        let _ = state;
    }
}

/// Backward liveness analysis over MIR [`Local`]s.
pub struct LiveVariables;

impl LiveVariables {
    fn mark_place_uses(state: &mut DenseBitSet<Local>, place: &Place<'_>, is_write: bool) {
        if !is_write || !place.projection.is_empty() {
            state.insert(place.local);
        }
        for elem in &place.projection {
            if let PlaceElem::Index(idx) = *elem {
                state.insert(idx);
            }
        }
    }

    fn mark_operand(state: &mut DenseBitSet<Local>, op: &Operand<'_>) {
        if let Some(p) = op.place() {
            Self::mark_place_uses(state, p, false);
        }
    }

    fn mark_rvalue(state: &mut DenseBitSet<Local>, rv: &Rvalue<'_>) {
        match rv {
            Rvalue::Use(op)
            | Rvalue::Repeat(op, _)
            | Rvalue::Cast(_, op, _)
            | Rvalue::UnaryOp(_, op)
            | Rvalue::ClassNew(op) => Self::mark_operand(state, op),
            Rvalue::Ref(_, p) | Rvalue::RawPtr(_, p) | Rvalue::Len(p) | Rvalue::Discriminant(p) => {
                Self::mark_place_uses(state, p, false);
            }
            Rvalue::BinaryOp(_, ops) | Rvalue::CheckedBinaryOp(_, ops) => {
                Self::mark_operand(state, &ops.0);
                Self::mark_operand(state, &ops.1);
            }
            Rvalue::Aggregate(_, ops) => {
                for op in ops {
                    Self::mark_operand(state, op);
                }
            }
        }
    }
}

impl<'tcx> Analysis<'tcx> for LiveVariables {
    type Domain = DenseBitSet<Local>;
    type Direction = Backward;
    const NAME: &'static str = "live_variables";

    fn bottom_value(&self, body: &Body<'tcx>) -> Self::Domain {
        DenseBitSet::new_empty(body.local_decls.len())
    }

    fn initialize_start_block(&self, _body: &Body<'tcx>, _state: &mut Self::Domain) {}

    fn apply_statement_effect(
        &self,
        state: &mut Self::Domain,
        stmt: &Statement<'tcx>,
        _loc: Location,
    ) {
        match &stmt.kind {
            StatementKind::Assign(box_pair) => {
                let (lhs, rv) = &**box_pair;
                if lhs.projection.is_empty() {
                    state.remove(lhs.local);
                } else {
                    Self::mark_place_uses(state, lhs, true);
                }
                Self::mark_rvalue(state, rv);
            }
            StatementKind::SetDiscriminant { place, .. } => {
                Self::mark_place_uses(state, place, true);
            }
            StatementKind::StorageLive(l) | StatementKind::StorageDead(l) => {
                state.remove(*l);
            }
            StatementKind::Nop => {}
        }
    }

    fn apply_terminator_effect(
        &self,
        state: &mut Self::Domain,
        terminator: &Terminator<'tcx>,
        _loc: Location,
    ) {
        match &terminator.kind {
            TerminatorKind::Return => {
                state.insert(RETURN_PLACE);
            }
            TerminatorKind::SwitchInt { discr, .. } | TerminatorKind::Assert { cond: discr, .. } => {
                Self::mark_operand(state, discr);
            }
            TerminatorKind::Drop { place, .. } => {
                Self::mark_place_uses(state, place, false);
            }
            TerminatorKind::Call { func, args, destination, .. } => {
                if destination.projection.is_empty() {
                    state.remove(destination.local);
                } else {
                    Self::mark_place_uses(state, destination, true);
                }
                Self::mark_operand(state, func);
                for arg in args {
                    Self::mark_operand(state, arg);
                }
            }
            TerminatorKind::InlineAsm { operands, .. } => {
                for op in operands {
                    Self::mark_operand(state, op);
                }
            }
            TerminatorKind::Yield { value, .. } => {
                Self::mark_operand(state, value);
            }
            TerminatorKind::Goto { .. } | TerminatorKind::Unreachable => {}
        }
    }

    fn apply_call_return_effect(
        &self,
        state: &mut Self::Domain,
        _block: BasicBlock,
        return_places: CallReturnPlaces<'_, 'tcx>,
    ) {
        let CallReturnPlaces::Call(dest) = return_places;
        if dest.projection.is_empty() {
            state.remove(dest.local);
        } else {
            Self::mark_place_uses(state, dest, true);
        }
    }
}
