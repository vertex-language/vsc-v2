//! Method lookup (rustc's `method/probe.rs` and `method/confirm.rs`), and
//! associated items named through a type (`Task.New`).
//!
//! The receiver's type is dereferenced step by step (through references,
//! and through a class value to its class, exception 3). At each step the
//! method's `self` is tried by value, then by `&`, then by `&mut`; inherent
//! methods win over trait methods, as in rustc.
//!
//! Vertex has no `use`, so every trait in the crate is in scope.
//!
//! Probing only unifies: it never normalizes or registers obligations, since
//! a probe is rolled back and its variables with it.

use vsc_errors::ErrCode;
use vsc_hir::def::DefKind;
use vsc_hir::{self as hir};
use vsc_middle::ty::*;
use vsc_span::def_id::DefId;
use vsc_span::{Ident, Span};
use vsc_trait_selection::Selection;

use crate::FnCtxt;

#[derive(Clone, Copy, Debug)]
enum Candidate {
    Inherent { imp: DefId, item: DefId },
    Trait { trait_def: DefId, item: DefId },
}

impl Candidate {
    fn item(self) -> DefId {
        match self {
            Candidate::Inherent { item, .. } | Candidate::Trait { item, .. } => item,
        }
    }
}

pub struct Pick<'tcx> {
    pub item: DefId,
    pub args: GenericArgsRef<'tcx>,
    /// The receiver's type after each dereference.
    pub derefs: Vec<Ty<'tcx>>,
    pub autoref: Option<Mutability>,
    /// The last step was `[T; N]` to `[T]`.
    pub unsize: bool,
    pub adjusted_self: Ty<'tcx>,
    pub trait_def: Option<DefId>,
}

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    /// A signature for these arguments, regions erased, not normalized
    /// (safe inside a probe).
    pub(crate) fn raw_sig(&self, def: DefId, args: GenericArgsRef<'tcx>) -> FnSig<'tcx> {
        let sig = self.tcx.fn_sig(def).skip_binder();
        let io: Vec<_> = sig.inputs_and_output.iter().map(|t| self.tcx.erase_regions(self.tcx.instantiate(*t, args))).collect();
        FnSig { inputs_and_output: self.tcx.mk_type_list(&io), ..sig }
    }

    fn raw_type_of(&self, def: DefId, args: GenericArgsRef<'tcx>) -> Ty<'tcx> {
        self.tcx.erase_regions(self.tcx.instantiate(self.tcx.type_of(def), args))
    }

    fn inherent_impls(&self) -> Vec<DefId> {
        self.tcx
            .defs
            .iter_enumerated()
            .filter(|(_, d)| d.kind == DefKind::Impl { of_trait: false })
            .map(|(id, _)| id.to_def_id())
            .collect()
    }

    fn traits_with_item(&self, name: Ident, want: impl Fn(AssocKind) -> bool) -> Vec<(DefId, DefId)> {
        let tcx = self.tcx;
        tcx.defs
            .iter_enumerated()
            .filter(|(_, d)| d.kind == DefKind::Trait)
            .filter_map(|(id, _)| {
                let t = id.to_def_id();
                tcx.associated_items(t).iter().find(|i| i.name == name.name && want(i.kind)).map(|i| (t, i.def_id))
            })
            .collect()
    }

    /// Does `ty` implement the trait (or might it, once types are known)?
    fn may_implement(&self, ty: Ty<'tcx>, trait_def: DefId) -> bool {
        let tcx = self.tcx;
        self.infcx.probe(|| {
            let mut args = self.infcx.fresh_args_for_item(trait_def).to_vec();
            args[0] = GenericArg::Type(ty);
            let tr = TraitRef { def_id: trait_def, args: tcx.mk_args(&args) };
            !matches!(self.selcx().select(tr, tcx.def_span(trait_def)), Selection::NoSolution)
        })
    }

    /// The candidates for a method or associated item named `name` on
    /// `ty`: inherent ones, then trait ones.
    fn candidates(&self, ty: Ty<'tcx>, name: Ident, want: impl Fn(AssocKind) -> bool + Copy) -> (Vec<Candidate>, Vec<Candidate>) {
        let tcx = self.tcx;
        let infcx = self.infcx;
        let mut inherent = Vec::new();
        for imp in self.inherent_impls() {
            let Some(item) = tcx.associated_items(imp).iter().find(|i| i.name == name.name && want(i.kind)) else { continue };
            let matches = infcx.probe(|| {
                let args = infcx.fresh_args_for_item(imp);
                infcx.unify(self.raw_type_of(imp, args), ty).is_ok()
            });
            if matches {
                inherent.push(Candidate::Inherent { imp, item: item.def_id });
            }
        }
        let mut traits = Vec::new();
        let resolved = infcx.shallow_resolve(ty);
        if matches!(resolved.kind(), TyKind::Param(_) | TyKind::Alias(_)) {
            for c in &self.param_env_clauses() {
                if let Clause::Trait(tr) = c {
                    if infcx.probe(|| infcx.unify(tr.self_ty(), ty).is_ok()) {
                        if let Some(item) = tcx.associated_items(tr.def_id).iter().find(|i| i.name == name.name && want(i.kind)) {
                            if !traits.iter().any(|c: &Candidate| c.item() == item.def_id) {
                                traits.push(Candidate::Trait { trait_def: tr.def_id, item: item.def_id });
                            }
                        }
                    }
                }
            }
        } else {
            for (trait_def, item) in self.traits_with_item(name, want) {
                if self.may_implement(ty, trait_def) {
                    traits.push(Candidate::Trait { trait_def, item });
                }
            }
        }
        (inherent, traits)
    }

    fn param_env_clauses(&self) -> Vec<Clause<'tcx>> {
        self.param_env.clauses.clone()
    }

    /// Unifies a candidate with the step's type and, for a method, its
    /// `self` with the adjusted receiver. Returns the item's arguments.
    fn try_candidate(
        &self,
        c: Candidate,
        step: Ty<'tcx>,
        adjusted: Option<Ty<'tcx>>,
        explicit: Option<GenericArgsRef<'tcx>>,
    ) -> Option<GenericArgsRef<'tcx>> {
        let tcx = self.tcx;
        let infcx = self.infcx;
        let item = c.item();
        let mut args = infcx.fresh_args_for_item(item).to_vec();
        match c {
            Candidate::Inherent { imp, .. } => {
                let impl_args = infcx.fresh_args_for_item(imp);
                infcx.unify(self.raw_type_of(imp, impl_args), step).ok()?;
                args[..impl_args.len()].copy_from_slice(impl_args);
            }
            Candidate::Trait { .. } => {
                infcx.unify(args[0].expect_ty(), step).ok()?;
            }
        }
        if let Some(explicit) = explicit {
            let parent = tcx.generics_of(item).parent_count;
            if explicit.len() == args.len() {
                args[parent..].copy_from_slice(&explicit[parent..]);
            }
        }
        let args = tcx.mk_args(&args);
        if let Some(adjusted) = adjusted {
            let sig = self.raw_sig(item, args);
            let first = *sig.inputs().first()?;
            infcx.unify(first, adjusted).ok()?;
        }
        Some(args)
    }

    /// The method `name` on a receiver of type `recv_ty`.
    pub fn probe_method(&self, recv_ty: Ty<'tcx>, name: Ident, explicit: Option<GenericArgsRef<'tcx>>) -> Option<Pick<'tcx>> {
        let tcx = self.tcx;
        let infcx = self.infcx;
        let is_method = |k: AssocKind| matches!(k, AssocKind::Fn { has_self: true });
        let mut steps: Vec<(Ty<'tcx>, bool)> = Vec::new();
        let mut t = infcx.shallow_resolve(recv_ty);
        for _ in 0..10 {
            steps.push((t, false));
            match self.autoderef_step(t) {
                Some(next) => t = infcx.shallow_resolve(next),
                None => break,
            }
        }
        if let TyKind::Array(elem, _) = *t.kind() {
            steps.push((tcx.mk_ty(TyKind::Slice(elem)), true));
        }
        for (i, &(step, unsize)) in steps.iter().enumerate() {
            if matches!(step.kind(), TyKind::Infer(InferTy::TyVar(_)) | TyKind::Error) {
                break;
            }
            let (inherent, traits) = self.candidates(step, name, is_method);
            for autoref in [None, Some(Mutability::Not), Some(Mutability::Mut)] {
                let adjusted = match autoref {
                    None => step,
                    Some(m) => tcx.mk_ref(tcx.regions.re_erased, step, m),
                };
                for group in [&inherent, &traits] {
                    let applicable: Vec<Candidate> =
                        group.iter().copied().filter(|&c| infcx.probe(|| self.try_candidate(c, step, Some(adjusted), explicit).is_some())).collect();
                    let Some(&first) = applicable.first() else { continue };
                    if applicable.len() > 1 {
                        let names: Vec<String> = applicable.iter().map(|c| format!("`{}`", tcx.def_path_str(c.item()))).collect();
                        self.err(name.span, "multiple applicable items in scope")
                            .with_code(ErrCode(34))
                            .with_span_label(name.span, format!("multiple `{name}` found"))
                            .with_note(format!("candidates: {}", names.join(", ")))
                            .emit();
                    }
                    let args = self.try_candidate(first, step, Some(adjusted), explicit)?;
                    let derefs = steps[1..=i].iter().filter(|(_, u)| !u).map(|(t, _)| *t).collect();
                    let trait_def = match first {
                        Candidate::Trait { trait_def, .. } => Some(trait_def),
                        Candidate::Inherent { .. } => None,
                    };
                    return Some(Pick { item: first.item(), args, derefs, autoref, unsize, adjusted_self: adjusted, trait_def });
                }
            }
        }
        None
    }

    pub fn check_method_call(&self, e: &hir::Expr, seg: &hir::PathSegment, recv: &hir::Expr, args: &[hir::Expr], span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        let recv_ty = self.check_expr(recv);
        let recv_resolved = self.structurally_resolve(recv_ty, recv.span);
        if recv_resolved == tcx.types.error {
            for a in args {
                self.check_expr(a);
            }
            return tcx.types.error;
        }
        let explicit = seg.args.as_deref().map(|a| {
            // The method isn't known yet, so its own parameters' count isn't
            // either; the arguments are lowered once it is.
            a
        });
        let pick = self.probe_method(recv_resolved, seg.ident, None);
        let Some(mut pick) = pick else {
            for a in args {
                self.check_expr(a);
            }
            self.report_no_method(recv_resolved, seg.ident);
            return tcx.types.error;
        };
        if let Some(explicit) = explicit {
            let own = self.lower_own_args(pick.item, explicit, span);
            let parent = tcx.generics_of(pick.item).parent_count;
            let mut v = pick.args.to_vec();
            if own.len() == v.len() {
                for i in parent..v.len() {
                    if let (GenericArg::Type(a), GenericArg::Type(b)) = (v[i], own[i]) {
                        self.demand_eq(span, a, b);
                    }
                    v[i] = own[i];
                }
            }
            pick.args = tcx.mk_args(&v);
        }

        // The receiver's adjustments.
        let mut adj: Vec<Adjustment<'tcx>> = pick.derefs.iter().map(|&t| Adjustment { kind: Adjust::Deref, target: t }).collect();
        if let Some(m) = pick.autoref {
            adj.push(Adjustment { kind: Adjust::Borrow(m), target: pick.adjusted_self });
        }
        if pick.unsize {
            adj.push(Adjustment { kind: Adjust::Unsize, target: pick.adjusted_self });
        }
        self.write_adjustments(recv.hir_id, adj);
        self.write_resolution(e.hir_id, DefKind::AssocFn, pick.item);
        self.write_args(e.hir_id, pick.args);
        self.register_predicates(pick.item, pick.args, span);
        let _ = pick.trait_def;

        let sig = self.instantiate_sig(pick.item, pick.args, span);
        if let Some(&first) = sig.inputs().first() {
            self.demand_eq(recv.span, first, pick.adjusted_self);
        }
        let inputs = sig.inputs().get(1..).unwrap_or(&[]);
        self.check_args(span, &sig, inputs, args, "method");
        sig.output()
    }

    fn report_no_method(&self, ty: Ty<'tcx>, name: Ident) {
        let tcx = self.tcx;
        let descr = match *self.infcx.shallow_resolve(ty).kind() {
            TyKind::Adt(d, _) => d.descr(),
            TyKind::Param(_) => "type parameter",
            TyKind::Ref(..) => "reference",
            _ => "type",
        };
        let mut err = self
            .err(name.span, format!("no method named `{name}` found for {descr} `{}` in the current scope", self.ty_str(ty)))
            .with_code(ErrCode(599))
            .with_span_label(name.span, format!("method not found in `{}`", self.ty_str(ty)));
        // A function, not a method: called through the type.
        let (inherent, _) = self.infcx.probe(|| self.candidates(ty, name, |k| matches!(k, AssocKind::Fn { has_self: false })));
        if let Some(c) = inherent.first() {
            err = err.with_help(format!("`{}` takes no `self`: call it as `{}(…)`", name, tcx.def_path_str(c.item())));
        }
        err.emit();
    }

    /// `Type.Name`: an associated function or constant.
    pub fn lookup_assoc_item(
        &self,
        self_ty: Ty<'tcx>,
        name: Ident,
        explicit: Option<&hir::GenericArgs>,
        span: Span,
    ) -> Option<(DefKind, DefId, GenericArgsRef<'tcx>)> {
        let tcx = self.tcx;
        let want = |k: AssocKind| !matches!(k, AssocKind::Type);
        let (inherent, traits) = self.candidates(self_ty, name, want);
        let c = inherent.first().or(traits.first()).copied()?;
        if inherent.len() > 1 {
            self.err(name.span, "multiple applicable items in scope").with_code(ErrCode(34)).emit();
        }
        let explicit = explicit.map(|a| self.lower_own_args(c.item(), a, span));
        let args = self.try_candidate(c, self_ty, None, explicit)?;
        let kind = tcx.def_kind(c.item());
        Some((kind, c.item(), args))
    }
}
