//! Move path construction and `Copy` evaluation (rustc's `rustc_mir_dataflow::move_paths`).

use vsc_data_structures::FxHashMap;
use vsc_hir::LangItem;
use vsc_index::IndexVec;
use vsc_infer::InferCtxt;
use vsc_middle::mir::*;
use vsc_middle::ty::{AdtKind, GenericArg, Mutability, TraitRef, Ty, TyCtxt, TyKind};
use vsc_span::{DUMMY_SP, Span};
use vsc_trait_selection::{ObligationKind, ParamEnv, Selection, SelectionCtxt};

use crate::framework::Location;

vsc_index::newtype_index! {
    pub struct MovePathIndex { DEBUG_FORMAT = "mp{}" }
}
vsc_index::newtype_index! {
    pub struct MoveOutIndex { DEBUG_FORMAT = "mo{}" }
}
vsc_index::newtype_index! {
    pub struct InitIndex { DEBUG_FORMAT = "in{}" }
}

/// Returns whether `ty` has copy semantics under `param_env`.
///
/// Mirrors Rust's `Copy` rules plus Vertex's reference-counted handle types
/// (`Class<T>`, `class` types, `Func<F>`).
pub fn type_is_copy<'tcx>(tcx: TyCtxt<'tcx>, param_env: &ParamEnv<'tcx>, ty: Ty<'tcx>) -> bool {
    let ty = tcx.erase_regions(ty);
    match *ty.kind() {
        TyKind::Bool
        | TyKind::Char
        | TyKind::Int(_)
        | TyKind::Uint(_)
        | TyKind::Float(_)
        | TyKind::RawPtr(..)
        | TyKind::FnDef(..)
        | TyKind::FnPtr(_)
        | TyKind::Never
        | TyKind::Error => return true,
        TyKind::Ref(_, _, Mutability::Not) => return true,
        TyKind::Ref(_, _, Mutability::Mut) => return false,
        TyKind::Tuple(tys) => return tys.iter().all(|&t| type_is_copy(tcx, param_env, t)),
        TyKind::Array(elem, _) => return type_is_copy(tcx, param_env, elem),
        TyKind::Adt(adt, _)
            if adt.is_class()
                || tcx.is_lang_item(adt.did(), LangItem::Class)
                || tcx.is_lang_item(adt.did(), LangItem::Func) =>
        {
            return true;
        }
        TyKind::Adt(..) | TyKind::Param(_) | TyKind::Alias(_) | TyKind::Closure(..) => {}
        TyKind::Str
        | TyKind::Slice(_)
        | TyKind::Foreign(_)
        | TyKind::Dynamic(..)
        | TyKind::Coroutine(..)
        | TyKind::Opaque(..)
        | TyKind::Infer(_) => {
            return false;
        }
    }

    if let Some(copy_trait) = tcx.lang_items().get(LangItem::Copy) {
        let infcx = InferCtxt::new(tcx);
        return satisfies_copy(tcx, &infcx, param_env, copy_trait, ty, 0);
    }
    false
}

fn satisfies_copy<'tcx>(
    tcx: TyCtxt<'tcx>,
    infcx: &InferCtxt<'tcx>,
    param_env: &ParamEnv<'tcx>,
    copy_trait: vsc_span::def_id::DefId,
    ty: Ty<'tcx>,
    depth: usize,
) -> bool {
    if depth > 16 {
        return false;
    }
    let selcx = SelectionCtxt::new(infcx, param_env);
    let tr = TraitRef { def_id: copy_trait, args: tcx.mk_args(&[GenericArg::Type(ty)]) };
    match selcx.select(tr, DUMMY_SP) {
        Selection::Selected(_, nested) => nested.into_iter().all(|ob| match ob.kind {
            ObligationKind::Trait(sub_tr) if sub_tr.def_id == copy_trait => {
                let sub_ty = sub_tr.self_ty();
                type_is_copy(tcx, param_env, sub_ty)
                    || satisfies_copy(tcx, infcx, param_env, copy_trait, sub_ty, depth + 1)
            }
            ObligationKind::Trait(sub_tr) => {
                matches!(selcx.select(sub_tr, DUMMY_SP), Selection::Selected(..))
            }
            ObligationKind::Projection(..) => true,
        }),
        Selection::Ambiguous | Selection::NoSolution => false,
    }
}

