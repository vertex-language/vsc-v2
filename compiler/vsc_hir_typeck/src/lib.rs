//! Type checking function bodies (rustc's `rustc_hir_typeck`).
//!
//! Each body (a function's, constant's or static's; function literals go
//! with their enclosing body) is checked by a `FnCtxt`: expressions are
//! checked against an expected type where one is known, unknown types
//! become inference variables, trait bounds are proven by
//! `vsc_trait_selection`, and at the end every type is resolved and written
//! to the body's `TypeckResults`.
//!
//! Where Vertex differs:
//!
//! - Exception 10: a block's last expression statement is its value, but
//!   when the block is expected to be `()`, the value is discarded. So
//!   `if ok { list.Pop() }` and a function returning `()` whose last line
//!   returns something are fine, as they would be in Rust with a `;`.
//! - Exception 3: a class value is `Class<C>`; its fields and methods are
//!   reached through it as if it were `C`.
//! - Exception 14: a `func` value is called like a function, and a function
//!   literal or function coerces to one.
//!
//! Not written yet: `async`, operator traits (operators work on primitive
//! types only), closures as `Fn` trait objects, and `unsafe` checking
//! (rustc does that on THIR).

mod coercion;
pub mod dump;
mod expr;
mod method;
mod pat;

#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};

use vsc_data_structures::FxHashMap;
use vsc_errors::{Diag, ErrCode};
use vsc_hir::def::DefKind;
use vsc_hir::{self as hir, HirId};
use vsc_hir_analysis::hir_ty_lowering::{ElidedRegions, ItemCtxt};
use vsc_infer::InferCtxt;
use vsc_middle::query::Providers;
use vsc_middle::ty::*;
use vsc_span::Span;
use vsc_span::def_id::{DefId, LocalDefId};
use vsc_trait_selection::{FulfillCtxt, FulfillmentError, Obligation, ObligationKind, ParamEnv, SelectionCtxt};

pub fn provide(providers: &mut Providers) {
    providers.typeck = typeck;
}

/// Type checks every body in the crate.
pub fn check_crate(tcx: TyCtxt<'_>) {
    for (local, info) in tcx.defs.iter_enumerated() {
        if body_of(tcx, local).is_some() && !matches!(info.kind, DefKind::Closure | DefKind::InlineConst) {
            tcx.typeck(local.to_def_id());
        }
    }
}

/// The body a definition owns, if it has one of its own.
pub fn body_of(tcx: TyCtxt<'_>, def: LocalDefId) -> Option<hir::BodyId> {
    tcx.body_owned_by(def)
}

fn typeck<'tcx>(tcx: TyCtxt<'tcx>, def: DefId) -> TypeckResults<'tcx> {
    let local = def.expect_local();
    let Some(body_id) = body_of(tcx, local) else { return TypeckResults::new(local) };
    let body = tcx.hir_crate.body(body_id);
    let infcx = InferCtxt::new(tcx);
    let param_env = ParamEnv::for_item(tcx, def);
    let fcx = FnCtxt::new(tcx, &infcx, &param_env, def);

    match tcx.def_kind(def) {
        DefKind::Fn | DefKind::AssocFn => {
            let sig = fcx.liberated_fn_sig(def);
            fcx.check_fn_body(body, sig.inputs(), sig.output());
        }
        DefKind::AnonConst => {
            // Array lengths are `uint`s; enum discriminants `int`s.
            let parent = tcx.parent(def);
            let ty = if tcx.def_kind(parent) == DefKind::Variant { tcx.types.isize } else { tcx.types.usize };
            fcx.check_fn_body(body, &[], ty);
        }
        _ => {
            let ty = fcx.normalize(tcx.erase_regions(tcx.type_of(def)), body.value.span);
            fcx.check_fn_body(body, &[], ty);
        }
    }
    fcx.finish()
}

/// What a breakable expression (`loop`, a labeled block) collects from its
/// `break`s.
struct Breakable<'tcx> {
    id: HirId,
    /// The type `break` values coerce to; `None` for `while` and `for`.
    target: Option<Ty<'tcx>>,
    may_break: bool,
}

