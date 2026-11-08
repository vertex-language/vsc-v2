//! Building MIR from THIR (rustc's `builder/`).
//!
//! Expressions are built in one of four ways, as in rustc: into a
//! destination place (`expr.rs`, `expr_into_dest`), as a place, as an
//! operand, or as an rvalue. Patterns are tested and bound in `matches.rs`,
//! one arm after another (rustc builds a decision tree; testing arm by arm
//! gives the same results and is simpler to check).
//!
//! Integer `+`, `-` and `*` are checked for overflow, division and
//! remainder for a zero divisor, and indexing for bounds, as in a Rust
//! debug build.

mod expr;
mod matches;

pub use matches::discriminant_value;

use vsc_data_structures::FxHashMap;
use vsc_hir::HirId;
use vsc_hir::def::DefKind;
use vsc_index::IndexVec;
use vsc_middle::mir::*;
use vsc_middle::thir::{self, Thir};
use vsc_middle::ty::{self, Mutability, Ty, TyCtxt, TyKind};
use vsc_span::def_id::DefId;
use vsc_span::{Span, Symbol};

pub fn mir_built<'tcx>(tcx: TyCtxt<'tcx>, def: DefId) -> Body<'tcx> {
    let thir = tcx.thir_body(def);
    let span = tcx.def_span(def);
    let mut b = Builder {
        tcx,
        thir,
        blocks: IndexVec::new(),
        locals: IndexVec::new(),
        var_debug_info: Vec::new(),
        var_indices: FxHashMap::default(),
        breakables: Vec::new(),
        upvars: FxHashMap::default(),
    };
    let value = thir.value.expect("a body has a value");
    let ret_ty = thir.exprs[value].ty;
    b.locals.push(LocalDecl { mutability: Mutability::Mut, ty: ret_ty, span, kind: LocalKind::ReturnPointer });

    // A function literal's first argument is itself, by reference; its
    // captured variables are its fields.
    let is_closure = tcx.def_kind(def) == DefKind::Closure;
    if is_closure {
        let sig = tcx.typeck(def).closure_sigs.get(&def.expect_local()).copied();
        let closure_ty = match sig {
            Some(s) => {
                let sig_ty = tcx.mk_ty(TyKind::FnPtr(s));
                tcx.mk_ty(TyKind::Closure(def, tcx.mk_args(&[ty::GenericArg::Type(sig_ty)])))
            }
            None => tcx.types.error,
        };
        let env_ty = tcx.mk_ref(tcx.regions.re_erased, closure_ty, Mutability::Not);
        b.locals.push(LocalDecl { mutability: Mutability::Not, ty: env_ty, span, kind: LocalKind::Arg });
        for (i, c) in captures(tcx, def).into_iter().enumerate() {
            b.upvars.insert(c.var, (i, c.field_ty, c.by_ref));
        }
    }
    let first_param = b.locals.len();
    for p in &thir.params {
        b.locals.push(LocalDecl { mutability: Mutability::Not, ty: p.ty, span, kind: LocalKind::Arg });
    }
    let arg_count = b.locals.len() - 1;

    let mut block = b.new_block();
    for (i, p) in thir.params.iter().enumerate() {
        let local = Local::from_usize(first_param + i);
        let Some(pat) = &p.pat else { continue };
        match &pat.kind {
            // `x: T` is the argument itself.
            thir::PatKind::Binding { name, mutability, mode: thir::ByRef::No, var, subpattern: None, .. } => {
                b.locals[local].mutability = *mutability;
                b.var_indices.insert(*var, local);
                b.var_debug_info.push((*name, local));
            }
            _ => block = b.bind_irrefutable(block, Place::from_local(local), pat),
        }
    }
    let end = b.expr_into_dest(Place::from_local(RETURN_PLACE), block, value);
    b.terminate(end, TerminatorKind::Return, span);
    // Blocks nothing jumps to (after a `return` or `break`) still need a
    // terminator.
    for data in b.blocks.iter_mut() {
        if data.terminator.is_none() {
            data.terminator = Some(Terminator { span, kind: TerminatorKind::Unreachable });
        }
    }
    Body { source: def, basic_blocks: b.blocks, local_decls: b.locals, arg_count, var_debug_info: b.var_debug_info, span }
}

/// A variable a function literal captures.
pub struct Capture<'tcx> {
    pub var: HirId,
    /// The capture's own type: `&T` or `&mut T` by reference, `T` by value.
    pub field_ty: Ty<'tcx>,
    pub by_ref: bool,
}

