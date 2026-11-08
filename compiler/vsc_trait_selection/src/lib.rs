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
        ParamEnv { clauses }
    }
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
