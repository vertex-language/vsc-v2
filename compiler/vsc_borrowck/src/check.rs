//! MIR borrow check driver and diagnostic reporter.

use vsc_data_structures::{FxHashMap, FxHashSet};
use vsc_errors::ErrCode;
use vsc_hir::LangItem;
use vsc_index::{DenseBitSet, Idx, IndexVec};
use vsc_middle::mir::*;
use vsc_middle::thir::ExprKind as ThirExprKind;
use vsc_middle::ty::{Mutability, TyCtxt, TyKind};
use vsc_mir_dataflow::framework::{
    Location, Results, ResultsCursor, iterate_to_fixpoint, predecessors,
};
use vsc_mir_dataflow::impls::{EverInitializedPlaces, LiveVariables, MaybeUninitializedPlaces};
use vsc_mir_dataflow::move_paths::{InitLocation, LookupResult, MoveData, MovePathIndex, has_deref};
use vsc_mir_dataflow::{place_ty, reverse_postorder, type_is_copy};
use vsc_span::{DUMMY_SP, Span};
use vsc_span::def_id::DefId;
use vsc_trait_selection::ParamEnv;

use crate::borrow_set::{BorrowData, BorrowIndex, BorrowSet, Borrows};
use crate::places_conflict::places_conflict;

pub fn check_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
    param_env: &ParamEnv<'tcx>,
    body: &Body<'tcx>,
) {
    let move_data = MoveData::gather_moves(body, tcx, param_env);
    let borrow_set = BorrowSet::build(tcx, body);
    let loop_resets = compute_loop_resets(tcx, def_id, body, &move_data);

    let maybe_uninit = iterate_to_fixpoint(MaybeUninitializedPlaces::new(&move_data), body);
    let ever_init =
        iterate_to_fixpoint(EverInitializedPlaces::with_block_resets(&move_data, loop_resets), body);
    let live_locals = iterate_to_fixpoint(LiveVariables, body);
    let borrows = iterate_to_fixpoint(Borrows::new(&borrow_set), body);

    let mut cx = BorrowckCtxt {
        tcx,
        param_env,
        body,
        move_data: &move_data,
        borrow_set: &borrow_set,
        uninit_cursor: ResultsCursor::new(&maybe_uninit, body),
        ever_cursor: ResultsCursor::new(&ever_init, body),
        live_cursor: ResultsCursor::new(&live_locals, body),
        borrows_cursor: ResultsCursor::new(&borrows, body),
        reported: FxHashSet::default(),
    };

    cx.check_all(body, &live_locals, &borrows);
}

struct BorrowckCtxt<'mir, 'tcx> {
    tcx: TyCtxt<'tcx>,
    param_env: &'mir ParamEnv<'tcx>,
    body: &'mir Body<'tcx>,
    move_data: &'mir MoveData<'tcx>,
    borrow_set: &'mir BorrowSet<'tcx>,
    uninit_cursor: ResultsCursor<'mir, 'tcx, MaybeUninitializedPlaces<'mir, 'tcx>>,
    ever_cursor: ResultsCursor<'mir, 'tcx, EverInitializedPlaces<'mir, 'tcx>>,
    live_cursor: ResultsCursor<'mir, 'tcx, LiveVariables>,
    borrows_cursor: ResultsCursor<'mir, 'tcx, Borrows<'mir, 'tcx>>,
    reported: FxHashSet<(u32, u32)>,
}

impl<'mir, 'tcx> BorrowckCtxt<'mir, 'tcx> {
    fn check_all(
        &mut self,
        body: &Body<'tcx>,
        _live_results: &Results<'tcx, LiveVariables>,
        _borrow_results: &Results<'tcx, Borrows<'mir, 'tcx>>,
    ) {
        let rpo = reverse_postorder(body);
        for bb in rpo {
            let data = &body.basic_blocks[bb];
            for (stmt_idx, stmt) in data.statements.iter().enumerate() {
                let loc = Location::new(bb, stmt_idx);
                self.check_statement(loc, stmt);
            }
            let term_loc = Location::new(bb, data.statements.len());
            self.check_terminator(term_loc, data.terminator());
        }
    }

