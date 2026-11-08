//! Proving trait bounds (rustc's `rustc_trait_selection`), in its simplest
//! form: a goal `T: Trait` is met by a where clause of the item being
//! checked, or by an impl whose header unifies with it and whose own where
//! clauses can be met. `<T as Trait>.Assoc` normalizes to the selected
//! impl's type for `Assoc`, or the where clause's `Assoc == U`, or stays as
//! it is when `T` is a parameter (a rigid projection).
//!
//! Goals whose types aren't known yet wait in a `FulfillCtxt` and are
//! retried as inference makes progress.

use std::cell::RefCell;

use vsc_hir::def::DefKind;
use vsc_infer::InferCtxt;
use vsc_middle::ty::fold::{self, TypeFolder};
use vsc_middle::ty::*;
use vsc_span::Span;
use vsc_span::def_id::DefId;

/// The where clauses in scope in a body.
#[derive(Clone, Debug, Default)]
pub struct ParamEnv<'tcx> {
    pub clauses: Vec<Clause<'tcx>>,
}

impl<'tcx> ParamEnv<'tcx> {
    /// The predicates of `def` and its parents, regions erased.
    pub fn for_item(tcx: TyCtxt<'tcx>, def: DefId) -> Self {
        let mut clauses = Vec::new();
        let mut d = Some(def);
        while let Some(x) = d {
            if matches!(tcx.def_kind(x), DefKind::Closure | DefKind::InlineConst | DefKind::AnonConst) {
                d = tcx.opt_parent(x);
                continue;
            }
            let p = tcx.predicates_of(x);
            clauses.extend(p.predicates.iter().map(|(c, _)| tcx.erase_regions_clause(*c)));
            d = p.parent;
        }
        elaborate(tcx, &mut clauses);
        ParamEnv { clauses }
    }
}

/// Adds what each `T: Trait` implies through the trait's supertraits:
/// `F: Fn(A)` gives `F: FnMut(A)` and `F: FnOnce(A)` (rustc's
/// `elaborate`).
pub fn elaborate<'tcx>(tcx: TyCtxt<'tcx>, clauses: &mut Vec<Clause<'tcx>>) {
    let mut i = 0;
    while i < clauses.len() && clauses.len() < 512 {
        if let Clause::Trait(t) = clauses[i] {
            if tcx.def_kind(t.def_id) == DefKind::Trait {
                // The first predicate of a trait is `Self: Trait` itself.
                for (c, _) in tcx.predicates_of(t.def_id).predicates.iter().skip(1) {
                    let c = tcx.erase_regions_clause(tcx.instantiate_clause(*c, t.args));
                    if !clauses.contains(&c) {
                        clauses.push(c);
                    }
                }
            }
        }
        i += 1;
    }
}

/// Which function trait `def` is, if it's one.
pub fn fn_trait_kind(tcx: TyCtxt<'_>, def: DefId) -> Option<ClosureKind> {
    let li = tcx.lang_items();
    if li.get(vsc_hir::LangItem::Fn) == Some(def) {
        Some(ClosureKind::Fn)
    } else if li.get(vsc_hir::LangItem::FnMut) == Some(def) {
        Some(ClosureKind::FnMut)
    } else if li.get(vsc_hir::LangItem::FnOnce) == Some(def) {
        Some(ClosureKind::FnOnce)
    } else {
        None
    }
}

/// The operator traits, grouped by what the primitive types implement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpTrait {
    /// `Add`…`Rem` and their `*Assign` forms: numbers, with the same type
    /// on both sides.
    Arith,
    /// `BitAnd`, `BitOr`, `BitXor` and their `*Assign` forms: integers and
    /// `bool`.
    Bit,
    /// `Shl`, `Shr` and their `*Assign` forms: integers, any integer on the
    /// right.
    Shift,
    Neg,
    Not,
    /// `PartialEq` and `PartialOrd`: the scalars.
    Cmp,
}

