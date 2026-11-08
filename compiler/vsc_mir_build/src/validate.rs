//! Checking that MIR is well formed (rustc's `-Z validate-mir`): every
//! block ends in a terminator whose targets exist, every local exists, an
//! assignment's value has its place's type, a call's destination has the
//! callee's return type, and switches are on integers.

use vsc_index::Idx;
use vsc_middle::mir::*;
use vsc_middle::ty::{self, Ty, TyCtxt, TyKind};

pub fn validate<'tcx>(tcx: TyCtxt<'tcx>, body: &Body<'tcx>) -> Vec<String> {
    let mut v = Validator { tcx, body, errors: Vec::new() };
    v.check();
    v.errors
}

struct Validator<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    body: &'a Body<'tcx>,
    errors: Vec<String>,
}

/// Types equal for MIR's purposes: regions are erased anyway, and an error
/// type matches anything.
fn same(a: Ty<'_>, b: Ty<'_>) -> bool {
    if a == b || matches!(a.kind(), TyKind::Error) || matches!(b.kind(), TyKind::Error) {
        return true;
    }
    match (*a.kind(), *b.kind()) {
        (TyKind::Ref(_, x, m1), TyKind::Ref(_, y, m2)) => m1 == m2 && same(x, y),
        (TyKind::Adt(d1, a1), TyKind::Adt(d2, a2)) => d1 == d2 && args_same(a1, a2),
        (TyKind::Tuple(x), TyKind::Tuple(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| same(*a, *b)),
        (TyKind::Array(x, _), TyKind::Array(y, _)) | (TyKind::Slice(x), TyKind::Slice(y)) => same(x, y),
        (TyKind::RawPtr(x, m1), TyKind::RawPtr(y, m2)) => m1 == m2 && same(x, y),
        (TyKind::FnDef(d1, a1), TyKind::FnDef(d2, a2)) | (TyKind::Closure(d1, a1), TyKind::Closure(d2, a2)) => d1 == d2 && args_same(a1, a2),
        _ => false,
    }
}

fn args_same(a: ty::GenericArgsRef<'_>, b: ty::GenericArgsRef<'_>) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(x, y)| match (x, y) {
            (ty::GenericArg::Type(x), ty::GenericArg::Type(y)) => same(*x, *y),
            _ => true,
        })
}

impl<'a, 'tcx> Validator<'a, 'tcx> {
    fn err(&mut self, msg: String) {
        self.errors.push(msg);
    }

    fn local_ok(&mut self, l: Local, at: &str) -> bool {
        if l.index() >= self.body.local_decls.len() {
            self.err(format!("{at}: {l:?} doesn't exist"));
            return false;
        }
        true
    }