/// Computes the MIR type of `place` inside `body`.
pub fn place_ty<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>, place: &Place<'tcx>) -> Ty<'tcx> {
    let mut ty = body.local_decls[place.local].ty;
    let mut active_variant: Option<usize> = None;

    for elem in &place.projection {
        match *elem {
            PlaceElem::Deref => {
                active_variant = None;
                ty = match *ty.kind() {
                    TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => inner,
                    TyKind::Adt(adt, args)
                        if adt.is_class() || tcx.is_lang_item(adt.did(), LangItem::Class) =>
                    {
                        if adt.is_class() {
                            ty
                        } else {
                            args.first().map(|a| a.expect_ty()).unwrap_or(tcx.types.error)
                        }
                    }
                    _ => tcx.types.error,
                };
            }
            PlaceElem::Downcast(v) => {
                active_variant = Some(v);
            }
            PlaceElem::Field(idx, fty) => {
                let resolved = match *ty.kind() {
                    TyKind::Adt(adt, args) => {
                        let vi = active_variant.unwrap_or(0);
                        adt.variants()
                            .get(vi)
                            .and_then(|v| v.fields.get(idx))
                            .map(|f| f.ty(tcx, args))
                            .unwrap_or(fty)
                    }
                    TyKind::Tuple(elems) => elems.get(idx).copied().unwrap_or(fty),
                    TyKind::Closure(def_id, _) => {
                        tcx.closure_upvars(def_id).get(idx).copied().unwrap_or(fty)
                    }
                    _ => fty,
                };
                ty = resolved;
                active_variant = None;
            }
            PlaceElem::Index(_) | PlaceElem::ConstantIndex { .. } => {
                active_variant = None;
                ty = match *ty.kind() {
                    TyKind::Array(elem, _) | TyKind::Slice(elem) => elem,
                    _ => tcx.types.error,
                };
            }
            PlaceElem::Subslice { .. } => {
                active_variant = None;
                if let TyKind::Array(elem, _) | TyKind::Slice(elem) = *ty.kind() {
                    ty = tcx.mk_ty(TyKind::Slice(elem));
                }
            }
        }
    }
    ty
}