/// The variables a function literal uses from enclosing bodies, in order of
/// first use, with how it captures each: by mutable reference if it
/// assigns or mutably borrows it, else by shared reference. (Exception 14
/// infers by-value captures from whether the literal escapes; that
/// analysis isn't written yet.)
pub fn captures<'tcx>(tcx: TyCtxt<'tcx>, closure: DefId) -> Vec<Capture<'tcx>> {
    use vsc_middle::thir::visit::{self, Visitor};
    struct Find<'a, 'tcx> {
        tcx: TyCtxt<'tcx>,
        thir: &'a Thir<'tcx>,
        locals: Vec<HirId>,
        found: Vec<(HirId, Ty<'tcx>, bool)>,
    }
    impl<'a, 'tcx> Find<'a, 'tcx> {
        fn note(&mut self, var: HirId, ty: Ty<'tcx>, mutable: bool) {
            if self.locals.contains(&var) {
                return;
            }
            match self.found.iter_mut().find(|(v, ..)| *v == var) {
                Some(f) => f.2 |= mutable,
                None => self.found.push((var, ty, mutable)),
            }
        }
        /// The captured variable at the base of a place expression.
        fn base_upvar(&self, mut e: thir::ExprId) -> Option<(HirId, Ty<'tcx>)> {
            loop {
                match &self.thir.exprs[e].kind {
                    thir::ExprKind::UpvarRef { var_hir_id, .. } => return Some((*var_hir_id, self.thir.exprs[e].ty)),
                    thir::ExprKind::Field { lhs, .. } | thir::ExprKind::Index { lhs, .. } => e = *lhs,
                    _ => return None,
                }
            }
        }
    }
    impl<'a, 'tcx> Visitor<'a, 'tcx> for Find<'a, 'tcx> {
        fn thir(&self) -> &'a Thir<'tcx> {
            self.thir
        }
        fn visit_pat(&mut self, p: &'a thir::Pat<'tcx>) {
            collect_pat_vars(p, &mut self.locals);
        }
        fn visit_expr(&mut self, e: &'a thir::Expr<'tcx>) {
            match &e.kind {
                thir::ExprKind::UpvarRef { var_hir_id, .. } => self.note(*var_hir_id, e.ty, false),
                thir::ExprKind::Assign { lhs, .. } | thir::ExprKind::AssignOp { lhs, .. } => {
                    if let Some((v, t)) = self.base_upvar(*lhs) {
                        self.note(v, t, true);
                    }
                }
                thir::ExprKind::Borrow { borrow_kind: thir::BorrowKind::Mut, arg } => {
                    if let Some((v, t)) = self.base_upvar(*arg) {
                        self.note(v, t, true);
                    }
                }
                thir::ExprKind::Closure { closure_id } => {
                    for c in captures(self.tcx, closure_id.to_def_id()) {
                        let (ty, mutable) = match *c.field_ty.kind() {
                            TyKind::Ref(_, t, m) if c.by_ref => (t, m.is_mut()),
                            _ => (c.field_ty, false),
                        };
                        self.note(c.var, ty, mutable);
                    }
                }
                _ => {}
            }
            visit::walk_expr(self, e);
        }
    }
    let thir = tcx.thir_body(closure);
    let mut f = Find { tcx, thir, locals: Vec::new(), found: Vec::new() };
    for p in &thir.params {
        if let Some(pat) = &p.pat {
            collect_pat_vars(pat, &mut f.locals);
        }
    }
    if let Some(v) = thir.value {
        f.visit_expr(&thir.exprs[v]);
    }
    f.found
        .into_iter()
        .map(|(var, ty, mutable)| Capture {
            var,
            field_ty: tcx.mk_ref(tcx.regions.re_erased, ty, if mutable { Mutability::Mut } else { Mutability::Not }),
            by_ref: true,
        })
        .collect()
}

fn collect_pat_vars(p: &thir::Pat<'_>, out: &mut Vec<HirId>) {
    match &p.kind {
        thir::PatKind::Binding { var, subpattern, .. } => {
            out.push(*var);
            if let Some(s) = subpattern {
                collect_pat_vars(s, out);
            }
        }
        thir::PatKind::Variant { subpatterns, .. } | thir::PatKind::Leaf { subpatterns } => {
            for s in subpatterns {
                collect_pat_vars(&s.pattern, out);
            }
        }
        thir::PatKind::Deref { subpattern } => collect_pat_vars(subpattern, out),
        thir::PatKind::Slice { prefix, slice, suffix } | thir::PatKind::Array { prefix, slice, suffix } => {
            for x in prefix.iter().chain(suffix.iter()) {
                collect_pat_vars(x, out);
            }
            if let Some(s) = slice {
                collect_pat_vars(s, out);
            }
        }
        thir::PatKind::Or { pats } => {
            for x in pats {
                collect_pat_vars(x, out);
            }
        }
        _ => {}
    }
}