    fn place_ty(&mut self, p: &Place<'tcx>, at: &str) -> Ty<'tcx> {
        let tcx = self.tcx;
        if !self.local_ok(p.local, at) {
            return tcx.types.error;
        }
        let mut t = self.body.local_decls[p.local].ty;
        let mut variant = None;
        for elem in &p.projection {
            t = match *elem {
                PlaceElem::Deref => match *t.kind() {
                    TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => inner,
                    // Through a class value to its struct (exception 3).
                    TyKind::Adt(d, args) if tcx.is_lang_item(d.did(), vsc_hir::LangItem::Class) => args[0].expect_ty(),
                    TyKind::Error => t,
                    _ => {
                        self.err(format!("{at}: dereference of `{}`", tcx.ty_to_string(t)));
                        tcx.types.error
                    }
                },
                PlaceElem::Field(i, fty) => {
                    let expected = match *t.kind() {
                        TyKind::Tuple(tys) => tys.get(i).copied(),
                        TyKind::Adt(adt, args) => {
                            let v = &adt.variants()[variant.unwrap_or(0)];
                            v.fields.get(i).map(|f| tcx.erase_regions(tcx.instantiate(tcx.type_of(f.did), args)))
                        }
                        TyKind::Closure(..) => Some(fty),
                        TyKind::Error => Some(fty),
                        _ => None,
                    };
                    match expected {
                        Some(e) if same(e, fty) => {}
                        Some(e) => self.err(format!("{at}: field {i} has type `{}`, not `{}`", tcx.ty_to_string(e), tcx.ty_to_string(fty))),
                        None => self.err(format!("{at}: no field {i} on `{}`", tcx.ty_to_string(t))),
                    }
                    variant = None;
                    fty
                }
                PlaceElem::Index(l) => {
                    self.local_ok(l, at);
                    match *t.kind() {
                        TyKind::Array(e, _) | TyKind::Slice(e) => e,
                        _ => {
                            self.err(format!("{at}: indexing `{}`", tcx.ty_to_string(t)));
                            tcx.types.error
                        }
                    }
                }
                PlaceElem::ConstantIndex { .. } => match *t.kind() {
                    TyKind::Array(e, _) | TyKind::Slice(e) => e,
                    _ => tcx.types.error,
                },
                PlaceElem::Subslice { .. } => t,
                PlaceElem::Downcast(v) => {
                    variant = Some(v);
                    t
                }
            };
        }
        t
    }

    fn operand_ty(&mut self, o: &Operand<'tcx>, at: &str) -> Ty<'tcx> {
        match o {
            Operand::Copy(p) | Operand::Move(p) => self.place_ty(p, at),
            Operand::Constant(c) => c.ty,
        }
    }

    fn rvalue_ty(&mut self, r: &Rvalue<'tcx>, expected: Ty<'tcx>, at: &str) -> Ty<'tcx> {
        let tcx = self.tcx;
        match r {
            Rvalue::Use(o) => self.operand_ty(o, at),
            Rvalue::Repeat(o, n) => {
                let t = self.operand_ty(o, at);
                tcx.mk_ty(TyKind::Array(t, *n))
            }
            Rvalue::Ref(k, p) => {
                let t = self.place_ty(p, at);
                tcx.mk_ref(tcx.regions.re_erased, t, if *k == BorrowKind::Mut { ty::Mutability::Mut } else { ty::Mutability::Not })
            }
            Rvalue::RawPtr(m, p) => {
                let t = self.place_ty(p, at);
                tcx.mk_ty(TyKind::RawPtr(t, *m))
            }
            Rvalue::Len(p) => {
                self.place_ty(p, at);
                tcx.types.usize
            }
            Rvalue::Cast(_, o, t) => {
                self.operand_ty(o, at);
                *t
            }
            Rvalue::BinaryOp(op, b) => {
                let l = self.operand_ty(&b.0, at);
                self.operand_ty(&b.1, at);
                if op.is_comparison() { tcx.types.bool } else { l }
            }
            Rvalue::CheckedBinaryOp(_, b) => {
                let l = self.operand_ty(&b.0, at);
                self.operand_ty(&b.1, at);
                tcx.mk_tup(&[l, tcx.types.bool])
            }
            Rvalue::UnaryOp(_, o) => self.operand_ty(o, at),
            Rvalue::Discriminant(p) => {
                self.place_ty(p, at);
                tcx.types.isize
            }
            Rvalue::ClassNew(o) => {
                self.operand_ty(o, at);
                expected
            }
            Rvalue::Aggregate(kind, ops) => {
                let tys: Vec<Ty<'tcx>> = ops.iter().map(|o| self.operand_ty(o, at)).collect();
                match &**kind {
                    AggregateKind::Tuple => tcx.mk_tup(&tys),
                    AggregateKind::Array(e) => match *expected.kind() {
                        TyKind::Array(_, n) => tcx.mk_ty(TyKind::Array(*e, n)),
                        _ => expected,
                    },
                    AggregateKind::Adt { def_id, variant_index, args, active_field } => {
                        let adt = tcx.adt_def(*def_id);
                        let v = &adt.variants()[*variant_index];
                        let fields: Vec<usize> = match active_field {
                            Some(f) => vec![*f],
                            None => (0..v.fields.len()).collect(),
                        };
                        if fields.len() != tys.len() {
                            self.err(format!("{at}: {} operands for {} fields", tys.len(), fields.len()));
                        }
                        for (i, t) in fields.iter().zip(&tys) {
                            let ft = tcx.erase_regions(tcx.instantiate(tcx.type_of(v.fields[*i].did), args));
                            if !same(ft, *t) {
                                self.err(format!("{at}: field {i} given `{}`, wants `{}`", tcx.ty_to_string(*t), tcx.ty_to_string(ft)));
                            }
                        }
                        tcx.mk_adt(adt, args)
                    }
                    AggregateKind::Closure(d, args) => tcx.mk_ty(TyKind::Closure(*d, args)),
                }
            }
        }
    }

    fn check(&mut self) {
        let tcx = self.tcx;
        let n = self.body.basic_blocks.len();
        for (bb, data) in self.body.basic_blocks.iter_enumerated() {
            for (i, st) in data.statements.iter().enumerate() {
                let at = format!("{bb:?}[{i}]");
                if let StatementKind::Assign(a) = &st.kind {
                    let pt = self.place_ty(&a.0, &at);
                    let rt = self.rvalue_ty(&a.1, pt, &at);
                    if !same(pt, rt) && !matches!(rt.kind(), TyKind::Never) {
                        self.err(format!("{at}: assigning `{}` to a place of type `{}`", tcx.ty_to_string(rt), tcx.ty_to_string(pt)));
                    }
                }
            }
            let at = format!("{bb:?} terminator");
            let Some(t) = &data.terminator else {
                self.err(format!("{bb:?} has no terminator"));
                continue;
            };
            for s in t.kind.successors() {
                if s.index() >= n {
                    self.err(format!("{at}: jumps to missing {s:?}"));
                }
            }
            match &t.kind {
                TerminatorKind::SwitchInt { discr, .. } => {
                    let d = self.operand_ty(discr, &at);
                    if !matches!(d.kind(), TyKind::Bool | TyKind::Char | TyKind::Int(_) | TyKind::Uint(_) | TyKind::Error) {
                        self.err(format!("{at}: switch on `{}`", tcx.ty_to_string(d)));
                    }
                }
                TerminatorKind::Call { func, args, destination, .. } => {
                    let f = self.operand_ty(func, &at);
                    for a in args {
                        self.operand_ty(a, &at);
                    }
                    let dt = self.place_ty(destination, &at);
                    let out = match *f.kind() {
                        TyKind::FnDef(d, fargs) => Some(tcx.erase_regions(tcx.instantiate(tcx.fn_sig(d).skip_binder().output(), fargs))),
                        TyKind::FnPtr(s) => Some(s.value.output()),
                        _ => None,
                    };
                    if let Some(out) = out {
                        // A normalized projection may differ from the
                        // signature's alias; only plain mismatches count.
                        if !same(out, dt) && !matches!(out.kind(), TyKind::Alias(_) | TyKind::Param(_)) && !contains_alias(out) {
                            self.err(format!("{at}: call returns `{}` into a place of type `{}`", tcx.ty_to_string(out), tcx.ty_to_string(dt)));
                        }
                    }
                }
                TerminatorKind::Assert { cond, .. } => {
                    let c = self.operand_ty(cond, &at);
                    if !matches!(c.kind(), TyKind::Bool) {
                        self.err(format!("{at}: assert on `{}`", tcx.ty_to_string(c)));
                    }
                }
                _ => {}
            }
        }
    }
}

fn contains_alias(t: Ty<'_>) -> bool {
    let mut found = false;
    fn walk(t: Ty<'_>, found: &mut bool) {
        match *t.kind() {
            TyKind::Alias(_) | TyKind::Param(_) => *found = true,
            TyKind::Adt(_, args) => args.iter().for_each(|a| {
                if let ty::GenericArg::Type(t) = a {
                    walk(*t, found)
                }
            }),
            TyKind::Ref(_, t, _) | TyKind::Slice(t) | TyKind::Array(t, _) | TyKind::RawPtr(t, _) => walk(t, found),
            TyKind::Tuple(ts) => ts.iter().for_each(|t| walk(*t, found)),
            _ => {}
        }
    }
    walk(t, &mut found);
    found
}