    fn place_name(&self, place: &Place<'tcx>) -> String {
        match self.body.local_decls[place.local].kind {
            LocalKind::UserVar(sym) => sym.as_str().to_string(),
            LocalKind::Arg => format!("argument _{}", place.local.index()),
            LocalKind::ReturnPointer => "return value".to_string(),
            LocalKind::Temp => format!("temporary _{}", place.local.index()),
        }
    }

    fn emit_code(&mut self, span: Span, code: u32, msg: String, note: Option<String>) {
        if !self.reported.insert((code, span.lo().0)) {
            return;
        }
        let mut diag = self.tcx.dcx().struct_span_err(span, msg);
        diag.code(ErrCode(code));
        if let Some(n) = note {
            diag.note(n);
        }
        diag.emit();
    }

    fn active_borrows_at(
        &mut self,
        loc: Location,
        is_shared_access: bool,
    ) -> Vec<BorrowIndex> {
        self.borrows_cursor.seek_before_primary_effect(loc);
        self.live_cursor.seek_after_primary_effect(loc);
        let active_set = self.borrows_cursor.get().clone();
        let live_set = self.live_cursor.get().clone();

        let mut active = Vec::new();
        for b_idx in self.borrow_set.borrows.indices() {
            if !active_set.contains(b_idx) {
                continue;
            }
            let b = &self.borrow_set.borrows[b_idx];
            // Two-phase mutable borrow only conflicts with shared accesses once
            // its call site is reached.
            if is_shared_access
                && let Some(call_loc) = b.two_phase_call
                && loc != call_loc
                && b.reserve_location.block == call_loc.block
                && loc.block == call_loc.block
                && loc.statement_index < call_loc.statement_index
            {
                continue;
            }
            if holders_live(&self.borrow_set.holders[b_idx], &live_set, self.body) {
                active.push(b_idx);
            }
        }
        active
    }

    fn check_statement(&mut self, loc: Location, stmt: &Statement<'tcx>) {
        match &stmt.kind {
            StatementKind::Assign(box_pair) => {
                let (lhs, rvalue) = &**box_pair;
                self.check_rvalue(loc, rvalue, stmt.span);
                self.check_assign_lhs(loc, lhs, stmt.span);
            }
            StatementKind::SetDiscriminant { place, .. } => {
                self.check_mutability_write(place, stmt.span);
                self.check_write_conflicts(loc, place, stmt.span);
            }
            StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {}
        }
    }

