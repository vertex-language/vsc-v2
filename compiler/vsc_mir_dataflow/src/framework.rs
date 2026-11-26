//! Generic forward/backward fixpoint solver and result cursor over MIR CFGs.

use std::collections::VecDeque;

use vsc_index::{DenseBitSet, Idx, IndexVec};
use vsc_middle::mir::*;

/// A point inside a MIR [`Body`]: either statement `statement_index` or
/// (when `statement_index == block.statements.len()`) the block's terminator.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Location {
    pub block: BasicBlock,
    pub statement_index: usize,
}

impl Location {
    pub const START: Location = Location { block: START_BLOCK, statement_index: 0 };

    pub fn new(block: BasicBlock, statement_index: usize) -> Self {
        Location { block, statement_index }
    }

    pub fn successor_within_block(self) -> Location {
        Location { block: self.block, statement_index: self.statement_index + 1 }
    }
}

pub trait JoinSemiLattice: Eq {
    fn join(&mut self, other: &Self) -> bool;
}

impl<T: Idx> JoinSemiLattice for DenseBitSet<T> {
    fn join(&mut self, other: &Self) -> bool {
        self.union(other)
    }
}

pub trait GenKill<T> {
    fn gen_elem(&mut self, elem: T);
    fn kill_elem(&mut self, elem: T);
}

impl<T: Idx> GenKill<T> for DenseBitSet<T> {
    fn gen_elem(&mut self, elem: T) {
        self.insert(elem);
    }

    fn kill_elem(&mut self, elem: T) {
        self.remove(elem);
    }
}

pub trait Direction {
    const IS_FORWARD: bool;
}

pub struct Forward;
impl Direction for Forward {
    const IS_FORWARD: bool = true;
}

pub struct Backward;
impl Direction for Backward {
    const IS_FORWARD: bool = false;
}

pub enum CallReturnPlaces<'a, 'tcx> {
    Call(&'a Place<'tcx>),
}