pub struct FnCtxt<'a, 'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub infcx: &'a InferCtxt<'tcx>,
    param_env: &'a ParamEnv<'tcx>,
    fulfill: FulfillCtxt<'tcx>,
    pub body_owner: DefId,
    pub(crate) results: RefCell<TypeckResults<'tcx>>,
    /// Where each node is, to report unresolved types.
    spans: RefCell<FxHashMap<HirId, Span>>,
    locals: RefCell<FxHashMap<HirId, Ty<'tcx>>>,
    /// What `return` coerces to: the function's, then each enclosing
    /// function literal's.
    ret_tys: RefCell<Vec<Ty<'tcx>>>,
    breakables: RefCell<Vec<Breakable<'tcx>>>,
    /// Whether the code checked so far always diverges (rustc's `Diverges`).
    diverges: Cell<bool>,
    /// Casts, checked once types are known.
    deferred_casts: RefCell<Vec<(Span, Ty<'tcx>, Ty<'tcx>)>>,
    tainted: Cell<bool>,
    /// The session's error count when checking started: any error since
    /// (a missing lang item, say) taints the body too.
    errors_at_start: usize,
}

/// What an expression is expected to be.
#[derive(Clone, Copy, Debug)]
pub enum Expectation<'tcx> {
    None,
    HasType(Ty<'tcx>),
}