    fn check_terminator(&mut self, loc: Location, term: &Terminator<'tcx>) {
        match &term.kind {
            TerminatorKind::Call { func, args, destination, .. } => {
                // `Call { func }` copies/borrows the callee without consuming it.
                self.check_operand_as_call_callee(loc, func, term.span);
                for arg in args {
                    self.check_operand(loc, arg, term.span);
                }
                self.check_assign_lhs(loc, destination, term.span);
            }
            TerminatorKind::SwitchInt { discr, .. } | TerminatorKind::Assert { cond: discr, .. } => {
                self.check_operand(loc, discr, term.span);
            }
            TerminatorKind::Return => {
                self.check_return_escaping_locals(loc, term.span);
            }
            TerminatorKind::Yield { value, resume_arg, .. } => {
                self.check_operand(loc, value, term.span);
                self.check_assign_lhs(loc, resume_arg, term.span);
            }
            TerminatorKind::Drop { .. }
            | TerminatorKind::InlineAsm { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::Unreachable => {}
        }
    }

    fn check_rvalue(&mut self, loc: Location, rvalue: &Rvalue<'tcx>, span: Span) {
        match rvalue {
            Rvalue::Use(op)
            | Rvalue::Repeat(op, _)
            | Rvalue::Cast(_, op, _)
            | Rvalue::UnaryOp(_, op)
            | Rvalue::ClassNew(op) => self.check_operand(loc, op, span),
            Rvalue::BinaryOp(_, ops) | Rvalue::CheckedBinaryOp(_, ops) => {
                self.check_operand(loc, &ops.0, span);
                self.check_operand(loc, &ops.1, span);
            }
            Rvalue::Aggregate(_, ops) => {
                for op in ops {
                    self.check_operand(loc, op, span);
                }
            }
            Rvalue::Ref(kind, place) => {
                self.check_borrow(loc, *kind, place, span);
            }
            Rvalue::RawPtr(mutability, place) => {
                let kind = if mutability.is_mut() { BorrowKind::Mut } else { BorrowKind::Shared };
                self.check_borrow_init_and_mutability(loc, kind, place, span, false);
            }
            Rvalue::Len(place) | Rvalue::Discriminant(place) => {
                self.check_place_read(loc, place, span);
            }
        }
    }

    fn check_operand(&mut self, loc: Location, op: &Operand<'tcx>, span: Span) {
        match op {
            Operand::Copy(place) => {
                self.check_place_read(loc, place, span);
            }
            Operand::Move(place) => {
                let ty = place_ty(self.tcx, self.body, place);
                if type_is_copy(self.tcx, self.param_env, ty) {
                    self.check_place_read(loc, place, span);
                } else {
                    self.check_place_move(loc, place, span);
                }
            }
            Operand::Constant(_) => {}
        }
    }

    fn check_operand_as_call_callee(&mut self, loc: Location, op: &Operand<'tcx>, span: Span) {
        match op {
            Operand::Copy(place) | Operand::Move(place) => {
                self.check_place_read(loc, place, span);
            }
            Operand::Constant(_) => {}
        }
    }

    fn check_place_initialized(&mut self, loc: Location, place: &Place<'tcx>, span: Span, verb: &str) -> bool {
        if has_deref(place) {
            // Check base up to first Deref is initialized.
            let mut prefix = Vec::new();
            for elem in &place.projection {
                if matches!(elem, PlaceElem::Deref) {
                    break;
                }
                prefix.push(*elem);
            }
            let base = Place { local: place.local, projection: prefix };
            return self.check_place_initialized(loc, &base, span, verb);
        }

        self.uninit_cursor.seek_before_primary_effect(loc);
        let uninit = self.uninit_cursor.get().clone();
        let mp = match self.move_data.lookup(place) {
            LookupResult::Exact(mp) => mp,
            LookupResult::Parent(Some(mp)) => mp,
            LookupResult::Parent(None) => return true,
        };
        if !uninit.contains(mp) {
            return true;
        }
        // Only report if a child is also uninitialized (avoid noisy self-moves on temps
        // where not initialized cleanly yet if MIR builder uses ReturnPointer/Temp partially).
        if matches!(self.body.local_decls[place.local].kind, LocalKind::Temp | LocalKind::ReturnPointer) {
            return true;
        }
        let name = self.place_name(place);
        let has_moves = !self.move_data.path_map[mp].is_empty()
            || has_descendant_move(self.move_data, mp);
        if has_moves {
            self.emit_code(
                span,
                382,
                format!("{verb} of moved value: `{name}`"),
                Some("value used here after move".to_string()),
            );
        } else {
            self.emit_code(
                span,
                381,
                format!("used binding `{name}` is possibly-uninitialized"),
                None,
            );
        }
        false
    }

    fn check_place_read(&mut self, loc: Location, place: &Place<'tcx>, span: Span) {
        self.check_place_initialized(loc, place, span, "use");
        let active = self.active_borrows_at(loc, true);
        for b_idx in active {
            let b = &self.borrow_set.borrows[b_idx];
            if b.kind == BorrowKind::Mut && places_conflict(self.tcx, self.body, &b.borrowed_place, place) {
                let name = self.place_name(place);
                let bspan = b.span;
                if !self.reported.insert((503, span.lo().0)) {
                    return;
                }
                let mut diag = self.tcx.dcx().struct_span_err(
                    span,
                    format!("cannot use `{name}` because it was mutably borrowed"),
                );
                diag.code(ErrCode(503));
                diag.span_label(bspan, format!("`{name}` is borrowed here"));
                diag.emit();
                return;
            }
        }
    }

    fn check_place_move(&mut self, loc: Location, place: &Place<'tcx>, span: Span) {
        if has_deref(place) {
            if let Some(deref_pos) = place.projection.iter().position(|e| matches!(e, PlaceElem::Deref)) {
                let base = Place {
                    local: place.local,
                    projection: place.projection[..deref_pos].to_vec(),
                };
                let ptr_ty = place_ty(self.tcx, self.body, &base);
                let is_ref = matches!(ptr_ty.kind(), TyKind::Ref(..));
                let is_class = matches!(
                    ptr_ty.kind(),
                    TyKind::Adt(adt, _)
                        if adt.is_class() || self.tcx.is_lang_item(adt.did(), LangItem::Class)
                );
                if is_ref || is_class {
                    let kind_desc = if matches!(ptr_ty.kind(), TyKind::Ref(_, _, Mutability::Mut)) {
                        "a `&mut` reference"
                    } else if is_class {
                        "a `class` reference"
                    } else {
                        "a shared reference"
                    };
                    self.emit_code(
                        span,
                        507,
                        format!("cannot move out of `{}` which is behind {kind_desc}", self.place_name(place)),
                        None,
                    );
                    return;
                }
            }
        }

        self.check_place_initialized(loc, place, span, "use");
        let active = self.active_borrows_at(loc, false);
        for b_idx in active {
            let b = &self.borrow_set.borrows[b_idx];
            if places_conflict(self.tcx, self.body, &b.borrowed_place, place) {
                let name = self.place_name(place);
                let bspan = b.span;
                if !self.reported.insert((505, span.lo().0)) {
                    return;
                }
                let mut diag = self.tcx.dcx().struct_span_err(
                    span,
                    format!("cannot move out of `{name}` because it is borrowed"),
                );
                diag.code(ErrCode(505));
                diag.span_label(bspan, format!("borrow of `{name}` occurs here"));
                diag.emit();
                return;
            }
        }
    }

    fn check_borrow(&mut self, loc: Location, kind: BorrowKind, place: &Place<'tcx>, span: Span) {
        self.check_borrow_init_and_mutability(loc, kind, place, span, true);
    }

    fn check_borrow_init_and_mutability(
        &mut self,
        loc: Location,
        kind: BorrowKind,
        place: &Place<'tcx>,
        span: Span,
        check_conflicts: bool,
    ) {
        self.check_place_initialized(loc, place, span, "borrow");

        if kind == BorrowKind::Mut {
            self.check_mutability_borrow(place, span);
        }

        if !check_conflicts {
            return;
        }

        let is_shared = kind == BorrowKind::Shared;
        let active = self.active_borrows_at(loc, is_shared);
        for b_idx in active {
            let b = &self.borrow_set.borrows[b_idx];
            if !places_conflict(self.tcx, self.body, &b.borrowed_place, place) {
                continue;
            }
            let name = self.place_name(place);
            let bspan = b.span;
            match (kind, b.kind) {
                (BorrowKind::Mut, BorrowKind::Mut) => {
                    if !self.reported.insert((499, span.lo().0)) {
                        return;
                    }
                    let mut diag = self.tcx.dcx().struct_span_err(
                        span,
                        format!("cannot borrow `{name}` as mutable more than once at a time"),
                    );
                    diag.code(ErrCode(499));
                    diag.span_label(bspan, "first mutable borrow occurs here");
                    diag.emit();
                    return;
                }
                (BorrowKind::Mut, BorrowKind::Shared) => {
                    if !self.reported.insert((502, span.lo().0)) {
                        return;
                    }
                    let mut diag = self.tcx.dcx().struct_span_err(
                        span,
                        format!(
                            "cannot borrow `{name}` as mutable because it is also borrowed as immutable"
                        ),
                    );
                    diag.code(ErrCode(502));
                    diag.span_label(bspan, "immutable borrow occurs here");
                    diag.emit();
                    return;
                }
                (BorrowKind::Shared, BorrowKind::Mut) => {
                    if !self.reported.insert((502, span.lo().0)) {
                        return;
                    }
                    let mut diag = self.tcx.dcx().struct_span_err(
                        span,
                        format!(
                            "cannot borrow `{name}` as immutable because it is also borrowed as mutable"
                        ),
                    );
                    diag.code(ErrCode(502));
                    diag.span_label(bspan, "mutable borrow occurs here");
                    diag.emit();
                    return;
                }
                (BorrowKind::Shared, BorrowKind::Shared) => {}
            }
        }
    }

    fn check_mutability_borrow(&mut self, place: &Place<'tcx>, span: Span) {
        if !has_deref(place) {
            let decl = &self.body.local_decls[place.local];
            if decl.mutability == Mutability::Not
                && matches!(decl.kind, LocalKind::UserVar(_) | LocalKind::Arg)
            {
                let name = self.place_name(place);
                self.emit_code(
                    span,
                    596,
                    format!("cannot borrow `{name}` as mutable, as it is not declared as mutable"),
                    Some("consider changing this to be mutable using `var`".to_string()),
                );
            }
            return;
        }
        self.check_deref_chain_mutability(place, span, 596, "borrow");
    }

    fn check_mutability_write(&mut self, place: &Place<'tcx>, span: Span) {
        if has_deref(place) {
            self.check_deref_chain_mutability(place, span, 594, "assign to");
        }
    }

    fn check_deref_chain_mutability(
        &mut self,
        place: &Place<'tcx>,
        span: Span,
        code: u32,
        verb: &str,
    ) {
        if let Some(last_deref) = place.projection.iter().rposition(|e| matches!(e, PlaceElem::Deref)) {
            let ptr_place = Place {
                local: place.local,
                projection: place.projection[..last_deref].to_vec(),
            };
            let ptr_ty = place_ty(self.tcx, self.body, &ptr_place);
            if matches!(ptr_ty.kind(), TyKind::Ref(_, _, Mutability::Not)) {
                let name = self.place_name(place);
                let action = if verb == "borrow" {
                    format!("cannot borrow `*{name}`, as it is behind a `&` reference")
                } else {
                    format!("cannot assign to `*{name}`, which is behind a `&` reference")
                };
                self.emit_code(span, code, action, None);
            }
        }
    }

    fn check_assign_lhs(&mut self, loc: Location, lhs: &Place<'tcx>, span: Span) {
        self.check_mutability_write(lhs, span);

        if !has_deref(lhs) {
            let decl = &self.body.local_decls[lhs.local];
            if decl.mutability == Mutability::Not
                && matches!(decl.kind, LocalKind::UserVar(_) | LocalKind::Arg)
            {
                self.ever_cursor.seek_before_primary_effect(loc);
                let ever = self.ever_cursor.get().clone();
                let root = self.move_data.local_paths[lhs.local];
                let was_ever_init = any_init_in_subtree(self.move_data, root, &ever);
                if was_ever_init {
                    let name = self.place_name(lhs);
                    if lhs.projection.is_empty() {
                        self.emit_code(
                            span,
                            384,
                            format!("cannot assign twice to immutable variable `{name}`"),
                            Some("consider making this binding mutable with `var`".to_string()),
                        );
                    } else {
                        self.emit_code(
                            span,
                            594,
                            format!("cannot assign to `{name}`, as `{name}` is not declared as mutable"),
                            Some("consider changing this to be mutable with `var`".to_string()),
                        );
                    }
                }
            }
        }

        self.check_write_conflicts(loc, lhs, span);
    }

    fn check_write_conflicts(&mut self, loc: Location, place: &Place<'tcx>, span: Span) {
        let active = self.active_borrows_at(loc, false);
        for b_idx in active {
            let b = &self.borrow_set.borrows[b_idx];
            if places_conflict(self.tcx, self.body, &b.borrowed_place, place) {
                let name = self.place_name(place);
                let bspan = b.span;
                if !self.reported.insert((506, span.lo().0)) {
                    return;
                }
                let mut diag = self.tcx.dcx().struct_span_err(
                    span,
                    format!("cannot assign to `{name}` because it is borrowed"),
                );
                diag.code(ErrCode(506));
                diag.span_label(bspan, format!("`{name}` is borrowed here"));
                diag.emit();
                return;
            }
        }
    }

    fn check_return_escaping_locals(&mut self, loc: Location, span: Span) {
        self.borrows_cursor.seek_before_primary_effect(loc);
        let active_set = self.borrows_cursor.get().clone();
        let ret_borrows = &self.borrow_set.local_held_borrows[RETURN_PLACE];

        for b_idx in self.borrow_set.borrows.indices() {
            if !active_set.contains(b_idx) || !ret_borrows.contains(b_idx) {
                continue;
            }
            let b = &self.borrow_set.borrows[b_idx];
            if self.borrow_originates_from_local_frame(b, &active_set) {
                let name = self.place_name(&b.borrowed_place);
                let is_arg = matches!(self.body.local_decls[b.borrowed_place.local].kind, LocalKind::Arg);
                let kind_str = if is_arg { "function parameter" } else { "local variable" };
                self.emit_code(
                    span,
                    515,
                    format!("cannot return value referencing {kind_str} `{name}`"),
                    Some(format!("returns a value referencing data owned by the current function")),
                );
                return;
            }
        }
    }

    fn borrow_originates_from_local_frame(
        &self,
        b: &BorrowData<'tcx>,
        active_set: &DenseBitSet<BorrowIndex>,
    ) -> bool {
        let bp = &b.borrowed_place;
        if !has_deref(bp) {
            return true;
        }
        // Check if dereferenced base is a `Class` (heap RC allocation) or an outer
        // reference whose origin is an argument (lives outside the frame).
        let deref_pos = bp.projection.iter().position(|e| matches!(e, PlaceElem::Deref)).unwrap();
        let base_place = Place {
            local: bp.local,
            projection: bp.projection[..deref_pos].to_vec(),
        };
        let base_ty = place_ty(self.tcx, self.body, &base_place);
        if matches!(
            base_ty.kind(),
            TyKind::Adt(adt, _)
                if adt.is_class() || self.tcx.is_lang_item(adt.did(), LangItem::Class)
        ) {
            return false;
        }
        if matches!(base_ty.kind(), TyKind::RawPtr(..)) {
            return false;
        }
        // Reborrow through a reference `base_place`: check whether `base_place.local`
        // itself may hold any active local-frame borrow.
        let held = &self.borrow_set.local_held_borrows[base_place.local];
        for inner_idx in self.borrow_set.borrows.indices() {
            if held.contains(inner_idx) && active_set.contains(inner_idx) {
                let inner = &self.borrow_set.borrows[inner_idx];
                if inner.reserve_location < b.reserve_location
                    && self.borrow_originates_from_local_frame(inner, active_set)
                {
                    return true;
                }
            }
        }
        false
    }
}

fn holders_live(holders: &DenseBitSet<Local>, live: &DenseBitSet<Local>, body: &Body<'_>) -> bool {
    for idx in 0..body.local_decls.len() {
        let l = Local::from_usize(idx);
        if holders.contains(l) && live.contains(l) {
            return true;
        }
    }
    false
}

fn has_descendant_move(move_data: &MoveData<'_>, root: MovePathIndex) -> bool {
    let mut found = false;
    move_data.for_each_child_inclusive(root, |mp| {
        if !move_data.path_map[mp].is_empty() {
            found = true;
        }
    });
    found
}

fn any_init_in_subtree(
    move_data: &MoveData<'_>,
    root: MovePathIndex,
    ever: &DenseBitSet<vsc_mir_dataflow::InitIndex>,
) -> bool {
    let mut found = false;
    move_data.for_each_child_inclusive(root, |mp| {
        for &init_idx in &move_data.init_path_map[mp] {
            if matches!(move_data.inits[init_idx].location, InitLocation::Argument(_))
                || ever.contains(init_idx)
            {
                found = true;
            }
        }
    });
    found
}

/// Identifies `UserVar` locals scoped within each natural loop so `EverInitializedPlaces`
/// treats each loop iteration as a fresh binding lifetime (matching `StorageLive` in loops).
fn compute_loop_resets<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
    body: &Body<'tcx>,
    move_data: &MoveData<'tcx>,
) -> IndexVec<BasicBlock, Vec<Local>> {
    let n = body.basic_blocks.len();
    let mut resets = IndexVec::from_raw(vec![Vec::new(); n]);
    if n == 0 {
        return resets;
    }

    let thir = tcx.thir_body(def_id);
    let loop_spans: Vec<Span> = thir
        .exprs
        .iter()
        .filter_map(|e| {
            matches!(e.kind, ThirExprKind::Loop { .. })
                .then_some(e.span)
                .filter(|&s| s != DUMMY_SP)
        })
        .collect();
    if loop_spans.is_empty() {
        return resets;
    }

    let rpo = reverse_postorder(body);
    let mut reachable = DenseBitSet::new_empty(n);
    for &bb in &rpo {
        reachable.insert(bb);
    }
    let preds = predecessors(body);
    let doms = compute_dominators(n, &rpo, &reachable, &preds);

    let mut loops: FxHashMap<BasicBlock, (Vec<BasicBlock>, DenseBitSet<BasicBlock>)> =
        FxHashMap::default();
    for &pred in &rpo {
        for succ in body.basic_blocks[pred].terminator().kind.successors() {
            if !doms[pred].contains(succ) {
                continue;
            }
            let (back_preds, nodes) = loops
                .entry(succ)
                .or_insert_with(|| (Vec::new(), DenseBitSet::new_empty(n)));
            back_preds.push(pred);
            collect_natural_loop(succ, pred, &reachable, &preds, nodes);
        }
    }

    for (header, (back_preds, loop_nodes)) in loops {
        let mut required_spans = Vec::with_capacity(back_preds.len() + 1);
        let head_span = body.basic_blocks[header].terminator().span;
        if head_span != DUMMY_SP {
            required_spans.push(head_span);
        }
        for bp in back_preds {
            let bp_span = body.basic_blocks[bp].terminator().span;
            if bp_span != DUMMY_SP {
                required_spans.push(bp_span);
            }
        }
        let Some(loop_span) = loop_spans
            .iter()
            .copied()
            .filter(|&cand| required_spans.iter().all(|&req| span_contains(cand, req)))
            .min_by_key(|s| s.hi().0.saturating_sub(s.lo().0))
        else {
            continue;
        };

        for local in body.local_decls.indices() {
            let decl = &body.local_decls[local];
            if !matches!(decl.kind, LocalKind::UserVar(_))
                || decl.span == DUMMY_SP
                || !span_contains(loop_span, decl.span)
            {
                continue;
            }
            let mut all_in_loop = true;
            let root = move_data.local_paths[local];
            move_data.for_each_child_inclusive(root, |mp| {
                for &init_idx in &move_data.init_path_map[mp] {
                    match move_data.inits[init_idx].location {
                        InitLocation::Statement(loc) if loop_nodes.contains(loc.block) => {}
                        _ => all_in_loop = false,
                    }
                }
            });
            if all_in_loop {
                resets[header].push(local);
            }
        }
    }

    resets
}

