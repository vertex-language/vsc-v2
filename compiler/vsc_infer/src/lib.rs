//! Type inference (rustc's `rustc_infer`).
//!
//! An `InferCtxt` holds the variables of one body: type variables, and the
//! integer and float variables of unsuffixed literals. Unification binds
//! them. Regions are ignored here: bodies are type checked with regions
//! erased, and borrow checking works them out later on MIR, as in rustc.
//!
//! Probing (method lookup, trait selection) tries a unification inside a
//! snapshot and rolls it back.

use std::cell::RefCell;

use vsc_middle::ty::fold::{self, TypeFolder};
use vsc_middle::ty::*;
use vsc_span::def_id::DefId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum VarKind {
    Ty,
    Int,
    Float,
}

#[derive(Default)]
struct Tables<'tcx> {
    ty_vars: Vec<Option<Ty<'tcx>>>,
    int_vars: Vec<Option<Ty<'tcx>>>,
    float_vars: Vec<Option<Ty<'tcx>>>,
    /// Each binding, to undo it when a snapshot is rolled back.
    undo_log: Vec<(VarKind, u32)>,
}

pub struct InferCtxt<'tcx> {
    pub tcx: TyCtxt<'tcx>,
    tables: RefCell<Tables<'tcx>>,
}

#[derive(Clone, Copy)]
pub struct Snapshot {
    undo_len: usize,
    ty_vars: usize,
    int_vars: usize,
    float_vars: usize,
}

/// Why two types don't unify.
#[derive(Clone, Copy, Debug)]
pub struct TypeError;

