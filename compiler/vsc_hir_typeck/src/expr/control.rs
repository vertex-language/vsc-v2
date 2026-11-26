//! Control flow (`if`, `match`, blocks, statements), `.await`, and closures.

use vsc_errors::ErrCode;
use vsc_hir::{self as hir, LangItem};
use vsc_middle::ty::*;
use vsc_trait_selection::{Obligation, ObligationKind};

use super::CoerceMany;
use crate::{Expectation, FnCtxt};

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    pub(super) fn check_if(&self, cond: &hir::Expr, then: &hir::Expr, els: Option<&hir::Expr>, expected: Expectation<'tcx>) -> Ty<'tcx> {
        let tcx = self.tcx;
        self.check_expr_coercible_to_type(cond, tcx.types.bool);
        let cond_diverges = self.diverges.replace(false);
        match els {
            None => {
                // Exception 10: the block's value, if any, is discarded.
                self.check_expr_coercible_to_type(then, tcx.types.unit);
                self.diverges.set(cond_diverges);
                tcx.types.unit
            }
            Some(els) => {
                let mut many = CoerceMany { target: expected.only_has_type() };
                let tt = self.check_expr_with_expectation(then, expected);
                let then_diverges = self.diverges.replace(false);
                self.coerce_many(&mut many, then, tt, "`if` and `else` have incompatible types");
                let et = self.check_expr_with_expectation(els, expected);
                let else_diverges = self.diverges.get();
                self.coerce_many(&mut many, els, et, "`if` and `else` have incompatible types");
                self.diverges.set(cond_diverges || (then_diverges && else_diverges));
                many.target.unwrap_or(tcx.types.never)
            }
        }
    }

    pub(super) fn check_match(&self, scrutinee: &hir::Expr, arms: &[hir::Arm], expected: Expectation<'tcx>) -> Ty<'tcx> {
        let tcx = self.tcx;
        let st = self.check_expr(scrutinee);
        let scrutinee_diverges = self.diverges.replace(false);
        let mut many = CoerceMany { target: expected.only_has_type() };
        let mut all_diverge = true;
        for arm in arms {
            self.diverges.set(false);
            self.check_pat_top(&arm.pat, st);
            if let Some(g) = &arm.guard {
                self.check_expr_coercible_to_type(g, tcx.types.bool);
            }
            let t = self.check_expr_with_expectation(&arm.body, expected);
            all_diverge &= self.diverges.get();
            self.coerce_many(&mut many, &arm.body, t, "`match` arms have incompatible types");
        }
        self.diverges.set(scrutinee_diverges || all_diverge);
        many.target.unwrap_or(tcx.types.never)
    }

    pub(super) fn coerce_many(&self, many: &mut CoerceMany<'tcx>, e: &hir::Expr, ty: Ty<'tcx>, msg: &str) {
        let resolved = self.infcx.shallow_resolve(ty);
        match many.target {
            None if matches!(resolved.kind(), TyKind::Never) => {}
            None => many.target = Some(ty),
            Some(target) => {
                if self.try_coerce(Some(e.hir_id), ty, target).is_ok() {
                    return;
                }
                // The earlier branches may coerce to this one's type
                // (`never` first, say); otherwise they don't fit.
                if self.infcx.commit_if_ok(|| self.infcx.unify(target, ty)).is_err() {
                    if self.references_error(target) || self.references_error(ty) {
                        return;
                    }
                    self.err(e.span, msg)
                        .with_code(ErrCode(308))
                        .with_span_label(e.span, format!("expected `{}`, found `{}`", self.ty_str(target), self.ty_str(ty)))
                        .emit();
                }
            }
        }
    }

    pub fn check_block(&self, b: &hir::Block, expected: Expectation<'tcx>) -> Ty<'tcx> {
        let tcx = self.tcx;
        for s in &b.stmts {
            self.check_stmt(s);
        }
        let expects_unit = expected.only_has_type().is_some_and(|t| self.infcx.shallow_resolve(t) == tcx.types.unit);
        let ty = match &b.expr {
            // Exception 10: a block expected to be `()` discards its value.
            Some(tail) if expects_unit => {
                let t = self.check_expr(tail);
                if matches!(self.infcx.shallow_resolve(t).kind(), TyKind::Never) { tcx.types.never } else { tcx.types.unit }
            }
            Some(tail) => self.check_expr_with_expectation(tail, expected),
            None if self.diverges.get() => tcx.types.never,
            None => tcx.types.unit,
        };
        self.write_ty(b.hir_id, b.span, ty);
        ty
    }

    pub(super) fn check_stmt(&self, s: &hir::Stmt) {
        let tcx = self.tcx;
        match &s.kind {
            hir::StmtKind::Let(l) => {
                let declared = l.ty.as_ref().map(|t| self.lower_ty(t));
                let ty = match (&l.init, declared) {
                    (Some(init), Some(d)) => {
                        self.check_expr_coercible_to_type(init, d);
                        d
                    }
                    (Some(init), None) => self.check_expr(init),
                    (None, Some(d)) => d,
                    (None, None) => self.infcx.next_ty_var(),
                };
                self.write_ty(l.hir_id, l.span, ty);
                self.check_pat_top(&l.pat, ty);
                if let Some(els) = &l.els {
                    let before = self.diverges.replace(false);
                    let t = self.check_block(els, Expectation::None);
                    if !matches!(self.infcx.shallow_resolve(t).kind(), TyKind::Never) && !self.diverges.get() {
                        self.err(els.span, "`else` clause of `let...else` does not diverge")
                            .with_code(ErrCode(308))
                            .with_help("try adding a diverging expression, such as `return` or `panic()`")
                            .emit();
                    }
                    self.diverges.set(before);
                }
            }
            hir::StmtKind::Item(_) => {}
            hir::StmtKind::Semi(e) => {
                // A block-like statement (`if`, `match`, a block, a loop) is
                // `()`, as in Rust without a `;`. Exception 10 then lets
                // the blocks inside discard their values.
                let block_like =
                    matches!(e.kind, hir::ExprKind::Block(..) | hir::ExprKind::Loop(..) | hir::ExprKind::If(..) | hir::ExprKind::Match(..));
                if block_like {
                    self.check_expr_coercible_to_type(e, tcx.types.unit);
                } else {
                    self.check_expr(e);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Function literals
    // -----------------------------------------------------------------------

    pub(super) fn check_await(&self, e: &hir::Expr, x: &hir::Expr) -> Ty<'tcx> {
        let tcx = self.tcx;
        let expr_ty = self.check_expr(x);
        if !self.in_async.get() {
            self.err(e.span, "`.await` is only allowed inside `async` functions and blocks")
                .with_code(ErrCode(728))
                .with_span_label(e.span, "only allowed inside `async` functions and blocks")
                .emit();
            return tcx.types.error;
        }
        let expr_ty = self.structurally_resolve(expr_ty, x.span);
        if expr_ty == tcx.types.error {
            return tcx.types.error;
        }
        let Some(future_trait) = tcx.require_lang_item(LangItem::Future, e.span) else {
            return tcx.types.error;
        };
        let Some(future_output) = tcx.require_lang_item(LangItem::FutureOutput, e.span) else {
            return tcx.types.error;
        };
        let Some(poll_def) = tcx.require_lang_item(LangItem::FuturePoll, e.span) else {
            return tcx.types.error;
        };

        // Check whether `expr_ty` directly implements `Future`; otherwise convert via `IntoFuture`.
        let direct_future = self.infcx.probe(|| {
            let tr = TraitRef { def_id: future_trait, args: tcx.mk_args(&[GenericArg::Type(expr_ty)]) };
            matches!(self.selcx().select(tr, e.span), vsc_trait_selection::Selection::Selected(..))
        });

        let (into_future_def, into_future_args, future_ty) = if direct_future {
            let tr = TraitRef { def_id: future_trait, args: tcx.mk_args(&[GenericArg::Type(expr_ty)]) };
            self.fulfill.register(Obligation { kind: ObligationKind::Trait(tr), span: e.span });
            (None, tcx.mk_args(&[]), expr_ty)
        } else if let (Some(into_trait), Some(into_method)) = (
            tcx.lang_items().get(LangItem::IntoFuture),
            tcx.lang_items().get(LangItem::IntoFutureIntoFuture),
        ) {
            let args = tcx.mk_args(&[GenericArg::Type(expr_ty)]);
            let tr = TraitRef { def_id: into_trait, args };
            self.fulfill.register(Obligation { kind: ObligationKind::Trait(tr), span: e.span });
            let into_assoc = tcx
                .associated_items(into_trait)
                .iter()
                .find(|i| i.kind == AssocKind::Type && i.name.as_str() == "IntoFuture")
                .map(|i| i.def_id);
            let fut_ty = match into_assoc {
                Some(assoc) => {
                    let alias = AliasTy { def_id: assoc, args };
                    self.normalize(tcx.mk_ty(TyKind::Alias(alias)), e.span)
                }
                None => expr_ty,
            };
            (Some(into_method), args, fut_ty)
        } else {
            let tr = TraitRef { def_id: future_trait, args: tcx.mk_args(&[GenericArg::Type(expr_ty)]) };
            self.fulfill.register(Obligation { kind: ObligationKind::Trait(tr), span: e.span });
            (None, tcx.mk_args(&[]), expr_ty)
        };

        let poll_args = tcx.mk_args(&[GenericArg::Type(future_ty)]);
        let output_alias = AliasTy { def_id: future_output, args: poll_args };
        let output_ty = self.normalize(tcx.mk_ty(TyKind::Alias(output_alias)), e.span);
        self.results.borrow_mut().await_info.insert(
            e.hir_id,
            AwaitInfo {
                into_future_def,
                into_future_args,
                future_ty,
                poll_def,
                poll_args,
                output_ty,
            },
        );
        output_ty
    }

    pub(super) fn check_closure(&self, e: &hir::Expr, c: &hir::Closure, expected: Expectation<'tcx>) -> Ty<'tcx> {
        let tcx = self.tcx;
        if matches!(c.kind, hir::ClosureKind::AsyncBlock | hir::ClosureKind::AsyncFn) {
            let out_ty = if c.kind == hir::ClosureKind::AsyncFn {
                // Constrain the inner coroutine's output type from the enclosing `async func`'s declared return type.
                let mut declared = None;
                if let Some(&(opaque_local, _, _)) = self.pending_opaques.borrow().last() {
                    if let Some(future_output) = tcx.lang_items().get(LangItem::FutureOutput) {
                        for &(clause, _) in tcx.item_bounds(opaque_local.to_def_id()).predicates {
                            if let Clause::Projection(alias, term) = tcx.erase_regions_clause(clause) {
                                if alias.def_id == future_output {
                                    declared = Some(term);
                                    break;
                                }
                            }
                        }
                    }
                }
                declared.unwrap_or_else(|| self.infcx.next_ty_var())
            } else {
                self.infcx.next_ty_var()
            };
            let sig = FnSig {
                inputs_and_output: tcx.mk_type_list(&[out_ty]),
                c_variadic: false,
                safety: Safety::Safe,
                abi: None,
            };
            self.results.borrow_mut().closure_sigs.insert(c.def_id, Binder::dummy(sig));
            let body = tcx.hir_crate.body(c.body);
            let saved_div = self.diverges.replace(false);
            let saved_async = self.in_async.replace(true);
            self.check_fn_body(body, &[], out_ty);
            self.in_async.set(saved_async);
            self.diverges.set(saved_div);
            let out_ty = self.infcx.resolve_vars_if_possible(out_ty);
            let sig = FnSig {
                inputs_and_output: tcx.mk_type_list(&[out_ty]),
                c_variadic: false,
                safety: Safety::Safe,
                abi: None,
            };
            self.results.borrow_mut().closure_sigs.insert(c.def_id, Binder::dummy(sig));
            let upvars = self.coroutine_upvar_tys(body);
            self.infcx.coroutine_upvars.borrow_mut().insert(c.def_id.to_def_id(), upvars);
            let parent_args = tcx.identity_args(tcx.parent(c.def_id.to_def_id()));
            let mut coro_args = parent_args.to_vec();
            coro_args.push(GenericArg::Type(out_ty));
            return tcx.mk_ty(TyKind::Coroutine(c.def_id.to_def_id(), tcx.mk_args(&coro_args)));
        }
        if c.kind == hir::ClosureKind::AsyncClosure {
            self.err(e.span, "`async func(...)` closures aren't supported yet").with_span_label(e.span, "not supported yet").emit();
            return tcx.types.error;
        }
        let mut io: Vec<Ty<'tcx>> = c.fn_decl.inputs.iter().map(|t| self.lower_ty(t)).collect();
        io.push(match &c.fn_decl.output {
            hir::FnRetTy::Return(t) => self.lower_ty(t),
            hir::FnRetTy::DefaultReturn(_) => self.infcx.next_ty_var(),
        });
        let sig = FnSig { inputs_and_output: tcx.mk_type_list(&io), c_variadic: false, safety: Safety::Safe, abi: None };
        // The expected type fills in what isn't written: `func(x) { … }`
        // passed where a `func(int64) -> int64` is wanted.
        if let Some(exp) = expected.only_has_type() {
            let exp = self.infcx.shallow_resolve(exp);
            let exp_sig = match *exp.kind() {
                TyKind::FnPtr(s) => Some(s.value),
                TyKind::Adt(d, a) if tcx.is_lang_item(d.did(), LangItem::Func) && a.len() == 1 => {
                    match *self.infcx.shallow_resolve(a[0].expect_ty()).kind() {
                        TyKind::FnPtr(s) => Some(s.value),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(s) = exp_sig {
                if s.inputs_and_output.len() == io.len() {
                    for (a, b) in io.iter().zip(s.inputs_and_output.iter()) {
                        let _ = self.infcx.commit_if_ok(|| self.infcx.unify(*a, *b));
                    }
                }
            }
        }
        self.results.borrow_mut().closure_sigs.insert(c.def_id, Binder::dummy(sig));
        let body = tcx.hir_crate.body(c.body);
        let saved = self.diverges.replace(false);
        self.check_fn_body(body, sig.inputs(), sig.output());
        self.diverges.set(saved);
        let (kind, mutated) = self.closure_kind(body);
        self.closure_mutations.borrow_mut().insert(c.def_id, mutated);
        self.closure_decl_spans.borrow_mut().insert(c.def_id, c.fn_decl_span);
        self.infcx.closure_kinds.borrow_mut().insert(c.def_id.to_def_id(), kind);
        self.results.borrow_mut().closure_kinds.insert(c.def_id, kind);
        let sig_ty = tcx.mk_ty(TyKind::FnPtr(Binder::dummy(sig)));
        tcx.mk_ty(TyKind::Closure(c.def_id.to_def_id(), tcx.mk_args(&[GenericArg::Type(sig_ty)])))
    }
}
