//! Path expression resolution and function / overloaded call checking.

use vsc_errors::ErrCode;
use vsc_hir::def::{CtorKind, DefKind, Res};
use vsc_hir::{self as hir, HirId, LangItem};
use vsc_middle::ty::*;
use vsc_span::Span;
use vsc_span::def_id::DefId;

use super::{callee_text, ty_text};
use crate::coercion::closure_sig;
use crate::FnCtxt;

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    pub fn check_expr_path(&self, q: &hir::QPath, id: HirId, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        match q {
            hir::QPath::Resolved(None, path) => {
                let mut seg_args = path.segments.last().and_then(|s| s.args.as_deref());
                // A variant's arguments may also go on its enum, as in Rust:
                // `Option<int32>.None`.
                if seg_args.is_none() && matches!(path.res, Res::Def(DefKind::Ctor(vsc_hir::def::CtorOf::Variant, _), _)) {
                    let n = path.segments.len();
                    seg_args = n.checked_sub(2).and_then(|i| path.segments[i].args.as_deref());
                }
                self.value_path(path.res, seg_args, id, span)
            }
            hir::QPath::Resolved(Some(qself), path) => {
                // `<T as Trait>.method`
                let self_ty = self.lower_ty(qself);
                let Res::Def(kind @ (DefKind::AssocFn | DefKind::AssocConst), item) = path.res else { return tcx.types.error };
                let trait_def = tcx.parent(item);
                let mut args: Vec<GenericArg<'tcx>> = self.infcx.fresh_args_for_item(item).to_vec();
                args[0] = GenericArg::Type(self_ty);
                let args = tcx.mk_args(&args);
                if kind == DefKind::AssocConst {
                    let n = tcx.generics_of(trait_def).count();
                    self.register_trait(TraitRef { def_id: trait_def, args: tcx.mk_args(&args[..n]) }, span);
                }
                self.write_args(id, args);
                self.item_value_ty(kind, item, args, span)
            }
            hir::QPath::TypeRelative(ty, seg) => {
                let self_ty = self.lower_ty(ty);
                self.type_relative_value(self_ty, seg, id, span)
            }
            hir::QPath::LangItem(item, span) => self.lang_item_value(*item, id, *span),
        }
    }

    /// The type of a function, constant or constructor for `args`.
    pub(super) fn item_value_ty(&self, kind: DefKind, def: DefId, args: GenericArgsRef<'tcx>, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        match kind {
            DefKind::Fn | DefKind::AssocFn | DefKind::Ctor(_, CtorKind::Fn) => {
                self.register_predicates(def, args, span);
                tcx.mk_fn_def(def, args)
            }
            DefKind::Ctor(_, CtorKind::Const) => self.instantiate_type_of(def, args, span),
            _ => self.instantiate_type_of(def, args, span),
        }
    }

    /// Arguments for `def` from new variables, with explicit arguments for
    /// its own parameters when written.
    pub(super) fn args_for(&self, def: DefId, explicit: Option<&hir::GenericArgs>, span: Span) -> GenericArgsRef<'tcx> {
        let fresh = self.infcx.fresh_args_for_item(def);
        let Some(explicit) = explicit else { return fresh };
        let g = self.tcx.generics_of(def);
        let own = self.lower_own_args(def, explicit, span);
        let mut v = fresh.to_vec();
        v[g.parent_count..].copy_from_slice(&own[g.parent_count..]);
        self.tcx.mk_args(&v)
    }

    pub(super) fn value_path(&self, res: Res<HirId>, explicit: Option<&hir::GenericArgs>, id: HirId, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        match res {
            Res::Local(binding) => self.local_ty(binding).unwrap_or(tcx.types.error),
            Res::Def(DefKind::Fn | DefKind::AssocFn, def) => {
                let args = self.args_for(def, explicit, span);
                self.write_args(id, args);
                self.register_predicates(def, args, span);
                tcx.mk_fn_def(def, args)
            }
            Res::Def(kind @ DefKind::Ctor(..), ctor) => {
                let fresh = self.infcx.fresh_args_for_item(ctor);
                // Explicit arguments are the type's: `Pair<int64>(1, 2)`.
                let args = match explicit {
                    Some(explicit) => {
                        let parent = tcx.parent(ctor);
                        let adt = if tcx.def_kind(parent) == DefKind::Variant { tcx.parent(parent) } else { parent };
                        let own = self.lower_own_args(adt, explicit, span);
                        if own.len() == fresh.len() { own } else { fresh }
                    }
                    None => fresh,
                };
                self.write_args(id, args);
                self.item_value_ty(kind, ctor, args, span)
            }
            Res::Def(DefKind::Const | DefKind::Static { .. } | DefKind::ConstParam, def) => {
                self.instantiate_type_of(def, tcx.identity_args(def), span)
            }
            Res::Def(DefKind::AssocConst, def) => {
                let args = self.infcx.fresh_args_for_item(def);
                self.write_args(id, args);
                self.register_predicates(def, args, span);
                self.instantiate_type_of(def, args, span)
            }
            Res::SelfCtor(imp) => {
                let self_ty = tcx.erase_regions(tcx.type_of(imp));
                match *self_ty.kind() {
                    TyKind::Adt(adt, args) if !adt.is_enum() => match adt.non_enum_variant().ctor {
                        Some((CtorKind::Fn, ctor)) => tcx.mk_fn_def(ctor, args),
                        Some((CtorKind::Const, _)) => self_ty,
                        None => {
                            self.err(span, "the `Self` constructor can only be used with tuple or unit structs").emit();
                            tcx.types.error
                        }
                    },
                    _ => tcx.types.error,
                }
            }
            Res::Err => tcx.types.error,
            other => {
                self.err(span, format!("expected value, found {}", other.descr())).with_code(ErrCode(423)).emit();
                tcx.types.error
            }
        }
    }

    /// `Type.Name` in an expression: a variant, or an associated function
    /// or constant of the type's impls or of a trait it implements.
    pub(super) fn type_relative_value(&self, self_ty: Ty<'tcx>, seg: &hir::PathSegment, id: HirId, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        let name = seg.ident;
        let resolved = self.structurally_resolve(self_ty, span);
        if resolved == tcx.types.error {
            return resolved;
        }
        // An enum's variant, as through an alias or `Self`.
        if let TyKind::Adt(adt, args) = *resolved.kind() {
            if adt.is_enum() {
                if let Some(v) = adt.variants().iter().find(|v| v.name == name.name) {
                    return match v.ctor {
                        Some((kind, ctor)) => {
                            self.write_resolution(id, DefKind::Ctor(vsc_hir::def::CtorOf::Variant, kind), ctor);
                            self.write_args(id, args);
                            match kind {
                                CtorKind::Fn => tcx.mk_fn_def(ctor, args),
                                CtorKind::Const => resolved,
                            }
                        }
                        None => {
                            self.err(span, format!("expected value, found struct variant `{}.{}`", tcx.def_path_str(adt.did()), name))
                                .with_code(ErrCode(533))
                                .with_help("use struct literal syntax instead")
                                .emit();
                            tcx.types.error
                        }
                    };
                }
            }
        }
        match self.lookup_assoc_item(resolved, name, seg.args.as_deref(), span) {
            Some((kind, item, args)) => {
                self.write_resolution(id, kind, item);
                self.write_args(id, args);
                self.item_value_ty(kind, item, args, span)
            }
            None => {
                self.err(name.span, format!("no function or associated item named `{name}` found for `{}` in the current scope", self.ty_str(resolved)))
                    .with_code(ErrCode(599))
                    .with_span_label(name.span, format!("function or associated item not found in `{}`", self.ty_str(resolved)))
                    .emit();
                tcx.types.error
            }
        }
    }

    /// A path to a lang item in an expression, from a desugaring.
    pub(super) fn lang_item_value(&self, item: LangItem, id: HirId, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        let Some(def) = tcx.require_lang_item(item, span) else { return tcx.types.error };
        match tcx.def_kind(def) {
            DefKind::Fn => {
                let args = self.infcx.fresh_args_for_item(def);
                self.write_args(id, args);
                self.item_value_ty(DefKind::Fn, def, args, span)
            }
            DefKind::AssocFn => {
                let args = self.infcx.fresh_args_for_item(def);
                self.write_args(id, args);
                let container = tcx.parent(def);
                // A trait method's `Self: Trait` is among its predicates,
                // registered below.
                if tcx.def_kind(container) != DefKind::Trait {
                    // An inherent function: its impl's type must match.
                    let _ = self.instantiate_type_of(container, args, span);
                }
                self.item_value_ty(DefKind::AssocFn, def, args, span)
            }
            DefKind::Variant => {
                let adt = tcx.adt_def(tcx.parent(def));
                let args = self.infcx.fresh_args_for_item(adt.did());
                let v = adt.variants().iter().find(|v| v.def_id == def).unwrap();
                match v.ctor {
                    Some((CtorKind::Fn, ctor)) => tcx.mk_fn_def(ctor, args),
                    _ => tcx.mk_adt(adt, args),
                }
            }
            DefKind::Struct => {
                let args = self.infcx.fresh_args_for_item(def);
                tcx.mk_adt(tcx.adt_def(def), args)
            }
            _ => tcx.types.error,
        }
    }

    // -----------------------------------------------------------------------
    // Calls
    // -----------------------------------------------------------------------

    pub(super) fn check_call(&self, e: &hir::Expr, callee: &hir::Expr, args: &[hir::Expr]) -> Ty<'tcx> {
        let tcx = self.tcx;
        let callee_ty = self.check_expr(callee);
        let mut t = self.structurally_resolve(callee_ty, callee.span);
        // Through references: calling `&F`.
        let mut derefs = Vec::new();
        while let TyKind::Ref(_, inner, _) = *t.kind() {
            t = self.structurally_resolve(inner, callee.span);
            derefs.push(t);
        }
        let sig = match *t.kind() {
            TyKind::FnDef(def, fargs) => self.instantiate_sig(def, fargs, e.span),
            TyKind::FnPtr(sig) => sig.value,
            TyKind::Closure(_, cargs) => closure_sig(cargs),
            TyKind::Adt(d, fargs) if tcx.is_lang_item(d.did(), LangItem::Func) && fargs.len() == 1 => {
                match *self.infcx.shallow_resolve(fargs[0].expect_ty()).kind() {
                    TyKind::FnPtr(sig) => sig.value,
                    _ => return self.not_callable(callee, t, args),
                }
            }
            TyKind::Error => {
                for a in args {
                    self.check_expr(a);
                }
                return tcx.types.error;
            }
            _ => return self.check_overloaded_call(e, callee, t, derefs, args),
        };
        if sig.inputs().len() == args.len() + 1 && self.report_unit_struct_method(callee, t) {
            for (a, &ty) in args.iter().zip(&sig.inputs()[1..]) {
                self.check_expr_coercible_to_type(a, ty);
            }
            return sig.output();
        }
        self.check_args(e.span, &sig, sig.inputs(), args, "function");
        sig.output()
    }

    /// `Marker.hello()` where `Marker` is a unit struct and `hello` takes
    /// `self`: the path names the method on the type (exception 7, as in Go
    /// and Swift), so the receiver is missing. Parentheses make `Marker` the
    /// value.
    pub(super) fn report_unit_struct_method(&self, callee: &hir::Expr, t: Ty<'tcx>) -> bool {
        let tcx = self.tcx;
        let hir::ExprKind::Path(hir::QPath::TypeRelative(base, seg)) = &callee.kind else { return false };
        let TyKind::FnDef(def, _) = *t.kind() else { return false };
        let Some(parent) = tcx.opt_parent(def) else { return false };
        let is_method = tcx.associated_items(parent).iter().any(|i| i.def_id == def && matches!(i.kind, AssocKind::Fn { has_self: true }));
        let unit_struct = match *self.lower_ty(base).kind() {
            TyKind::Adt(adt, _) => !adt.is_enum() && matches!(adt.non_enum_variant().ctor, Some((CtorKind::Const, _))),
            _ => false,
        };
        if is_method && unit_struct {
            self.err(callee.span, format!("`{}` is a method, so `{}` needs a value to call it on", seg.ident, tcx.ty_to_string(self.lower_ty(base))))
                .with_code(ErrCode(61))
                .with_help(format!("`{}` names the method on the type; to call it on the unit value, write `({}).{}(…)`", callee_text(base, seg), ty_text(base), seg.ident))
                .emit();
        }
        is_method && unit_struct
    }

    /// `f(x)` where `f` isn't a function but implements one of the function
    /// traits, as a parameter `F: Fn(int32) -> int32` does: a call of
    /// `Fn.call(&f, (x,))`, `FnMut.callMut(&mut f, (x,))` or
    /// `FnOnce.callOnce(f, (x,))`, trying them in that order (rustc's
    /// `try_overloaded_call_traits`).
    pub(super) fn check_overloaded_call(&self, e: &hir::Expr, callee: &hir::Expr, t: Ty<'tcx>, derefs: Vec<Ty<'tcx>>, args: &[hir::Expr]) -> Ty<'tcx> {
        let tcx = self.tcx;
        let li = tcx.lang_items();
        let arg_tys: Vec<Ty<'tcx>> = args.iter().map(|_| self.infcx.next_ty_var()).collect();
        let tuple = tcx.mk_tup(&arg_tys);
        let fn_once = li.get(LangItem::FnOnce);
        let output_item = fn_once.and_then(|d| tcx.associated_items(d).iter().find(|i| i.kind == AssocKind::Type).map(|i| i.def_id));
        for (item, name, borrow) in [
            (LangItem::Fn, "call", Some(Mutability::Not)),
            (LangItem::FnMut, "callMut", Some(Mutability::Mut)),
            (LangItem::FnOnce, "callOnce", None),
        ] {
            let Some(trait_def) = li.get(item) else { continue };
            let call_args = tcx.mk_args(&[GenericArg::Type(t), GenericArg::Type(tuple)]);
            let tr = TraitRef { def_id: trait_def, args: call_args };
            let selcx = self.selcx();
            if !self.infcx.probe(|| matches!(selcx.select(tr, e.span), vsc_trait_selection::Selection::Selected(..))) {
                continue;
            }
            let Some(method) = tcx.associated_items(trait_def).iter().find(|i| matches!(i.kind, AssocKind::Fn { .. }) && i.name.as_str() == name).map(|i| i.def_id)
            else {
                continue;
            };
            let (Some(fn_once), Some(output_item)) = (fn_once, output_item) else { continue };
            self.register_trait(tr, e.span);
            self.select_where_possible();
            let mut adj: Vec<Adjustment<'tcx>> = derefs.iter().map(|&target| Adjustment { kind: Adjust::Deref, target }).collect();
            if let Some(m) = borrow {
                adj.push(Adjustment { kind: Adjust::Borrow(m), target: tcx.mk_ref(tcx.regions.re_erased, t, m) });
            }
            self.write_adjustments(callee.hir_id, adj);
            self.write_resolution(e.hir_id, DefKind::AssocFn, method);
            self.write_args(e.hir_id, call_args);
            for (a, &ty) in args.iter().zip(&arg_tys) {
                self.check_expr_coercible_to_type(a, ty);
            }
            let _ = fn_once;
            let output = tcx.mk_ty(TyKind::Alias(AliasTy { def_id: output_item, args: call_args }));
            return self.normalize(output, e.span);
        }
        // A bound `F: Fn(A)` with a different number of arguments.
        let bound_arity = self.param_env_clauses().iter().find_map(|c| match c {
            Clause::Trait(tr) if tr.self_ty() == t && vsc_trait_selection::fn_trait_kind(tcx, tr.def_id).is_some() && tr.args.len() == 2 => {
                match tr.args[1].expect_ty().kind() {
                    TyKind::Tuple(tys) => Some(tys.len()),
                    _ => None,
                }
            }
            _ => None,
        });
        if let Some(n) = bound_arity {
            if n != args.len() {
                for a in args {
                    self.check_expr(a);
                }
                let plural = |k: usize| format!("{k} argument{}", if k == 1 { "" } else { "s" });
                self.err(e.span, format!(
                    "this function takes {} but {} {} supplied",
                    plural(n),
                    plural(args.len()),
                    if args.len() == 1 { "was" } else { "were" }
                ))
                .with_code(ErrCode(57))
                .emit();
                return tcx.types.error;
            }
        }
        self.not_callable(callee, t, args)
    }

    pub(super) fn not_callable(&self, callee: &hir::Expr, t: Ty<'tcx>, args: &[hir::Expr]) -> Ty<'tcx> {
        for a in args {
            self.check_expr(a);
        }
        self.err(callee.span, format!("expected function, found `{}`", self.ty_str(t)))
            .with_code(ErrCode(618))
            .with_span_label(callee.span, "call expression requires function")
            .emit();
        self.tcx.types.error
    }

    /// Checks the arguments of a call against the parameter types.
    pub fn check_args(&self, span: Span, sig: &FnSig<'tcx>, inputs: &[Ty<'tcx>], args: &[hir::Expr], what: &str) {
        let n = inputs.len();
        let count_ok = if sig.c_variadic { args.len() >= n } else { args.len() == n };
        if !count_ok {
            let plural = |k: usize, w: &str| format!("{k} {w}{}", if k == 1 { "" } else { "s" });
            self.err(span, format!(
                "this {what} takes {}{} but {} {} supplied",
                if sig.c_variadic { "at least " } else { "" },
                plural(n, "argument"),
                plural(args.len(), "argument"),
                if args.len() == 1 { "was" } else { "were" }
            ))
            .with_code(ErrCode(if sig.c_variadic { 60 } else { 61 }))
            .emit();
        }
        for (i, a) in args.iter().enumerate() {
            match inputs.get(i) {
                Some(&t) => {
                    self.check_expr_coercible_to_type(a, t);
                }
                None => {
                    self.check_expr(a);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Operators
    // -----------------------------------------------------------------------

}