struct Breakable<'tcx> {
    target: HirId,
    break_block: BasicBlock,
    continue_block: Option<BasicBlock>,
    /// Where `break value` writes.
    dest: Place<'tcx>,
}

pub(crate) struct Builder<'a, 'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub thir: &'a Thir<'tcx>,
    blocks: IndexVec<BasicBlock, BasicBlockData<'tcx>>,
    locals: IndexVec<Local, LocalDecl<'tcx>>,
    var_debug_info: Vec<(Symbol, Local)>,
    var_indices: FxHashMap<HirId, Local>,
    breakables: Vec<Breakable<'tcx>>,
    /// In a function literal: each captured variable's field, its type,
    /// and whether it's captured by reference.
    upvars: FxHashMap<HirId, (usize, Ty<'tcx>, bool)>,
}

impl<'a, 'tcx> Builder<'a, 'tcx> {
    pub fn new_block(&mut self) -> BasicBlock {
        self.blocks.push(BasicBlockData::default())
    }

    pub fn temp(&mut self, ty: Ty<'tcx>, span: Span) -> Local {
        self.locals.push(LocalDecl { mutability: Mutability::Mut, ty, span, kind: LocalKind::Temp })
    }

    pub fn push_assign(&mut self, block: BasicBlock, place: Place<'tcx>, rvalue: Rvalue<'tcx>, span: Span) {
        self.blocks[block].statements.push(Statement { span, kind: StatementKind::Assign(Box::new((place, rvalue))) });
    }

    pub fn push_unit(&mut self, block: BasicBlock, place: Place<'tcx>, span: Span) {
        let unit = self.unit_operand(span);
        self.push_assign(block, place, Rvalue::Use(unit), span);
    }

    pub fn unit_operand(&self, span: Span) -> Operand<'tcx> {
        Operand::Constant(Box::new(ConstOperand { span, ty: self.tcx.types.unit, value: ConstValue::ZeroSized }))
    }

    pub fn terminate(&mut self, block: BasicBlock, kind: TerminatorKind<'tcx>, span: Span) {
        let data = &mut self.blocks[block];
        debug_assert!(data.terminator.is_none(), "{block:?} terminated twice");
        data.terminator = Some(Terminator { span, kind });
    }

    pub fn goto(&mut self, from: BasicBlock, to: BasicBlock, span: Span) {
        self.terminate(from, TerminatorKind::Goto { target: to }, span);
    }

    /// The local for a user variable, made on first sight.
    pub fn var_local(&mut self, var: HirId, name: Symbol, mutability: Mutability, ty: Ty<'tcx>, span: Span) -> Local {
        if let Some(&l) = self.var_indices.get(&var) {
            return l;
        }
        let l = self.locals.push(LocalDecl { mutability, ty, span, kind: LocalKind::UserVar(name) });
        self.var_indices.insert(var, l);
        self.var_debug_info.push((name, l));
        l
    }

    /// The type of a place.
    pub fn place_ty(&self, p: &Place<'tcx>) -> Ty<'tcx> {
        let tcx = self.tcx;
        let mut t = self.locals[p.local].ty;
        for elem in &p.projection {
            t = match *elem {
                PlaceElem::Deref => match *t.kind() {
                    TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => inner,
                    TyKind::Adt(_, args) if !args.is_empty() => args[0].expect_ty(),
                    _ => tcx.types.error,
                },
                PlaceElem::Field(_, ty) => ty,
                PlaceElem::Index(_) | PlaceElem::ConstantIndex { .. } => match *t.kind() {
                    TyKind::Array(e, _) | TyKind::Slice(e) => e,
                    _ => tcx.types.error,
                },
                PlaceElem::Subslice { .. } => t,
                PlaceElem::Downcast(_) => t,
            };
        }
        t
    }

    fn push_breakable(&mut self, target: HirId, break_block: BasicBlock, continue_block: Option<BasicBlock>, dest: Place<'tcx>) {
        self.breakables.push(Breakable { target, break_block, continue_block, dest });
    }
}