pub trait Analysis<'tcx> {
    type Domain: Clone + JoinSemiLattice + Eq;
    type Direction: Direction;
    const NAME: &'static str;

    fn bottom_value(&self, body: &Body<'tcx>) -> Self::Domain;

    fn initialize_start_block(&self, body: &Body<'tcx>, state: &mut Self::Domain);

    fn apply_block_start_effect(&self, _state: &mut Self::Domain, _block: BasicBlock) {}

    fn apply_before_statement_effect(
        &self,
        _state: &mut Self::Domain,
        _statement: &Statement<'tcx>,
        _location: Location,
    ) {
    }

    fn apply_statement_effect(
        &self,
        state: &mut Self::Domain,
        statement: &Statement<'tcx>,
        location: Location,
    );

    fn apply_before_terminator_effect(
        &self,
        _state: &mut Self::Domain,
        _terminator: &Terminator<'tcx>,
        _location: Location,
    ) {
    }

    fn apply_terminator_effect(
        &self,
        state: &mut Self::Domain,
        terminator: &Terminator<'tcx>,
        location: Location,
    );

    fn apply_call_return_effect(
        &self,
        _state: &mut Self::Domain,
        _block: BasicBlock,
        _return_places: CallReturnPlaces<'_, 'tcx>,
    ) {
    }
}

/// Computes reachable basic blocks from [`START_BLOCK`] in reverse postorder.
pub fn reverse_postorder(body: &Body<'_>) -> Vec<BasicBlock> {
    let n = body.basic_blocks.len();
    if n == 0 {
        return Vec::new();
    }
    let mut visited = DenseBitSet::<BasicBlock>::new_empty(n);
    let mut postorder = Vec::with_capacity(n);

    // Explicit stack to avoid blowing the host stack on huge generated CFGs.
    let mut stack: Vec<(BasicBlock, Vec<BasicBlock>, usize)> = Vec::new();
    visited.insert(START_BLOCK);
    let succs = body.basic_blocks[START_BLOCK].terminator().kind.successors();
    stack.push((START_BLOCK, succs, 0));

    while let Some((bb, succs, idx)) = stack.last_mut() {
        if *idx < succs.len() {
            let next = succs[*idx];
            *idx += 1;
            if visited.insert(next) {
                let next_succs = body.basic_blocks[next].terminator().kind.successors();
                stack.push((next, next_succs, 0));
            }
        } else {
            postorder.push(*bb);
            stack.pop();
        }
    }

    postorder.reverse();
    postorder
}

pub fn predecessors(body: &Body<'_>) -> IndexVec<BasicBlock, Vec<BasicBlock>> {
    let mut preds = IndexVec::from_raw(vec![Vec::new(); body.basic_blocks.len()]);
    for (bb, data) in body.basic_blocks.iter_enumerated() {
        for succ in data.terminator().kind.successors() {
            preds[succ].push(bb);
        }
    }
    preds
}

pub struct Results<'tcx, A: Analysis<'tcx>> {
    pub analysis: A,
    /// For forward analyses: state at entry to each block.
    /// For backward analyses: state at exit from each block (after terminator).
    pub block_states: IndexVec<BasicBlock, A::Domain>,
    pub reachable: DenseBitSet<BasicBlock>,
}

pub fn iterate_to_fixpoint<'tcx, A: Analysis<'tcx>>(
    analysis: A,
    body: &Body<'tcx>,
) -> Results<'tcx, A> {
    let n = body.basic_blocks.len();
    let rpo = reverse_postorder(body);
    let mut reachable = DenseBitSet::new_empty(n);
    for &bb in &rpo {
        reachable.insert(bb);
    }

    let bottom = analysis.bottom_value(body);
    let mut block_states = IndexVec::from_raw(vec![bottom.clone(); n]);

    if A::Direction::IS_FORWARD {
        if n > 0 {
            analysis.initialize_start_block(body, &mut block_states[START_BLOCK]);
        }

        let mut in_worklist = DenseBitSet::new_empty(n);
        let mut worklist = VecDeque::with_capacity(rpo.len());
        for &bb in &rpo {
            worklist.push_back(bb);
            in_worklist.insert(bb);
        }

        while let Some(bb) = worklist.pop_front() {
            in_worklist.remove(bb);
            let mut state = block_states[bb].clone();
            analysis.apply_block_start_effect(&mut state, bb);
            let data = &body.basic_blocks[bb];
            for (stmt_idx, stmt) in data.statements.iter().enumerate() {
                let loc = Location::new(bb, stmt_idx);
                analysis.apply_before_statement_effect(&mut state, stmt, loc);
                analysis.apply_statement_effect(&mut state, stmt, loc);
            }
            let term_loc = Location::new(bb, data.statements.len());
            let term = data.terminator();
            analysis.apply_before_terminator_effect(&mut state, term, term_loc);
            analysis.apply_terminator_effect(&mut state, term, term_loc);

            match &term.kind {
                TerminatorKind::Call { destination, target, .. } => {
                    if let Some(t) = *target {
                        let mut edge_state = state;
                        analysis.apply_call_return_effect(
                            &mut edge_state,
                            bb,
                            CallReturnPlaces::Call(destination),
                        );
                        if block_states[t].join(&edge_state) && in_worklist.insert(t) {
                            worklist.push_back(t);
                        }
                    }
                }
                _ => {
                    for succ in term.kind.successors() {
                        if block_states[succ].join(&state) && in_worklist.insert(succ) {
                            worklist.push_back(succ);
                        }
                    }
                }
            }
        }
    } else {
        let preds = predecessors(body);
        for &bb in &rpo {
            let term = body.basic_blocks[bb].terminator();
            if term.kind.successors().is_empty() {
                analysis.initialize_start_block(body, &mut block_states[bb]);
            }
        }

        let mut in_worklist = DenseBitSet::new_empty(n);
        let mut worklist = VecDeque::with_capacity(rpo.len());
        for &bb in rpo.iter().rev() {
            worklist.push_back(bb);
            in_worklist.insert(bb);
        }

        while let Some(bb) = worklist.pop_front() {
            in_worklist.remove(bb);
            let mut state = block_states[bb].clone();
            analysis.apply_block_start_effect(&mut state, bb);
            let data = &body.basic_blocks[bb];
            let term = data.terminator();
            let term_loc = Location::new(bb, data.statements.len());
            if let TerminatorKind::Call { destination, target: Some(_), .. } = &term.kind {
                analysis.apply_call_return_effect(
                    &mut state,
                    bb,
                    CallReturnPlaces::Call(destination),
                );
            }
            analysis.apply_terminator_effect(&mut state, term, term_loc);
            analysis.apply_before_terminator_effect(&mut state, term, term_loc);

            for (stmt_idx, stmt) in data.statements.iter().enumerate().rev() {
                let loc = Location::new(bb, stmt_idx);
                analysis.apply_statement_effect(&mut state, stmt, loc);
                analysis.apply_before_statement_effect(&mut state, stmt, loc);
            }

            for &pred in &preds[bb] {
                if !reachable.contains(pred) {
                    continue;
                }
                if block_states[pred].join(&state) && in_worklist.insert(pred) {
                    worklist.push_back(pred);
                }
            }
        }
    }

    Results { analysis, block_states, reachable }
}