impl<'tcx> Expectation<'tcx> {
    pub fn only_has_type(self) -> Option<Ty<'tcx>> {
        match self {
            Expectation::HasType(t) => Some(t),
            Expectation::None => None,
        }
    }
}

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    fn new(tcx: TyCtxt<'tcx>, infcx: &'a InferCtxt<'tcx>, param_env: &'a ParamEnv<'tcx>, body_owner: DefId) -> Self {
        FnCtxt {
            tcx,
            infcx,
            param_env,
            fulfill: FulfillCtxt::default(),
            body_owner,
            results: RefCell::new(TypeckResults::new(body_owner.expect_local())),
            spans: RefCell::new(FxHashMap::default()),
            locals: RefCell::new(FxHashMap::default()),
            ret_tys: RefCell::new(Vec::new()),
            breakables: RefCell::new(Vec::new()),
            diverges: Cell::new(false),
            deferred_casts: RefCell::new(Vec::new()),
            tainted: Cell::new(false),
            errors_at_start: tcx.dcx().err_count(),
        }
    }

    fn selcx(&self) -> SelectionCtxt<'_, 'tcx> {
        SelectionCtxt::new(self.infcx, self.param_env)
    }

    // -----------------------------------------------------------------------
    // Errors
    // -----------------------------------------------------------------------

    /// Has an error been reported while checking this body?
    pub fn is_tainted(&self) -> bool {
        self.tainted.get() || self.tcx.dcx().err_count() > self.errors_at_start
    }

    pub fn err(&self, span: Span, msg: impl Into<String>) -> Diag<'tcx> {
        self.tainted.set(true);
        self.tcx.dcx().struct_span_err(span, msg)
    }

    pub fn ty_str(&self, ty: Ty<'tcx>) -> String {
        self.tcx.ty_to_string(self.infcx.resolve_vars_if_possible(ty))
    }

    /// Has an error made `ty` meaningless (so further errors would only
    /// repeat it)?
    pub fn references_error(&self, ty: Ty<'tcx>) -> bool {
        let ty = self.infcx.resolve_vars_if_possible(ty);
        let mut e = false;
        vsc_infer::walk_tys(ty, &mut |t| e |= matches!(t.kind(), TyKind::Error));
        e
    }

    pub fn report_mismatch(&self, span: Span, expected: Ty<'tcx>, found: Ty<'tcx>) {
        if self.references_error(expected) || self.references_error(found) {
            return;
        }
        self.err(span, "mismatched types")
            .with_code(ErrCode(308))
            .with_span_label(span, format!("expected `{}`, found `{}`", self.ty_str(expected), self.ty_str(found)))
            .emit();
    }

    /// Makes `actual` equal `expected`, or reports a mismatch.
    pub fn demand_eq(&self, span: Span, expected: Ty<'tcx>, actual: Ty<'tcx>) {
        if self.infcx.commit_if_ok(|| self.infcx.unify(expected, actual)).is_err() {
            self.report_mismatch(span, expected, actual);
        }
    }

    // -----------------------------------------------------------------------
    // Recording
    // -----------------------------------------------------------------------

    pub fn write_ty(&self, id: HirId, span: Span, ty: Ty<'tcx>) {
        self.results.borrow_mut().node_types.insert(id, ty);
        self.spans.borrow_mut().insert(id, span);
    }

    pub fn write_args(&self, id: HirId, args: GenericArgsRef<'tcx>) {
        self.results.borrow_mut().node_args.insert(id, args);
    }

    pub fn write_resolution(&self, id: HirId, kind: DefKind, def: DefId) {
        self.results.borrow_mut().type_dependent_defs.insert(id, (kind, def));
    }

    pub fn write_adjustments(&self, id: HirId, adj: Vec<Adjustment<'tcx>>) {
        if !adj.is_empty() {
            self.results.borrow_mut().adjustments.entry(id).or_default().extend(adj);
        }
    }

    pub fn write_field_index(&self, id: HirId, index: usize) {
        self.results.borrow_mut().field_indices.insert(id, index);
    }

    pub fn local_ty(&self, id: HirId) -> Option<Ty<'tcx>> {
        self.locals.borrow().get(&id).copied()
    }

    pub fn declare_local(&self, id: HirId, ty: Ty<'tcx>) {
        self.locals.borrow_mut().insert(id, ty);
    }

    // -----------------------------------------------------------------------
    // Types from signatures and written types
    // -----------------------------------------------------------------------

    /// A type written in the body.
    pub fn lower_ty(&self, t: &hir::Ty) -> Ty<'tcx> {
        let ty = ItemCtxt::new(self.tcx, self.body_owner, ElidedRegions::Body).lower_ty(t);
        let ty = self.infcx.replace_fresh(self.tcx.erase_regions(ty));
        self.normalize(ty, t.span)
    }

    pub fn lower_qpath_ty(&self, q: &hir::QPath, span: Span) -> Ty<'tcx> {
        let ty = ItemCtxt::new(self.tcx, self.body_owner, ElidedRegions::Body).lower_qpath(q, span);
        let ty = self.infcx.replace_fresh(self.tcx.erase_regions(ty));
        self.normalize(ty, span)
    }

    pub fn lower_array_len(&self, c: &hir::AnonConst) -> Const<'tcx> {
        ItemCtxt::new(self.tcx, self.body_owner, ElidedRegions::Body).lower_array_len(c)
    }

    /// Explicit generic arguments for `def`'s own parameters, in a body.
    pub fn lower_own_args(&self, def: DefId, args: &hir::GenericArgs, span: Span) -> GenericArgsRef<'tcx> {
        let lowered = ItemCtxt::new(self.tcx, self.body_owner, ElidedRegions::Body).lower_generic_args(def, Some(args), None, span);
        let v: Vec<_> = lowered
            .iter()
            .map(|a| match *a {
                GenericArg::Type(t) => GenericArg::Type(self.infcx.replace_fresh(self.tcx.erase_regions(t))),
                GenericArg::Region(_) => GenericArg::Region(self.tcx.regions.re_erased),
                c => c,
            })
            .collect();
        self.tcx.mk_args(&v)
    }

    pub fn normalize(&self, ty: Ty<'tcx>, span: Span) -> Ty<'tcx> {
        vsc_trait_selection::normalize(&self.selcx(), &self.fulfill, ty, span)
    }

    /// `def`'s type for these arguments, regions erased and normalized.
    pub fn instantiate_type_of(&self, def: DefId, args: GenericArgsRef<'tcx>, span: Span) -> Ty<'tcx> {
        let ty = self.tcx.erase_regions(self.tcx.instantiate(self.tcx.type_of(def), args));
        self.normalize(ty, span)
    }

    /// A function's signature for these arguments.
    pub fn instantiate_sig(&self, def: DefId, args: GenericArgsRef<'tcx>, span: Span) -> FnSig<'tcx> {
        let sig = self.tcx.fn_sig(def).skip_binder();
        let io: Vec<Ty<'tcx>> = sig
            .inputs_and_output
            .iter()
            .map(|t| {
                let t = self.tcx.erase_regions(self.tcx.instantiate(*t, args));
                self.normalize(t, span)
            })
            .collect();
        FnSig { inputs_and_output: self.tcx.mk_type_list(&io), ..sig }
    }

    /// The signature of the function being checked, in terms of its own
    /// parameters.
    fn liberated_fn_sig(&self, def: DefId) -> FnSig<'tcx> {
        let span = self.tcx.def_span(def);
        self.instantiate_sig(def, self.tcx.identity_args(def), span)
    }

    pub fn register_trait(&self, tr: TraitRef<'tcx>, span: Span) {
        self.fulfill.register(Obligation { kind: ObligationKind::Trait(tr), span });
    }

    /// Registers the item's where clauses for these arguments.
    pub fn register_predicates(&self, def: DefId, args: GenericArgsRef<'tcx>, span: Span) {
        let mut d = Some(def);
        while let Some(x) = d {
            let p = self.tcx.predicates_of(x);
            for (c, _) in p.predicates {
                match self.tcx.erase_regions_clause(self.tcx.instantiate_clause(*c, args)) {
                    Clause::Trait(t) => {
                        // A trait's `Self: Trait` is the caller's obligation
                        // too, so it's kept.
                        self.register_trait(t, span)
                    }
                    Clause::Projection(a, t) => self.fulfill.register(Obligation { kind: ObligationKind::Projection(a, t), span }),
                    _ => {}
                }
            }
            d = p.parent;
        }
    }

    pub fn select_where_possible(&self) {
        let errors = self.fulfill.select_where_possible(&self.selcx());
        self.report_fulfillment_errors(errors);
    }

    /// The type, known well enough to look inside: pending bounds are
    /// proven first. A type still unknown is an error.
    pub fn structurally_resolve(&self, ty: Ty<'tcx>, span: Span) -> Ty<'tcx> {
        let mut t = self.infcx.shallow_resolve(ty);
        if matches!(t.kind(), TyKind::Infer(InferTy::TyVar(_))) {
            self.select_where_possible();
            t = self.infcx.shallow_resolve(ty);
        }
        if matches!(t.kind(), TyKind::Infer(InferTy::TyVar(_))) {
            if !self.is_tainted() {
                self.err(span, "type annotations needed")
                    .with_code(ErrCode(282))
                    .with_span_label(span, "type must be known at this point")
                    .emit();
            }
            self.tainted.set(true);
            let _ = self.infcx.unify(t, self.tcx.types.error);
            return self.tcx.types.error;
        }
        t
    }

    fn report_fulfillment_errors(&self, errors: Vec<FulfillmentError<'tcx>>) {
        for e in errors {
            match e {
                FulfillmentError::Unsatisfied(o) => {
                    let ObligationKind::Trait(tr) = o.kind else { continue };
                    let self_ty = self.infcx.resolve_vars_if_possible(tr.self_ty());
                    if self.references_error(self_ty) {
                        continue;
                    }
                    let tr = TraitRef { def_id: tr.def_id, args: self.infcx.resolve_args(tr.args) };
                    let trait_str = self.tcx.trait_ref_to_string(tr);
                    let ty_str = self.ty_str(self_ty);
                    self.err(o.span, format!("the trait bound `{ty_str}: {trait_str}` is not satisfied"))
                        .with_code(ErrCode(277))
                        .with_span_label(o.span, format!("the trait `{trait_str}` is not implemented for `{ty_str}`"))
                        .emit();
                }
                FulfillmentError::ProjectionMismatch { obligation, normalized } => {
                    let ObligationKind::Projection(alias, ty) = obligation.kind else { continue };
                    if self.references_error(normalized) || self.references_error(ty) {
                        continue;
                    }
                    let alias_ty = self.tcx.mk_ty(TyKind::Alias(AliasTy { def_id: alias.def_id, args: self.infcx.resolve_args(alias.args) }));
                    self.err(obligation.span, format!("type mismatch resolving `{} == {}`", self.ty_str(alias_ty), self.ty_str(ty)))
                        .with_code(ErrCode(271))
                        .with_span_label(obligation.span, format!("expected `{}`, found `{}`", self.ty_str(ty), self.ty_str(normalized)))
                        .emit();
                }
                FulfillmentError::Ambiguous(o) => {
                    if self.is_tainted() {
                        continue;
                    }
                    let what = match o.kind {
                        ObligationKind::Trait(tr) => {
                            let tr = TraitRef { def_id: tr.def_id, args: self.infcx.resolve_args(tr.args) };
                            format!("{}: {}", self.ty_str(tr.self_ty()), self.tcx.trait_ref_to_string(tr))
                        }
                        ObligationKind::Projection(a, t) => {
                            let alias_ty = self.tcx.mk_ty(TyKind::Alias(a));
                            format!("{} == {}", self.ty_str(alias_ty), self.ty_str(t))
                        }
                    };
                    self.err(o.span, "type annotations needed")
                        .with_code(ErrCode(283))
                        .with_span_label(o.span, format!("cannot satisfy `{what}`"))
                        .emit();
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Bodies
    // -----------------------------------------------------------------------

    fn check_fn_body(&self, body: &hir::Body, inputs: &[Ty<'tcx>], output: Ty<'tcx>) {
        for (param, &ty) in body.params.iter().zip(inputs) {
            self.check_pat_top(&param.pat, ty);
        }
        self.ret_tys.borrow_mut().push(output);
        self.check_expr_coercible_to_type(&body.value, output);
        self.ret_tys.borrow_mut().pop();
    }

    /// Breakable scopes: `loop` and labeled blocks.
    fn with_breakable<R>(&self, id: HirId, target: Option<Ty<'tcx>>, f: impl FnOnce() -> R) -> (R, bool) {
        self.breakables.borrow_mut().push(Breakable { id, target, may_break: false });
        let r = f();
        let b = self.breakables.borrow_mut().pop().unwrap();
        (r, b.may_break)
    }

    /// Resolves everything and builds the results.
    fn finish(self) -> TypeckResults<'tcx> {
        let tcx = self.tcx;
        self.select_where_possible();
        // Unsuffixed literals that nothing constrained: `int32` and
        // `float64`, as in Rust.
        for v in self.infcx.unbound_numeric_vars() {
            let default = match v.kind() {
                TyKind::Infer(InferTy::IntVar(_)) => tcx.mk_ty(TyKind::Int(IntTy::I32)),
                _ => tcx.mk_ty(TyKind::Float(vsc_hir::def::FloatTy::F64)),
            };
            let _ = self.infcx.unify(v, default);
        }
        let errors = self.fulfill.select_all_or_error(&self.selcx());
        self.report_fulfillment_errors(errors);
        for (span, from, to) in std::mem::take(&mut *self.deferred_casts.borrow_mut()) {
            self.check_cast(span, from, to);
        }

        let mut reported = self.is_tainted();
        let mut results = self.results.into_inner();
        let spans = self.spans.into_inner();
        let infcx = self.infcx;
        let mut resolve = |ty: Ty<'tcx>, span: Option<Span>| -> Ty<'tcx> {
            let r = infcx.resolve_vars_if_possible(ty);
            if infcx.has_infer(r) {
                if !reported {
                    if let Some(span) = span {
                        tcx.dcx()
                            .struct_span_err(span, "type annotations needed")
                            .with_code(ErrCode(282))
                            .with_span_label(span, format!("cannot infer the type: `{}`", tcx.ty_to_string(r)))
                            .emit();
                        reported = true;
                    }
                }
                return replace_infer_with_error(tcx, r);
            }
            r
        };
        let mut ids: Vec<HirId> = results.node_types.keys().copied().collect();
        ids.sort_by_key(|id| spans.get(id).map(|s| (s.lo(), s.hi())));
        for id in ids {
            let ty = results.node_types[&id];
            let r = resolve(ty, spans.get(&id).copied());
            results.node_types.insert(id, r);
        }
        for args in results.node_args.values_mut() {
            *args = resolve_args_or_error(tcx, infcx, args);
        }
        for adj in results.adjustments.values_mut() {
            for a in adj.iter_mut() {
                a.target = replace_infer_with_error(tcx, infcx.resolve_vars_if_possible(a.target));
            }
        }
        for tys in results.pat_adjustments.values_mut() {
            for t in tys.iter_mut() {
                *t = replace_infer_with_error(tcx, infcx.resolve_vars_if_possible(*t));
            }
        }
        for sig in results.closure_sigs.values_mut() {
            let io: Vec<_> =
                sig.value.inputs_and_output.iter().map(|t| replace_infer_with_error(tcx, infcx.resolve_vars_if_possible(*t))).collect();
            sig.value.inputs_and_output = tcx.mk_type_list(&io);
        }
        results.tainted_by_errors = reported;
        results
    }
}

fn resolve_args_or_error<'tcx>(tcx: TyCtxt<'tcx>, infcx: &InferCtxt<'tcx>, args: GenericArgsRef<'tcx>) -> GenericArgsRef<'tcx> {
    let v: Vec<_> = args
        .iter()
        .map(|a| match *a {
            GenericArg::Type(t) => GenericArg::Type(replace_infer_with_error(tcx, infcx.resolve_vars_if_possible(t))),
            other => other,
        })
        .collect();
    tcx.mk_args(&v)
}

fn replace_infer_with_error<'tcx>(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
    use vsc_middle::ty::fold::{TypeFolder, super_fold_ty};
    struct R<'tcx>(TyCtxt<'tcx>);
    impl<'tcx> TypeFolder<'tcx> for R<'tcx> {
        fn tcx(&self) -> TyCtxt<'tcx> {
            self.0
        }
        fn fold_ty(&mut self, t: Ty<'tcx>) -> Ty<'tcx> {
            match t.kind() {
                TyKind::Infer(_) => self.0.types.error,
                _ => super_fold_ty(self, t),
            }
        }
    }
    R(tcx).fold_ty(ty)
}