fn compute_dominators(
    n: usize,
    rpo: &[BasicBlock],
    reachable: &DenseBitSet<BasicBlock>,
    preds: &IndexVec<BasicBlock, Vec<BasicBlock>>,
) -> IndexVec<BasicBlock, DenseBitSet<BasicBlock>> {
    let mut doms = IndexVec::from_raw(vec![DenseBitSet::new_empty(n); n]);
    for &bb in rpo {
        if bb == START_BLOCK {
            doms[bb].insert(bb);
        } else {
            doms[bb] = reachable.clone();
        }
    }
    let mut changed = true;
    while changed {
        changed = false;
        for &bb in rpo {
            if bb == START_BLOCK {
                continue;
            }
            let mut next: Option<DenseBitSet<BasicBlock>> = None;
            for &p in &preds[bb] {
                if !reachable.contains(p) {
                    continue;
                }
                match next {
                    None => next = Some(doms[p].clone()),
                    Some(ref mut acc) => {
                        acc.intersect(&doms[p]);
                    }
                }
            }
            let mut new_dom = next.unwrap_or_else(|| DenseBitSet::new_empty(n));
            new_dom.insert(bb);
            if new_dom != doms[bb] {
                doms[bb] = new_dom;
                changed = true;
            }
        }
    }
    doms
}

fn collect_natural_loop(
    header: BasicBlock,
    pred: BasicBlock,
    reachable: &DenseBitSet<BasicBlock>,
    preds: &IndexVec<BasicBlock, Vec<BasicBlock>>,
    nodes: &mut DenseBitSet<BasicBlock>,
) {
    nodes.insert(header);
    if !nodes.insert(pred) {
        return;
    }
    let mut worklist = vec![pred];
    while let Some(cur) = worklist.pop() {
        for &p in &preds[cur] {
            if reachable.contains(p) && nodes.insert(p) {
                worklist.push(p);
            }
        }
    }
}

fn span_contains(outer: Span, inner: Span) -> bool {
    outer.lo() <= inner.lo() && inner.hi() <= outer.hi()
}