pub struct ResultsCursor<'mir, 'tcx, A: Analysis<'tcx>> {
    results: &'mir Results<'tcx, A>,
    body: &'mir Body<'tcx>,
    state: A::Domain,
    pos: Option<Location>,
}

impl<'mir, 'tcx, A: Analysis<'tcx>> ResultsCursor<'mir, 'tcx, A> {
    pub fn new(results: &'mir Results<'tcx, A>, body: &'mir Body<'tcx>) -> Self {
        let bottom = results.analysis.bottom_value(body);
        ResultsCursor { results, body, state: bottom, pos: None }
    }

    pub fn analysis(&self) -> &A {
        &self.results.analysis
    }

    pub fn get(&self) -> &A::Domain {
        &self.state
    }

    pub fn seek_to_block_start(&mut self, block: BasicBlock) {
        if A::Direction::IS_FORWARD {
            self.state = self.results.block_states[block].clone();
            self.results.analysis.apply_block_start_effect(&mut self.state, block);
            self.pos = Some(Location::new(block, 0));
        } else {
            let mut state = self.results.block_states[block].clone();
            let data = &self.body.basic_blocks[block];
            let term = data.terminator();
            let term_loc = Location::new(block, data.statements.len());
            if let TerminatorKind::Call { destination, target: Some(_), .. } = &term.kind {
                self.results.analysis.apply_call_return_effect(
                    &mut state,
                    block,
                    CallReturnPlaces::Call(destination),
                );
            }
            self.results.analysis.apply_terminator_effect(&mut state, term, term_loc);
            self.results.analysis.apply_before_terminator_effect(&mut state, term, term_loc);
            for (i, s) in data.statements.iter().enumerate().rev() {
                let loc = Location::new(block, i);
                self.results.analysis.apply_statement_effect(&mut state, s, loc);
                self.results.analysis.apply_before_statement_effect(&mut state, s, loc);
            }
            self.state = state;
            self.pos = Some(Location::new(block, 0));
        }
    }

    pub fn seek_before_primary_effect(&mut self, loc: Location) {
        self.seek(loc, false);
    }

    pub fn seek_after_primary_effect(&mut self, loc: Location) {
        self.seek(loc, true);
    }

    fn seek(&mut self, loc: Location, after: bool) {
        if A::Direction::IS_FORWARD {
            let need_reset = self.pos.is_none_or(|p| {
                p.block != loc.block
                    || p.statement_index > loc.statement_index
                    || (!after && p.statement_index == loc.statement_index)
            });
            if need_reset {
                self.seek_to_block_start(loc.block);
            }
            let cur = self.pos.unwrap().statement_index;
            let data = &self.body.basic_blocks[loc.block];
            for i in cur..loc.statement_index {
                let l = Location::new(loc.block, i);
                let stmt = &data.statements[i];
                self.results.analysis.apply_before_statement_effect(&mut self.state, stmt, l);
                self.results.analysis.apply_statement_effect(&mut self.state, stmt, l);
            }
            if loc.statement_index < data.statements.len() {
                let stmt = &data.statements[loc.statement_index];
                self.results.analysis.apply_before_statement_effect(&mut self.state, stmt, loc);
                if after {
                    self.results.analysis.apply_statement_effect(&mut self.state, stmt, loc);
                    self.pos = Some(loc.successor_within_block());
                } else {
                    self.pos = Some(loc);
                }
            } else {
                let term = data.terminator();
                self.results.analysis.apply_before_terminator_effect(&mut self.state, term, loc);
                if after {
                    self.results.analysis.apply_terminator_effect(&mut self.state, term, loc);
                }
                self.pos = None;
            }
        } else {
            let data = &self.body.basic_blocks[loc.block];
            let mut state = self.results.block_states[loc.block].clone();
            let term = data.terminator();
            let term_loc = Location::new(loc.block, data.statements.len());
            if let TerminatorKind::Call { destination, target: Some(_), .. } = &term.kind {
                self.results.analysis.apply_call_return_effect(
                    &mut state,
                    loc.block,
                    CallReturnPlaces::Call(destination),
                );
            }
            if loc.statement_index == data.statements.len() {
                if after {
                    self.results.analysis.apply_terminator_effect(&mut state, term, term_loc);
                }
                self.results.analysis.apply_before_terminator_effect(&mut state, term, term_loc);
                self.state = state;
                self.pos = None;
                return;
            }
            self.results.analysis.apply_terminator_effect(&mut state, term, term_loc);
            self.results.analysis.apply_before_terminator_effect(&mut state, term, term_loc);
            for i in (loc.statement_index + 1..data.statements.len()).rev() {
                let l = Location::new(loc.block, i);
                let stmt = &data.statements[i];
                self.results.analysis.apply_statement_effect(&mut state, stmt, l);
                self.results.analysis.apply_before_statement_effect(&mut state, stmt, l);
            }
            let stmt = &data.statements[loc.statement_index];
            if after {
                self.results.analysis.apply_statement_effect(&mut state, stmt, loc);
            }
            self.results.analysis.apply_before_statement_effect(&mut state, stmt, loc);
            self.state = state;
            self.pos = None;
        }
    }
}