/// Which operator trait `def` is, if it's one.
pub fn op_trait(tcx: TyCtxt<'_>, def: DefId) -> Option<OpTrait> {
    use vsc_hir::LangItem as L;
    let item = tcx.lang_items().from_def_id(def)?;
    Some(match item {
        L::Add | L::Sub | L::Mul | L::Div | L::Rem | L::AddAssign | L::SubAssign | L::MulAssign | L::DivAssign | L::RemAssign => OpTrait::Arith,
        L::BitAnd | L::BitOr | L::BitXor | L::BitAndAssign | L::BitOrAssign | L::BitXorAssign => OpTrait::Bit,
        L::Shl | L::Shr | L::ShlAssign | L::ShrAssign => OpTrait::Shift,
        L::Neg => OpTrait::Neg,
        L::Not => OpTrait::Not,
        L::PartialEq | L::PartialOrd => OpTrait::Cmp,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug)]
pub enum ObligationKind<'tcx> {
    Trait(TraitRef<'tcx>),
    /// The projection normalizes to the type.
    Projection(AliasTy<'tcx>, Ty<'tcx>),
}

#[derive(Clone, Copy, Debug)]
pub struct Obligation<'tcx> {
    pub kind: ObligationKind<'tcx>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug)]
pub enum Candidate<'tcx> {
    Impl(DefId, GenericArgsRef<'tcx>),
    /// A where clause in scope.
    Param(TraitRef<'tcx>),
    /// Implemented by the compiler: the function traits for function
    /// items, pointers, `func` values and literals.
    Builtin,
}

#[derive(Debug)]
pub enum Selection<'tcx> {
    /// One candidate applies; its unification is done, and its where
    /// clauses are new obligations.
    Selected(Candidate<'tcx>, Vec<Obligation<'tcx>>),
    /// The types aren't known well enough to choose.
    Ambiguous,
    NoSolution,
}

/// A goal that failed.
#[derive(Clone, Copy, Debug)]
pub enum FulfillmentError<'tcx> {
    Unsatisfied(Obligation<'tcx>),
    /// The projection normalized to a type that doesn't match.
    ProjectionMismatch { obligation: Obligation<'tcx>, normalized: Ty<'tcx> },
    Ambiguous(Obligation<'tcx>),
}

const DEPTH_LIMIT: u32 = 16;

pub struct SelectionCtxt<'a, 'tcx> {
    pub infcx: &'a InferCtxt<'tcx>,
    pub param_env: &'a ParamEnv<'tcx>,
}

impl<'a, 'tcx> SelectionCtxt<'a, 'tcx> {
    pub fn new(infcx: &'a InferCtxt<'tcx>, param_env: &'a ParamEnv<'tcx>) -> Self {
        SelectionCtxt { infcx, param_env }
    }

    fn tcx(&self) -> TyCtxt<'tcx> {
        self.infcx.tcx
    }

    /// The impls of a trait in this crate.
    pub fn trait_impls(&self, trait_def: DefId) -> Vec<DefId> {
        let tcx = self.tcx();
        tcx.defs
            .iter_enumerated()
            .filter(|(_, d)| d.kind == DefKind::Impl { of_trait: true })
            .map(|(id, _)| id.to_def_id())
            .filter(|&i| tcx.impl_trait_ref(i).is_some_and(|t| t.def_id == trait_def))
            .collect()
    }

    /// An impl's trait reference with new variables for its parameters.
    fn fresh_impl(&self, imp: DefId) -> (GenericArgsRef<'tcx>, TraitRef<'tcx>) {
        let tcx = self.tcx();
        let args = self.infcx.fresh_args_for_item(imp);
        let tr = tcx.impl_trait_ref(imp).unwrap();
        let tr = match tcx.instantiate_clause(Clause::Trait(tr), args) {
            Clause::Trait(t) => t,
            _ => unreachable!(),
        };
        let tr = match tcx.erase_regions_clause(Clause::Trait(tr)) {
            Clause::Trait(t) => t,
            _ => unreachable!(),
        };
        (args, tr)
    }

    /// The where clauses of an impl, for its arguments.
    fn impl_obligations(&self, imp: DefId, args: GenericArgsRef<'tcx>, span: Span) -> Vec<Obligation<'tcx>> {
        let tcx = self.tcx();
        tcx.predicates_of(imp)
            .predicates
            .iter()
            .filter_map(|(c, _)| match tcx.erase_regions_clause(tcx.instantiate_clause(*c, args)) {
                Clause::Trait(t) => Some(Obligation { kind: ObligationKind::Trait(t), span }),
                Clause::Projection(a, t) => Some(Obligation { kind: ObligationKind::Projection(a, t), span }),
                _ => None,
            })
            .collect()
    }

    pub fn select(&self, tr: TraitRef<'tcx>, span: Span) -> Selection<'tcx> {
        self.select_at_depth(tr, span, 0)
    }

    fn select_at_depth(&self, tr: TraitRef<'tcx>, span: Span, depth: u32) -> Selection<'tcx> {
        let infcx = self.infcx;
        let self_ty = infcx.shallow_resolve(tr.self_ty());
        if matches!(self_ty.kind(), TyKind::Infer(InferTy::TyVar(_))) {
            return Selection::Ambiguous;
        }
        if self_ty == self.tcx().types.error || depth > DEPTH_LIMIT {
            return Selection::Ambiguous;
        }
        // `<Evens as Container>.Item: Describe`, from a where clause
        // instantiated for a caller: normalize the projection first.
        if let TyKind::Alias(alias) = *self_ty.kind() {
            match self.normalize_projection(alias, span) {
                Some((ty, mut nested)) if ty != self_ty => {
                    let mut args = tr.args.to_vec();
                    args[0] = GenericArg::Type(ty);
                    let tr = TraitRef { def_id: tr.def_id, args: self.tcx().mk_args(&args) };
                    return match self.select_at_depth(tr, span, depth + 1) {
                        Selection::Selected(c, more) => {
                            nested.extend(more);
                            Selection::Selected(c, nested)
                        }
                        other => other,
                    };
                }
                None => return Selection::Ambiguous,
                _ => {}
            }
        }

        // Where clauses first, as in rustc.
        let params: Vec<TraitRef<'tcx>> = self
            .param_env
            .clauses
            .iter()
            .filter_map(|c| match c {
                Clause::Trait(c) if c.def_id == tr.def_id => Some(*c),
                _ => None,
            })
            .filter(|c| infcx.probe(|| infcx.unify_args(c.args, tr.args).is_ok()))
            .collect();
        if let Some(&first) = params.first() {
            let all_same = params.iter().all(|c| infcx.resolve_args(c.args) == infcx.resolve_args(first.args));
            if params.len() > 1 && !all_same {
                return Selection::Ambiguous;
            }
            infcx.unify_args(first.args, tr.args).ok();
            return Selection::Selected(Candidate::Param(first), Vec::new());
        }

        if let Some(kind) = fn_trait_kind(self.tcx(), tr.def_id) {
            if let Some((sig, self_kind)) = self.callable_sig(self_ty) {
                if !self_kind.extends(kind) || tr.args.len() < 2 {
                    return Selection::NoSolution;
                }
                let tuple = self.tcx().mk_tup(sig.inputs());
                if infcx.commit_if_ok(|| infcx.unify(tr.args[1].expect_ty(), tuple)).is_err() {
                    return Selection::NoSolution;
                }
                return Selection::Selected(Candidate::Builtin, Vec::new());
            }
        }

        if let Some(op) = op_trait(self.tcx(), tr.def_id) {
            if let Some(sel) = self.builtin_op(op, tr, self_ty) {
                return sel;
            }
        }
        if self.tcx().is_lang_item(tr.def_id, vsc_hir::LangItem::Copy) {
            if let Some(sel) = self.builtin_copy(tr, self_ty, span) {
                return sel;
            }
        }

        let mut applicable = Vec::new();
        for imp in self.trait_impls(tr.def_id) {
            let may_apply = infcx.probe(|| {
                let (args, impl_tr) = self.fresh_impl(imp);
                if infcx.unify_args(impl_tr.args, tr.args).is_err() {
                    return false;
                }
                self.impl_obligations(imp, args, span).iter().all(|o| match o.kind {
                    ObligationKind::Trait(t) => !matches!(self.select_at_depth(t, span, depth + 1), Selection::NoSolution),
                    ObligationKind::Projection(..) => true,
                })
            });
            if may_apply {
                applicable.push(imp);
            }
        }
        match applicable[..] {
            [imp] => {
                let (args, impl_tr) = self.fresh_impl(imp);
                infcx.unify_args(impl_tr.args, tr.args).ok();
                let nested = self.impl_obligations(imp, args, span);
                Selection::Selected(Candidate::Impl(imp, args), nested)
            }
            [] => Selection::NoSolution,
            _ => Selection::Ambiguous,
        }
    }

    /// The operator traits the compiler provides for the primitive types
    /// (rustc's library implements them in `core`).
    fn builtin_op(&self, op: OpTrait, tr: TraitRef<'tcx>, self_ty: Ty<'tcx>) -> Option<Selection<'tcx>> {
        let infcx = self.infcx;
        let is_int = |t: Ty<'tcx>| matches!(t.kind(), TyKind::Int(_) | TyKind::Uint(_) | TyKind::Infer(InferTy::IntVar(_)));
        let is_float = |t: Ty<'tcx>| matches!(t.kind(), TyKind::Float(_) | TyKind::Infer(InferTy::FloatVar(_)));
        let applies = match op {
            OpTrait::Arith => is_int(self_ty) || is_float(self_ty),
            OpTrait::Bit => is_int(self_ty) || matches!(self_ty.kind(), TyKind::Bool),
            OpTrait::Shift => is_int(self_ty),
            OpTrait::Neg => matches!(self_ty.kind(), TyKind::Int(_) | TyKind::Infer(InferTy::IntVar(_))) || is_float(self_ty),
            OpTrait::Not => is_int(self_ty) || matches!(self_ty.kind(), TyKind::Bool),
            OpTrait::Cmp => is_int(self_ty) || is_float(self_ty) || matches!(self_ty.kind(), TyKind::Bool | TyKind::Char),
        };
        if !applies {
            return None;
        }
        // The right-hand side, if the trait has one.
        if let Some(rhs) = tr.args.get(1).map(|a| a.expect_ty()) {
            if op == OpTrait::Shift {
                let rhs = infcx.shallow_resolve(rhs);
                if matches!(rhs.kind(), TyKind::Infer(InferTy::TyVar(_))) {
                    return Some(Selection::Ambiguous);
                }
                if !is_int(rhs) {
                    return Some(Selection::NoSolution);
                }
            } else if infcx.commit_if_ok(|| infcx.unify(rhs, self_ty)).is_err() {
                return Some(Selection::NoSolution);
            }
        }
        Some(Selection::Selected(Candidate::Builtin, Vec::new()))
    }

    /// `Copy` for the types Rust makes it built in for: scalars, shared
    /// references, raw pointers, function items and pointers, `never`, and
    /// tuples and arrays of `Copy` values (as nested goals). `None` leaves
    /// it to the impls (structs and enums opt in).
    fn builtin_copy(&self, tr: TraitRef<'tcx>, self_ty: Ty<'tcx>, span: Span) -> Option<Selection<'tcx>> {
        let tcx = self.tcx();
        let goal = |t: Ty<'tcx>| Obligation { kind: ObligationKind::Trait(TraitRef { def_id: tr.def_id, args: tcx.mk_args(&[GenericArg::Type(t)]) }), span };
        let nested = match *self_ty.kind() {
            TyKind::Bool
            | TyKind::Char
            | TyKind::Int(_)
            | TyKind::Uint(_)
            | TyKind::Float(_)
            | TyKind::Never
            | TyKind::RawPtr(..)
            | TyKind::FnDef(..)
            | TyKind::FnPtr(_)
            | TyKind::Infer(InferTy::IntVar(_) | InferTy::FloatVar(_)) => Vec::new(),
            TyKind::Ref(_, _, Mutability::Not) => Vec::new(),
            TyKind::Ref(_, _, Mutability::Mut) => return Some(Selection::NoSolution),
            TyKind::Tuple(tys) => tys.iter().map(|&t| goal(t)).collect(),
            TyKind::Array(elem, _) => vec![goal(elem)],
            _ => return None,
        };
        Some(Selection::Selected(Candidate::Builtin, nested))
    }

    /// The signature of a value the function traits are built in for, and
    /// the most specific of them it implements.
    pub fn callable_sig(&self, ty: Ty<'tcx>) -> Option<(FnSig<'tcx>, ClosureKind)> {
        let tcx = self.tcx();
        match *self.infcx.shallow_resolve(ty).kind() {
            TyKind::Closure(def, args) => {
                let TyKind::FnPtr(sig) = *args[0].expect_ty().kind() else { return None };
                let known = self.infcx.closure_kinds.borrow().get(&def).copied();
                let kind = known.or_else(|| tcx.typeck(def).closure_kinds.get(&def.expect_local()).copied()).unwrap_or(ClosureKind::Fn);
                Some((sig.value, kind))
            }
            TyKind::FnDef(def, args) => {
                let sig = tcx.fn_sig(def).skip_binder();
                if sig.safety == Safety::Unsafe {
                    return None;
                }
                let io: Vec<Ty<'tcx>> = sig.inputs_and_output.iter().map(|t| tcx.erase_regions(tcx.instantiate(*t, args))).collect();
                Some((FnSig { inputs_and_output: tcx.mk_type_list(&io), ..sig }, ClosureKind::Fn))
            }
            TyKind::FnPtr(sig) if sig.value.safety == Safety::Safe => Some((sig.value, ClosureKind::Fn)),
            TyKind::Adt(d, a) if tcx.is_lang_item(d.did(), vsc_hir::LangItem::Func) && a.len() == 1 => {
                match *self.infcx.shallow_resolve(a[0].expect_ty()).kind() {
                    TyKind::FnPtr(sig) => Some((sig.value, ClosureKind::Fn)),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Normalizes a projection. `None` if it can't be decided yet.
    pub fn normalize_projection(&self, alias: AliasTy<'tcx>, span: Span) -> Option<(Ty<'tcx>, Vec<Obligation<'tcx>>)> {
        let tcx = self.tcx();
        let infcx = self.infcx;
        let trait_def = tcx.parent(alias.def_id);
        let n = tcx.generics_of(trait_def).count();
        let tr = TraitRef { def_id: trait_def, args: tcx.mk_args(&alias.args[..n.min(alias.args.len())]) };
        match self.select(tr, span) {
            Selection::Selected(Candidate::Impl(imp, args), nested) => {
                let name = tcx.item_name(alias.def_id);
                let item = tcx.associated_items(imp).iter().find(|i| i.kind == AssocKind::Type && i.name == name).copied();
                let ty = match item {
                    Some(item) => tcx.erase_regions(tcx.instantiate(tcx.type_of(item.def_id), args)),
                    None => tcx.types.error,
                };
                // The impl's type may itself be a projection: `&F`'s
                // `Output` is `F`'s.
                if let TyKind::Alias(inner) = *self.infcx.resolve_vars_if_possible(ty).kind() {
                    if inner != alias {
                        if let Some((ty, more)) = self.normalize_projection(inner, span) {
                            let mut nested = nested;
                            nested.extend(more);
                            return Some((ty, nested));
                        }
                    }
                }
                Some((ty, nested))
            }
            Selection::Selected(Candidate::Param(_), _) => {
                // A where clause `<T as Trait>.Assoc == U` decides it;
                // otherwise the projection is rigid.
                for c in &self.param_env.clauses {
                    if let Clause::Projection(a, term) = c {
                        if a.def_id == alias.def_id && infcx.probe(|| infcx.unify_args(a.args, alias.args).is_ok()) {
                            infcx.unify_args(a.args, alias.args).ok();
                            return Some((*term, Vec::new()));
                        }
                    }
                }
                let resolved = AliasTy { def_id: alias.def_id, args: infcx.resolve_args(alias.args) };
                Some((tcx.mk_ty(TyKind::Alias(resolved)), Vec::new()))
            }
            Selection::Selected(Candidate::Builtin, nested) => {
                // `<int32 as Add<int32>>.Output` is `int32`; `<F as
                // FnOnce<A>>.Output` is the signature's output.
                let output = if op_trait(tcx, trait_def).is_some() {
                    infcx.shallow_resolve(tr.self_ty())
                } else {
                    self.callable_sig(tr.self_ty()).map(|(sig, _)| sig.output()).unwrap_or(tcx.types.error)
                };
                Some((output, nested))
            }
            Selection::Ambiguous => None,
            Selection::NoSolution => Some((tcx.types.error, Vec::new())),
        }
    }
}

/// Goals waiting to be proven.
#[derive(Default)]
pub struct FulfillCtxt<'tcx> {
    pending: RefCell<Vec<Obligation<'tcx>>>,
}

impl<'tcx> FulfillCtxt<'tcx> {
    pub fn register(&self, o: Obligation<'tcx>) {
        self.pending.borrow_mut().push(o);
    }

    pub fn is_empty(&self) -> bool {
        self.pending.borrow().is_empty()
    }

    /// Proves what can be proven now. Returns the goals that failed.
    pub fn select_where_possible(&self, selcx: &SelectionCtxt<'_, 'tcx>) -> Vec<FulfillmentError<'tcx>> {
        let mut errors = Vec::new();
        loop {
            let pending = std::mem::take(&mut *self.pending.borrow_mut());
            let before = pending.len();
            let mut kept = Vec::new();
            let mut progress = false;
            for o in pending {
                match o.kind {
                    ObligationKind::Trait(tr) => match selcx.select(tr, o.span) {
                        Selection::Selected(_, nested) => {
                            progress = true;
                            kept.extend(nested);
                        }
                        Selection::Ambiguous => kept.push(o),
                        Selection::NoSolution => {
                            progress = true;
                            errors.push(FulfillmentError::Unsatisfied(o));
                        }
                    },
                    ObligationKind::Projection(alias, ty) => match selcx.normalize_projection(alias, o.span) {
                        Some((normalized, nested)) => {
                            progress = true;
                            kept.extend(nested);
                            if selcx.infcx.commit_if_ok(|| selcx.infcx.unify(normalized, ty)).is_err() {
                                errors.push(FulfillmentError::ProjectionMismatch { obligation: o, normalized });
                            }
                        }
                        None => kept.push(o),
                    },
                }
            }
            let done = !progress || kept.is_empty();
            let n = kept.len();
            self.pending.borrow_mut().extend(kept);
            if done || (n == before && !progress) {
                break;
            }
        }
        errors
    }

    /// Proves everything, or reports what's left as ambiguous.
    pub fn select_all_or_error(&self, selcx: &SelectionCtxt<'_, 'tcx>) -> Vec<FulfillmentError<'tcx>> {
        let mut errors = self.select_where_possible(selcx);
        errors.extend(self.pending.borrow_mut().drain(..).map(FulfillmentError::Ambiguous));
        errors
    }
}

/// Replaces each projection in `ty` with a new variable, registering the
/// obligation that it normalizes to it (rustc's `normalize` with
/// obligations). Projections that can be decided now are replaced
/// directly.
pub fn normalize<'tcx>(selcx: &SelectionCtxt<'_, 'tcx>, fulfill: &FulfillCtxt<'tcx>, ty: Ty<'tcx>, span: Span) -> Ty<'tcx> {
    struct N<'a, 'b, 'tcx> {
        selcx: &'a SelectionCtxt<'b, 'tcx>,
        fulfill: &'a FulfillCtxt<'tcx>,
        span: Span,
    }
    impl<'tcx> TypeFolder<'tcx> for N<'_, '_, 'tcx> {
        fn tcx(&self) -> TyCtxt<'tcx> {
            self.selcx.infcx.tcx
        }
        fn fold_ty(&mut self, t: Ty<'tcx>) -> Ty<'tcx> {
            let t = fold::super_fold_ty(self, t);
            match *t.kind() {
                TyKind::Alias(alias) => match self.selcx.normalize_projection(alias, self.span) {
                    Some((ty, nested)) => {
                        for o in nested {
                            self.fulfill.register(o);
                        }
                        ty
                    }
                    _ => {
                        let v = self.selcx.infcx.next_ty_var();
                        self.fulfill.register(Obligation { kind: ObligationKind::Projection(alias, v), span: self.span });
                        v
                    }
                },
                _ => t,
            }
        }
    }
    N { selcx, fulfill, span }.fold_ty(ty)
}