#[derive(Clone, Debug)]
pub struct MovePath<'tcx> {
    pub place: Place<'tcx>,
    pub parent: Option<MovePathIndex>,
    pub first_child: Option<MovePathIndex>,
    pub next_sibling: Option<MovePathIndex>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MoveOut {
    pub path: MovePathIndex,
    pub source: Location,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitKind {
    Deep,
    NonPanicPathOnly,
    Shallow,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InitLocation {
    Argument(Local),
    Statement(Location),
}

#[derive(Clone, Copy, Debug)]
pub struct Init {
    pub path: MovePathIndex,
    pub kind: InitKind,
    pub location: InitLocation,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LookupResult {
    Exact(MovePathIndex),
    Parent(Option<MovePathIndex>),
}

pub struct MoveData<'tcx> {
    pub move_paths: IndexVec<MovePathIndex, MovePath<'tcx>>,
    pub local_paths: IndexVec<Local, MovePathIndex>,
    pub moves: IndexVec<MoveOutIndex, MoveOut>,
    pub loc_map: FxHashMap<Location, Vec<MoveOutIndex>>,
    pub path_map: IndexVec<MovePathIndex, Vec<MoveOutIndex>>,
    pub inits: IndexVec<InitIndex, Init>,
    pub init_loc_map: FxHashMap<Location, Vec<InitIndex>>,
    pub init_path_map: IndexVec<MovePathIndex, Vec<InitIndex>>,
    child_lookup: FxHashMap<(MovePathIndex, PlaceKey), MovePathIndex>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum PlaceKey {
    Field(usize),
    Downcast(usize),
}

impl<'tcx> MoveData<'tcx> {
    pub fn gather_moves(
        body: &Body<'tcx>,
        tcx: TyCtxt<'tcx>,
        param_env: &ParamEnv<'tcx>,
    ) -> MoveData<'tcx> {
        let mut move_paths = IndexVec::new();
        let mut local_paths = IndexVec::with_capacity(body.local_decls.len());
        let mut path_map = IndexVec::new();
        let mut init_path_map = IndexVec::new();

        for (local_idx, _) in body.local_decls.iter().enumerate() {
            let local = Local::from_usize(local_idx);
            let mp = move_paths.push(MovePath {
                place: Place::from_local(local),
                parent: None,
                first_child: None,
                next_sibling: None,
            });
            local_paths.push(mp);
            path_map.push(Vec::new());
            init_path_map.push(Vec::new());
        }

        let mut data = MoveData {
            move_paths,
            local_paths,
            moves: IndexVec::new(),
            loc_map: FxHashMap::default(),
            path_map,
            inits: IndexVec::new(),
            init_loc_map: FxHashMap::default(),
            init_path_map,
            child_lookup: FxHashMap::default(),
        };

        for arg in body.args_iter() {
            let mp = data.local_paths[arg];
            let init_idx = data.inits.push(Init {
                path: mp,
                kind: InitKind::Deep,
                location: InitLocation::Argument(arg),
                span: body.local_decls[arg].span,
            });
            data.init_path_map[mp].push(init_idx);
        }

        for (bb, block_data) in body.basic_blocks.iter_enumerated() {
            for (stmt_idx, stmt) in block_data.statements.iter().enumerate() {
                let loc = Location::new(bb, stmt_idx);
                match &stmt.kind {
                    StatementKind::Assign(box_pair) => {
                        let (lhs, rvalue) = &**box_pair;
                        data.scan_rvalue(tcx, param_env, body, loc, rvalue);
                        if !has_deref(lhs) {
                            let mp = data.intern_place(lhs);
                            data.record_init(mp, InitKind::Deep, loc, stmt.span);
                        }
                    }
                    StatementKind::StorageLive(_)
                    | StatementKind::StorageDead(_)
                    | StatementKind::SetDiscriminant { .. }
                    | StatementKind::Nop => {}
                }
            }
            let term_loc = Location::new(bb, block_data.statements.len());
            let term = block_data.terminator();
            match &term.kind {
                TerminatorKind::Call { func: _, args, destination, .. } => {
                    // `Call { func }` reads the callee by Copy/borrow rather than moving it out.
                    for arg in args {
                        data.scan_operand(tcx, param_env, body, term_loc, arg);
                    }
                    if !has_deref(destination) {
                        let mp = data.intern_place(destination);
                        data.record_init(mp, InitKind::Deep, term_loc, term.span);
                    }
                }
                TerminatorKind::Drop { place, .. } => {
                    if !has_deref(place) {
                        let mp = data.intern_place(place);
                        data.record_move(mp, term_loc);
                    }
                }
                TerminatorKind::SwitchInt { discr, .. } => {
                    data.scan_operand(tcx, param_env, body, term_loc, discr);
                }
                TerminatorKind::Assert { cond, .. } => {
                    data.scan_operand(tcx, param_env, body, term_loc, cond);
                }
                TerminatorKind::InlineAsm { operands, .. } => {
                    for op in operands {
                        data.scan_operand(tcx, param_env, body, term_loc, op);
                    }
                }
                TerminatorKind::Yield { value, resume_arg, .. } => {
                    data.scan_operand(tcx, param_env, body, term_loc, value);
                    if !has_deref(resume_arg) {
                        let mp = data.intern_place(resume_arg);
                        data.record_init(mp, InitKind::Deep, term_loc, term.span);
                    }
                }
                TerminatorKind::Goto { .. }
                | TerminatorKind::Return
                | TerminatorKind::Unreachable => {}
            }
        }

        data
    }

    fn intern_place(&mut self, place: &Place<'tcx>) -> MovePathIndex {
        let mut cur = self.local_paths[place.local];
        let mut prefix_proj = Vec::new();
        for elem in &place.projection {
            let key = match *elem {
                PlaceElem::Field(i, _) => PlaceKey::Field(i),
                PlaceElem::Downcast(v) => PlaceKey::Downcast(v),
                PlaceElem::Deref
                | PlaceElem::Index(_)
                | PlaceElem::ConstantIndex { .. }
                | PlaceElem::Subslice { .. } => break,
            };
            prefix_proj.push(*elem);
            if let Some(&child) = self.child_lookup.get(&(cur, key)) {
                cur = child;
            } else {
                let sibling = self.move_paths[cur].first_child;
                let new_mp = self.move_paths.push(MovePath {
                    place: Place { local: place.local, projection: prefix_proj.clone() },
                    parent: Some(cur),
                    first_child: None,
                    next_sibling: sibling,
                });
                self.path_map.push(Vec::new());
                self.init_path_map.push(Vec::new());
                self.move_paths[cur].first_child = Some(new_mp);
                self.child_lookup.insert((cur, key), new_mp);
                cur = new_mp;
            }
        }
        cur
    }

    pub fn lookup(&self, place: &Place<'tcx>) -> LookupResult {
        let mut cur = self.local_paths[place.local];
        for elem in &place.projection {
            let key = match *elem {
                PlaceElem::Field(i, _) => PlaceKey::Field(i),
                PlaceElem::Downcast(v) => PlaceKey::Downcast(v),
                PlaceElem::Deref
                | PlaceElem::Index(_)
                | PlaceElem::ConstantIndex { .. }
                | PlaceElem::Subslice { .. } => return LookupResult::Parent(Some(cur)),
            };
            if let Some(&child) = self.child_lookup.get(&(cur, key)) {
                cur = child;
            } else {
                return LookupResult::Parent(Some(cur));
            }
        }
        LookupResult::Exact(cur)
    }

    fn scan_rvalue(
        &mut self,
        tcx: TyCtxt<'tcx>,
        param_env: &ParamEnv<'tcx>,
        body: &Body<'tcx>,
        loc: Location,
        rvalue: &Rvalue<'tcx>,
    ) {
        match rvalue {
            Rvalue::Use(op)
            | Rvalue::Repeat(op, _)
            | Rvalue::Cast(_, op, _)
            | Rvalue::UnaryOp(_, op)
            | Rvalue::ClassNew(op) => self.scan_operand(tcx, param_env, body, loc, op),
            Rvalue::BinaryOp(_, box_ops) | Rvalue::CheckedBinaryOp(_, box_ops) => {
                self.scan_operand(tcx, param_env, body, loc, &box_ops.0);
                self.scan_operand(tcx, param_env, body, loc, &box_ops.1);
            }
            Rvalue::Aggregate(_, ops) => {
                for op in ops {
                    self.scan_operand(tcx, param_env, body, loc, op);
                }
            }
            Rvalue::Ref(..)
            | Rvalue::RawPtr(..)
            | Rvalue::Len(_)
            | Rvalue::Discriminant(_) => {}
        }
    }

    fn scan_operand(
        &mut self,
        tcx: TyCtxt<'tcx>,
        param_env: &ParamEnv<'tcx>,
        body: &Body<'tcx>,
        loc: Location,
        op: &Operand<'tcx>,
    ) {
        if let Operand::Move(place) = op {
            if has_deref(place) {
                return;
            }
            let ty = place_ty(tcx, body, place);
            if !type_is_copy(tcx, param_env, ty) {
                let mp = self.intern_place(place);
                self.record_move(mp, loc);
            }
        }
    }

    fn record_move(&mut self, path: MovePathIndex, source: Location) {
        let idx = self.moves.push(MoveOut { path, source });
        self.loc_map.entry(source).or_default().push(idx);
        self.path_map[path].push(idx);
    }

    fn record_init(&mut self, path: MovePathIndex, kind: InitKind, loc: Location, span: Span) {
        let idx = self.inits.push(Init { path, kind, location: InitLocation::Statement(loc), span });
        self.init_loc_map.entry(loc).or_default().push(idx);
        self.init_path_map[path].push(idx);
    }

    pub fn for_each_child_inclusive(
        &self,
        root: MovePathIndex,
        mut f: impl FnMut(MovePathIndex),
    ) {
        let mut stack = vec![root];
        while let Some(mp) = stack.pop() {
            f(mp);
            let mut child = self.move_paths[mp].first_child;
            while let Some(c) = child {
                stack.push(c);
                child = self.move_paths[c].next_sibling;
            }
        }
    }

    pub fn is_union_root(&self, tcx: TyCtxt<'tcx>, body: &Body<'tcx>, mp: MovePathIndex) -> bool {
        let ty = place_ty(tcx, body, &self.move_paths[mp].place);
        matches!(ty.kind(), TyKind::Adt(adt, _) if adt.kind() == AdtKind::Union)
    }
}

pub fn has_deref(place: &Place<'_>) -> bool {
    place.projection.iter().any(|p| matches!(p, PlaceElem::Deref))
}