pub trait ResultsVisitor<'mir, 'tcx, A: Analysis<'tcx>> {
    fn visit_block_start(&mut self, _state: &A::Domain) {}

    fn visit_statement_before_primary_effect(
        &mut self,
        _results: &Results<'tcx, A>,
        _state: &A::Domain,
        _statement: &'mir Statement<'tcx>,
        _location: Location,
    ) {
    }

    fn visit_statement_after_primary_effect(
        &mut self,
        _results: &Results<'tcx, A>,
        _state: &A::Domain,
        _statement: &'mir Statement<'tcx>,
        _location: Location,
    ) {
    }

    fn visit_terminator_before_primary_effect(
        &mut self,
        _results: &Results<'tcx, A>,
        _state: &A::Domain,
        _terminator: &'mir Terminator<'tcx>,
        _location: Location,
    ) {
    }

    fn visit_terminator_after_primary_effect(
        &mut self,
        _results: &Results<'tcx, A>,
        _state: &A::Domain,
        _terminator: &'mir Terminator<'tcx>,
        _location: Location,
    ) {
    }
}

pub fn visit_results<'mir, 'tcx, A: Analysis<'tcx>>(
    body: &'mir Body<'tcx>,
    blocks: impl IntoIterator<Item = BasicBlock>,
    results: &Results<'tcx, A>,
    visitor: &mut impl ResultsVisitor<'mir, 'tcx, A>,
) {
    if A::Direction::IS_FORWARD {
        for bb in blocks {
            if !results.reachable.contains(bb) {
                continue;
            }
            let data = &body.basic_blocks[bb];
            let mut state = results.block_states[bb].clone();
            visitor.visit_block_start(&state);
            for (idx, stmt) in data.statements.iter().enumerate() {
                let loc = Location::new(bb, idx);
                results.analysis.apply_before_statement_effect(&mut state, stmt, loc);
                visitor.visit_statement_before_primary_effect(results, &state, stmt, loc);
                results.analysis.apply_statement_effect(&mut state, stmt, loc);
                visitor.visit_statement_after_primary_effect(results, &state, stmt, loc);
            }
            let term = data.terminator();
            let term_loc = Location::new(bb, data.statements.len());
            results.analysis.apply_before_terminator_effect(&mut state, term, term_loc);
            visitor.visit_terminator_before_primary_effect(results, &state, term, term_loc);
            results.analysis.apply_terminator_effect(&mut state, term, term_loc);
            visitor.visit_terminator_after_primary_effect(results, &state, term, term_loc);
        }
    } else {
        for bb in blocks {
            if !results.reachable.contains(bb) {
                continue;
            }
            let data = &body.basic_blocks[bb];
            let mut state = results.block_states[bb].clone();
            let term = data.terminator();
            let term_loc = Location::new(bb, data.statements.len());
            if let TerminatorKind::Call { destination, target: Some(_), .. } = &term.kind {
                results.analysis.apply_call_return_effect(
                    &mut state,
                    bb,
                    CallReturnPlaces::Call(destination),
                );
            }
            visitor.visit_terminator_before_primary_effect(results, &state, term, term_loc);
            results.analysis.apply_terminator_effect(&mut state, term, term_loc);
            visitor.visit_terminator_after_primary_effect(results, &state, term, term_loc);
            for (idx, stmt) in data.statements.iter().enumerate().rev() {
                let loc = Location::new(bb, idx);
                visitor.visit_statement_before_primary_effect(results, &state, stmt, loc);
                results.analysis.apply_statement_effect(&mut state, stmt, loc);
                visitor.visit_statement_after_primary_effect(results, &state, stmt, loc);
            }
            visitor.visit_block_start(&state);
        }
    }
}