impl<'tcx> InferCtxt<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        InferCtxt { tcx, tables: RefCell::new(Tables::default()) }
    }

    // -----------------------------------------------------------------------
    // Variables
    // -----------------------------------------------------------------------

    pub fn next_ty_var(&self) -> Ty<'tcx> {
        let mut t = self.tables.borrow_mut();
        t.ty_vars.push(None);
        let v = (t.ty_vars.len() - 1) as u32;
        self.tcx.mk_ty(TyKind::Infer(InferTy::TyVar(v)))
    }

    pub fn next_int_var(&self) -> Ty<'tcx> {
        let mut t = self.tables.borrow_mut();
        t.int_vars.push(None);
        let v = (t.int_vars.len() - 1) as u32;
        self.tcx.mk_ty(TyKind::Infer(InferTy::IntVar(v)))
    }

    pub fn next_float_var(&self) -> Ty<'tcx> {
        let mut t = self.tables.borrow_mut();
        t.float_vars.push(None);
        let v = (t.float_vars.len() - 1) as u32;
        self.tcx.mk_ty(TyKind::Infer(InferTy::FloatVar(v)))
    }

    /// Arguments for `def`'s generics: a new variable for each type,
    /// erased regions, and error constants (const inference isn't written
    /// yet; an error constant unifies with any).
    pub fn fresh_args_for_item(&self, def: DefId) -> GenericArgsRef<'tcx> {
        let g = self.tcx.generics_of(def);
        let args: Vec<GenericArg<'tcx>> = (0..g.count())
            .map(|i| match g.param_at(i, self.tcx).kind {
                GenericParamDefKind::Origin => GenericArg::Region(self.tcx.regions.re_erased),
                GenericParamDefKind::Type { .. } => GenericArg::Type(self.next_ty_var()),
                GenericParamDefKind::Const { .. } => GenericArg::Const(self.tcx.mk_const(ConstKind::Error)),
            })
            .collect();
        self.tcx.mk_args(&args)
    }

    /// Replaces the `_`s in a type written in a body with new variables.
    pub fn replace_fresh(&self, ty: Ty<'tcx>) -> Ty<'tcx> {
        struct R<'a, 'tcx>(&'a InferCtxt<'tcx>);
        impl<'tcx> TypeFolder<'tcx> for R<'_, 'tcx> {
            fn tcx(&self) -> TyCtxt<'tcx> {
                self.0.tcx
            }
            fn fold_ty(&mut self, t: Ty<'tcx>) -> Ty<'tcx> {
                match t.kind() {
                    TyKind::Infer(InferTy::Fresh(_)) => self.0.next_ty_var(),
                    _ => fold::super_fold_ty(self, t),
                }
            }
        }
        R(self).fold_ty(ty)
    }

    fn bind(&self, kind: VarKind, v: u32, ty: Ty<'tcx>) {
        let mut t = self.tables.borrow_mut();
        let slot = match kind {
            VarKind::Ty => &mut t.ty_vars[v as usize],
            VarKind::Int => &mut t.int_vars[v as usize],
            VarKind::Float => &mut t.float_vars[v as usize],
        };
        debug_assert!(slot.is_none());
        *slot = Some(ty);
        t.undo_log.push((kind, v));
    }

    fn binding(&self, i: InferTy) -> Option<Ty<'tcx>> {
        let t = self.tables.borrow();
        match i {
            InferTy::TyVar(v) => t.ty_vars[v as usize],
            InferTy::IntVar(v) => t.int_vars[v as usize],
            InferTy::FloatVar(v) => t.float_vars[v as usize],
            InferTy::Fresh(_) => None,
        }
    }

    /// Follows bindings at the top of `ty`.
    pub fn shallow_resolve(&self, mut ty: Ty<'tcx>) -> Ty<'tcx> {
        while let TyKind::Infer(i) = *ty.kind() {
            match self.binding(i) {
                Some(t) => ty = t,
                None => break,
            }
        }
        ty
    }

    /// `ty` with every bound variable replaced, as far as known.
    pub fn resolve_vars_if_possible(&self, ty: Ty<'tcx>) -> Ty<'tcx> {
        struct R<'a, 'tcx>(&'a InferCtxt<'tcx>);
        impl<'tcx> TypeFolder<'tcx> for R<'_, 'tcx> {
            fn tcx(&self) -> TyCtxt<'tcx> {
                self.0.tcx
            }
            fn fold_ty(&mut self, t: Ty<'tcx>) -> Ty<'tcx> {
                let t = self.0.shallow_resolve(t);
                match t.kind() {
                    TyKind::Infer(_) => t,
                    _ => fold::super_fold_ty(self, t),
                }
            }
        }
        R(self).fold_ty(ty)
    }

    pub fn resolve_args(&self, args: GenericArgsRef<'tcx>) -> GenericArgsRef<'tcx> {
        let v: Vec<_> = args
            .iter()
            .map(|a| match *a {
                GenericArg::Type(t) => GenericArg::Type(self.resolve_vars_if_possible(t)),
                other => other,
            })
            .collect();
        self.tcx.mk_args(&v)
    }

    /// Does `ty` still contain variables (after resolving)?
    pub fn has_infer(&self, ty: Ty<'tcx>) -> bool {
        let ty = self.resolve_vars_if_possible(ty);
        let mut found = false;
        walk_tys(ty, &mut |t| found |= matches!(t.kind(), TyKind::Infer(_)));
        found
    }

    /// The integer and float variables not yet bound, for defaulting.
    pub fn unbound_numeric_vars(&self) -> Vec<Ty<'tcx>> {
        let t = self.tables.borrow();
        let ints = (0..t.int_vars.len()).filter(|&v| t.int_vars[v].is_none()).map(|v| InferTy::IntVar(v as u32));
        let floats = (0..t.float_vars.len()).filter(|&v| t.float_vars[v].is_none()).map(|v| InferTy::FloatVar(v as u32));
        ints.chain(floats).map(|i| self.tcx.mk_ty(TyKind::Infer(i))).collect::<Vec<_>>()
    }

    // -----------------------------------------------------------------------
    // Snapshots
    // -----------------------------------------------------------------------

    pub fn start_snapshot(&self) -> Snapshot {
        let t = self.tables.borrow();
        Snapshot { undo_len: t.undo_log.len(), ty_vars: t.ty_vars.len(), int_vars: t.int_vars.len(), float_vars: t.float_vars.len() }
    }

    pub fn rollback_to(&self, s: Snapshot) {
        let mut t = self.tables.borrow_mut();
        while t.undo_log.len() > s.undo_len {
            let (kind, v) = t.undo_log.pop().unwrap();
            let vec = match kind {
                VarKind::Ty => &mut t.ty_vars,
                VarKind::Int => &mut t.int_vars,
                VarKind::Float => &mut t.float_vars,
            };
            if let Some(slot) = vec.get_mut(v as usize) {
                *slot = None;
            }
        }
        t.ty_vars.truncate(s.ty_vars);
        t.int_vars.truncate(s.int_vars);
        t.float_vars.truncate(s.float_vars);
    }

    /// Runs `f`, then undoes everything it did.
    pub fn probe<R>(&self, f: impl FnOnce() -> R) -> R {
        let s = self.start_snapshot();
        let r = f();
        self.rollback_to(s);
        r
    }

    /// Runs `f`; undoes what it did if it fails.
    pub fn commit_if_ok<T, E>(&self, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
        let s = self.start_snapshot();
        let r = f();
        if r.is_err() {
            self.rollback_to(s);
        }
        r
    }

    // -----------------------------------------------------------------------
    // Unification
    // -----------------------------------------------------------------------

    /// Makes `a` and `b` the same type, binding variables. On failure,
    /// variables bound along the way stay bound; wrap in `commit_if_ok` to
    /// undo them.
    pub fn unify(&self, a: Ty<'tcx>, b: Ty<'tcx>) -> Result<(), TypeError> {
        let a = self.shallow_resolve(a);
        let b = self.shallow_resolve(b);
        if a == b {
            return Ok(());
        }
        use InferTy::*;
        match (*a.kind(), *b.kind()) {
            (TyKind::Error, _) | (_, TyKind::Error) => Ok(()),
            (TyKind::Infer(TyVar(x)), _) => self.bind_var(VarKind::Ty, x, b),
            (_, TyKind::Infer(TyVar(y))) => self.bind_var(VarKind::Ty, y, a),
            (TyKind::Infer(IntVar(x)), TyKind::Infer(IntVar(_))) => self.bind_var(VarKind::Int, x, b),
            (TyKind::Infer(IntVar(x)), TyKind::Int(_) | TyKind::Uint(_)) => self.bind_var(VarKind::Int, x, b),
            (TyKind::Int(_) | TyKind::Uint(_), TyKind::Infer(IntVar(y))) => self.bind_var(VarKind::Int, y, a),
            (TyKind::Infer(FloatVar(x)), TyKind::Infer(FloatVar(_))) => self.bind_var(VarKind::Float, x, b),
            (TyKind::Infer(FloatVar(x)), TyKind::Float(_)) => self.bind_var(VarKind::Float, x, b),
            (TyKind::Float(_), TyKind::Infer(FloatVar(y))) => self.bind_var(VarKind::Float, y, a),
            (TyKind::Adt(d1, a1), TyKind::Adt(d2, a2)) if d1 == d2 => self.unify_args(a1, a2),
            (TyKind::Array(t1, n1), TyKind::Array(t2, n2)) => {
                self.unify(t1, t2)?;
                self.unify_consts(n1, n2)
            }
            (TyKind::Slice(t1), TyKind::Slice(t2)) => self.unify(t1, t2),
            (TyKind::RawPtr(t1, m1), TyKind::RawPtr(t2, m2)) if m1 == m2 => self.unify(t1, t2),
            (TyKind::Ref(_, t1, m1), TyKind::Ref(_, t2, m2)) if m1 == m2 => self.unify(t1, t2),
            (TyKind::FnDef(d1, a1), TyKind::FnDef(d2, a2)) if d1 == d2 => self.unify_args(a1, a2),
            (TyKind::Closure(d1, a1), TyKind::Closure(d2, a2)) if d1 == d2 => self.unify_args(a1, a2),
            (TyKind::FnPtr(s1), TyKind::FnPtr(s2)) => self.unify_sigs(s1.value, s2.value),
            (TyKind::Tuple(x), TyKind::Tuple(y)) if x.len() == y.len() => {
                x.iter().zip(y.iter()).try_for_each(|(a, b)| self.unify(*a, *b))
            }
            (TyKind::Alias(x), TyKind::Alias(y)) if x.def_id == y.def_id => self.unify_args(x.args, y.args),
            (TyKind::Dynamic(p1, _), TyKind::Dynamic(p2, _)) if p1.len() == p2.len() => {
                for (x, y) in p1.iter().zip(p2.iter()) {
                    match (*x, *y) {
                        (ExistentialPredicate::Trait { def_id: d1, args: a1 }, ExistentialPredicate::Trait { def_id: d2, args: a2 })
                            if d1 == d2 =>
                        {
                            self.unify_args(a1, a2)?
                        }
                        (
                            ExistentialPredicate::Projection { def_id: d1, args: a1, term: t1 },
                            ExistentialPredicate::Projection { def_id: d2, args: a2, term: t2 },
                        ) if d1 == d2 => {
                            self.unify_args(a1, a2)?;
                            self.unify(t1, t2)?
                        }
                        _ => return Err(TypeError),
                    }
                }
                Ok(())
            }
            _ => Err(TypeError),
        }
    }

    pub fn unify_sigs(&self, a: FnSig<'tcx>, b: FnSig<'tcx>) -> Result<(), TypeError> {
        if a.inputs_and_output.len() != b.inputs_and_output.len() || a.c_variadic != b.c_variadic || a.safety != b.safety || a.abi != b.abi {
            return Err(TypeError);
        }
        a.inputs_and_output.iter().zip(b.inputs_and_output.iter()).try_for_each(|(x, y)| self.unify(*x, *y))
    }

    pub fn unify_args(&self, a: GenericArgsRef<'tcx>, b: GenericArgsRef<'tcx>) -> Result<(), TypeError> {
        if a.len() != b.len() {
            return Err(TypeError);
        }
        for (x, y) in a.iter().zip(b.iter()) {
            match (*x, *y) {
                (GenericArg::Type(x), GenericArg::Type(y)) => self.unify(x, y)?,
                (GenericArg::Const(x), GenericArg::Const(y)) => self.unify_consts(x, y)?,
                // Regions are left to borrow checking.
                (GenericArg::Region(_), GenericArg::Region(_)) => {}
                _ => return Err(TypeError),
            }
        }
        Ok(())
    }

    fn unify_consts(&self, a: Const<'tcx>, b: Const<'tcx>) -> Result<(), TypeError> {
        match (*a.kind(), *b.kind()) {
            _ if a == b => Ok(()),
            (ConstKind::Error, _) | (_, ConstKind::Error) => Ok(()),
            // Unevaluated constants can't be compared before evaluation.
            (ConstKind::Unevaluated(..), _) | (_, ConstKind::Unevaluated(..)) => Ok(()),
            (ConstKind::Value(_, x), ConstKind::Value(_, y)) if x == y => Ok(()),
            _ => Err(TypeError),
        }
    }

    fn bind_var(&self, kind: VarKind, v: u32, ty: Ty<'tcx>) -> Result<(), TypeError> {
        // The occurs check: `?0 = Vec<?0>` has no solution.
        if kind == VarKind::Ty {
            let resolved = self.resolve_vars_if_possible(ty);
            let mut occurs = false;
            walk_tys(resolved, &mut |t| occurs |= matches!(t.kind(), TyKind::Infer(InferTy::TyVar(x)) if *x == v));
            if occurs {
                return Err(TypeError);
            }
        }
        self.bind(kind, v, ty);
        Ok(())
    }
}

/// Calls `f` on `ty` and every type inside it.
pub fn walk_tys<'tcx>(ty: Ty<'tcx>, f: &mut dyn FnMut(Ty<'tcx>)) {
    f(ty);
    let args = |a: &[GenericArg<'tcx>], f: &mut dyn FnMut(Ty<'tcx>)| {
        for x in a {
            if let GenericArg::Type(t) = x {
                walk_tys(*t, f);
            }
        }
    };
    match *ty.kind() {
        TyKind::Adt(_, a) | TyKind::FnDef(_, a) | TyKind::Closure(_, a) => args(a, f),
        TyKind::Alias(a) => args(a.args, f),
        TyKind::Array(t, _) | TyKind::Slice(t) | TyKind::RawPtr(t, _) | TyKind::Ref(_, t, _) => walk_tys(t, f),
        TyKind::Tuple(tys) => tys.iter().for_each(|t| walk_tys(*t, f)),
        TyKind::FnPtr(sig) => sig.value.inputs_and_output.iter().for_each(|t| walk_tys(*t, f)),
        TyKind::Dynamic(preds, _) => {
            for p in preds {
                match *p {
                    ExistentialPredicate::Trait { args: a, .. } => args(a, f),
                    ExistentialPredicate::Projection { args: a, term, .. } => {
                        args(a, f);
                        walk_tys(term, f);
                    }
                }
            }
        }
        _ => {}
    }
}
