//! Binary/unary operators, casts, place expressions, fields, indexing, and struct literals.

use vsc_ast::token::LitKind;
use vsc_ast::{BinOpKind, UnOp};
use vsc_errors::ErrCode;
use vsc_hir::def::{DefKind, Res};
use vsc_hir::{self as hir, LangItem};
use vsc_middle::ty::*;
use vsc_span::{Ident, Span};

use super::is_place;
use crate::{Expectation, FnCtxt};

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    pub(super) fn check_binop(&self, e: &hir::Expr, op: BinOpKind, a: &hir::Expr, b: &hir::Expr, is_assign: bool) -> Ty<'tcx> {
        let tcx = self.tcx;
        if matches!(op, BinOpKind::And | BinOpKind::Or) {
            self.check_expr_coercible_to_type(a, tcx.types.bool);
            self.check_expr_coercible_to_type(b, tcx.types.bool);
            return tcx.types.bool;
        }
        let lt = self.check_expr(a);
        let rt = if matches!(op, BinOpKind::Shl | BinOpKind::Shr) {
            self.check_expr(b)
        } else {
            self.check_expr_with_expectation(b, Expectation::HasType(lt))
        };
        let mut l = self.infcx.shallow_resolve(lt);
        let mut r = self.infcx.shallow_resolve(rt);
        // A side whose type waits on a bound (a `?`'s value, say): prove
        // what can be proven first.
        if matches!(l.kind(), TyKind::Infer(InferTy::TyVar(_))) || matches!(r.kind(), TyKind::Infer(InferTy::TyVar(_))) {
            self.select_where_possible();
            l = self.infcx.shallow_resolve(lt);
            r = self.infcx.shallow_resolve(rt);
            // Still unknown: the operands of a primitive operator have the
            // same type (except shifts).
            if matches!(l.kind(), TyKind::Infer(InferTy::TyVar(_))) && !matches!(op, BinOpKind::Shl | BinOpKind::Shr) {
                let _ = self.infcx.commit_if_ok(|| self.infcx.unify(l, r));
                l = self.infcx.shallow_resolve(lt);
            }
        }
        if self.references_error(l) || self.references_error(r) {
            return if op.is_comparison() { tcx.types.bool } else { tcx.types.error };
        }
        let is_int = |t: Ty<'tcx>| matches!(t.kind(), TyKind::Int(_) | TyKind::Uint(_) | TyKind::Infer(InferTy::IntVar(_)));
        let is_num = |t: Ty<'tcx>| is_int(t) || matches!(t.kind(), TyKind::Float(_) | TyKind::Infer(InferTy::FloatVar(_)));
        let is_scalar = |t: Ty<'tcx>| is_num(t) || matches!(t.kind(), TyKind::Bool | TyKind::Char | TyKind::RawPtr(..) | TyKind::FnPtr(_));
        let ok = match op {
            BinOpKind::Add | BinOpKind::Sub | BinOpKind::Mul | BinOpKind::Div | BinOpKind::Rem => {
                is_num(l) && self.infcx.commit_if_ok(|| self.infcx.unify(l, r)).is_ok()
            }
            BinOpKind::BitAnd | BinOpKind::BitOr | BinOpKind::BitXor => {
                (is_int(l) || matches!(l.kind(), TyKind::Bool)) && self.infcx.commit_if_ok(|| self.infcx.unify(l, r)).is_ok()
            }
            BinOpKind::Shl | BinOpKind::Shr => is_int(l) && is_int(r),
            _ => {
                // Comparisons of unknown types wait for the other side.
                let unify_ok = self.infcx.commit_if_ok(|| self.infcx.unify(l, r)).is_ok();
                let l = self.infcx.shallow_resolve(l);
                unify_ok && (is_scalar(l) || matches!(l.kind(), TyKind::Infer(InferTy::TyVar(_)) | TyKind::Never))
            }
        };
        if !ok {
            if let Some(t) = self.check_overloaded_binop(e, op, a, b, self.infcx.shallow_resolve(l), self.infcx.shallow_resolve(r), is_assign) {
                return t;
            }
            let (ls, rs) = (self.ty_str(l), self.ty_str(r));
            let msg = match op {
                BinOpKind::Add => format!("cannot add `{rs}` to `{ls}`"),
                BinOpKind::Sub => format!("cannot subtract `{rs}` from `{ls}`"),
                BinOpKind::Mul => format!("cannot multiply `{ls}` by `{rs}`"),
                BinOpKind::Div => format!("cannot divide `{ls}` by `{rs}`"),
                BinOpKind::Rem => format!("cannot calculate the remainder of `{ls}` divided by `{rs}`"),
                _ if is_assign => format!("binary assignment operation `{}=` cannot be applied to type `{ls}`", op.as_str()),
                _ => format!("binary operation `{}` cannot be applied to type `{ls}`", op.as_str()),
            };
            let mut err = self.err(e.span, msg).with_code(ErrCode(if is_assign { 368 } else { 369 }));
            if !(is_scalar(l) && is_scalar(r)) {
                let item = match (op, is_assign) {
                    (BinOpKind::Eq | BinOpKind::Ne, _) => "PartialEq",
                    (BinOpKind::Lt | BinOpKind::Le | BinOpKind::Gt | BinOpKind::Ge, _) => "PartialOrd",
                    (BinOpKind::Add, false) => "Add",
                    (BinOpKind::Sub, false) => "Sub",
                    (BinOpKind::Mul, false) => "Mul",
                    (BinOpKind::Div, false) => "Div",
                    (BinOpKind::Rem, false) => "Rem",
                    (BinOpKind::BitAnd, false) => "BitAnd",
                    (BinOpKind::BitOr, false) => "BitOr",
                    (BinOpKind::BitXor, false) => "BitXor",
                    (BinOpKind::Shl, false) => "Shl",
                    (BinOpKind::Shr, false) => "Shr",
                    (BinOpKind::Add, true) => "AddAssign",
                    (BinOpKind::Sub, true) => "SubAssign",
                    (BinOpKind::Mul, true) => "MulAssign",
                    (BinOpKind::Div, true) => "DivAssign",
                    (BinOpKind::Rem, true) => "RemAssign",
                    (BinOpKind::BitAnd, true) => "BitAndAssign",
                    (BinOpKind::BitOr, true) => "BitOrAssign",
                    (BinOpKind::BitXor, true) => "BitXorAssign",
                    (BinOpKind::Shl, true) => "ShlAssign",
                    (BinOpKind::Shr, true) => "ShrAssign",
                    _ => "the operator's trait",
                };
                if let TyKind::Param(_) = self.infcx.shallow_resolve(l).kind() {
                    let bound = if op.is_comparison() || is_assign { format!("{ls}: {item}") } else { format!("{ls}: {item}<Output = {ls}>") };
                    err = err.with_help(format!("consider restricting type parameter `{ls}` with trait `{item}`: `{bound}`"));
                } else {
                    err = err.with_note(format!("an implementation of `{item}` might be missing for `{ls}`"));
                }
            }
            err.emit();
            return if op.is_comparison() { tcx.types.bool } else { tcx.types.error };
        }
        if op.is_comparison() { tcx.types.bool } else { l }
    }

    pub(super) fn check_unop(&self, e: &hir::Expr, op: UnOp, x: &hir::Expr, expected: Expectation<'tcx>) -> Ty<'tcx> {
        let tcx = self.tcx;
        let inner_expected = match op {
            UnOp::Not | UnOp::Neg => expected,
            UnOp::Deref => Expectation::None,
        };
        let t = self.check_expr_with_expectation(x, inner_expected);
        // `-128` is one literal for the range check.
        if op == UnOp::Neg && matches!(&x.kind, hir::ExprKind::Lit(l) if l.kind == LitKind::Integer) {
            if let Some(last) = self.int_literals.borrow_mut().last_mut() {
                last.2 = true;
            }
        }
        let t = match op {
            UnOp::Deref => self.structurally_resolve(t, x.span),
            _ => self.infcx.shallow_resolve(t),
        };
        if t == tcx.types.error {
            return t;
        }
        match op {
            UnOp::Deref => match *t.kind() {
                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => inner,
                _ => {
                    self.err(e.span, format!("type `{}` cannot be dereferenced", self.ty_str(t))).with_code(ErrCode(614)).emit();
                    tcx.types.error
                }
            },
            UnOp::Not => match t.kind() {
                TyKind::Bool | TyKind::Int(_) | TyKind::Uint(_) | TyKind::Infer(InferTy::IntVar(_)) => t,
                _ if let Some(out) = self.check_overloaded_unop(e, op, t) => out,
                _ => {
                    self.err(e.span, format!("cannot apply unary operator `!` to type `{}`", self.ty_str(t))).with_code(ErrCode(600)).emit();
                    tcx.types.error
                }
            },
            UnOp::Neg => match t.kind() {
                TyKind::Int(_) | TyKind::Float(_) | TyKind::Infer(InferTy::IntVar(_) | InferTy::FloatVar(_)) => t,
                TyKind::Uint(_) => {
                    self.err(e.span, format!("cannot apply unary operator `-` to type `{}`", self.ty_str(t)))
                        .with_code(ErrCode(600))
                        .with_note("unsigned values cannot be negated")
                        .emit();
                    tcx.types.error
                }
                _ if let Some(out) = self.check_overloaded_unop(e, op, t) => out,
                _ => {
                    self.err(e.span, format!("cannot apply unary operator `-` to type `{}`", self.ty_str(t)))
                        .with_code(ErrCode(600))
                        .with_note(format!("an implementation of `Neg` might be missing for `{}`", self.ty_str(t)))
                        .emit();
                    tcx.types.error
                }
            },
        }
    }

    /// `x as T`, once both types are known.
    pub fn check_cast(&self, span: Span, from: Ty<'tcx>, to: Ty<'tcx>) {
        let infcx = self.infcx;
        let from = infcx.resolve_vars_if_possible(from);
        let to = infcx.resolve_vars_if_possible(to);
        if self.references_error(from) || self.references_error(to) {
            return;
        }
        if infcx.probe(|| self.try_coerce(None, from, to).is_ok()) {
            let _ = self.try_coerce(None, from, to);
            return;
        }
        use TyKind::*;
        let numeric = |t: Ty<'tcx>| matches!(t.kind(), Int(_) | Uint(_) | Float(_));
        let int = |t: Ty<'tcx>| matches!(t.kind(), Int(_) | Uint(_));
        let ptr = |t: Ty<'tcx>| matches!(t.kind(), RawPtr(..));
        let c_like_enum = |t: Ty<'tcx>| matches!(*t.kind(), Adt(d, _) if d.is_enum() && d.variants().iter().all(|v| v.fields.is_empty()));
        let ok = match (*from.kind(), *to.kind()) {
            _ if numeric(from) && numeric(to) => true,
            (_, Bool) if numeric(from) => {
                self.err(span, format!("cannot cast `{}` as `bool`", self.ty_str(from)))
                    .with_code(ErrCode(54))
                    .with_help("compare with zero instead: `x != 0`")
                    .emit();
                return;
            }
            (Bool | Char, _) if int(to) => true,
            (Uint(UintTy::U8), Char) => true,
            (_, Char) if int(from) => {
                self.err(span, format!("only `uint8` can be cast as `char`, not `{}`", self.ty_str(from))).with_code(ErrCode(604)).emit();
                return;
            }
            _ if ptr(from) && (ptr(to) || int(to)) => true,
            _ if int(from) && ptr(to) => true,
            (FnDef(..) | FnPtr(_), _) if ptr(to) || int(to) => true,
            (Ref(_, inner, _), RawPtr(..)) if matches!(inner.kind(), Array(..)) => true,
            _ if c_like_enum(from) && int(to) => true,
            _ => false,
        };
        if !ok {
            let primitive = |t: Ty<'tcx>| numeric(t) || ptr(t) || matches!(t.kind(), Bool | Char);
            if !primitive(from) || !primitive(to) {
                self.err(span, format!("non-primitive cast: `{}` as `{}`", self.ty_str(from), self.ty_str(to)))
                    .with_code(ErrCode(605))
                    .with_span_label(span, "an `as` expression can only be used to convert between primitive types")
                    .emit();
            } else {
                self.err(span, format!("casting `{}` as `{}` is invalid", self.ty_str(from), self.ty_str(to))).with_code(ErrCode(606)).emit();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Control flow
    // -----------------------------------------------------------------------


    /// The left side of an assignment: must be a place.
    pub(super) fn check_place(&self, e: &hir::Expr) -> Ty<'tcx> {
        let t = self.check_expr(e);
        if !is_place(e) {
            self.err(e.span, "invalid left-hand side of assignment")
                .with_code(ErrCode(70))
                .with_span_label(e.span, "cannot assign to this expression")
                .emit();
        }
        t
    }

    /// The type's next step for autoderef: through references, and through
    /// a class value to the class (exception 3).
    pub fn autoderef_step(&self, t: Ty<'tcx>) -> Option<Ty<'tcx>> {
        match *self.infcx.shallow_resolve(t).kind() {
            TyKind::Ref(_, inner, _) => Some(inner),
            TyKind::Adt(d, args) if self.tcx.is_lang_item(d.did(), LangItem::Class) && args.len() == 1 => Some(args[0].expect_ty()),
            _ => None,
        }
    }

    pub(super) fn check_field(&self, e: &hir::Expr, base: &hir::Expr, ident: Ident) -> Ty<'tcx> {
        let tcx = self.tcx;
        let bt = self.check_expr(base);
        let mut t = self.structurally_resolve(bt, base.span);
        let mut adj = Vec::new();
        for _ in 0..16 {
            match *t.kind() {
                TyKind::Adt(adt, args) if !adt.is_enum() && !tcx.is_lang_item(adt.did(), LangItem::Class) => {
                    if let Some((i, f)) = adt.non_enum_variant().fields.iter().enumerate().find(|(_, f)| f.name == ident.name) {
                        self.write_field_index(e.hir_id, i);
                        self.write_adjustments(base.hir_id, adj);
                        return self.instantiate_type_of(f.did, args, e.span);
                    }
                    break;
                }
                TyKind::Tuple(tys) => {
                    if let Ok(i) = ident.as_str().parse::<usize>() {
                        if let Some(&ft) = tys.get(i) {
                            self.write_field_index(e.hir_id, i);
                            self.write_adjustments(base.hir_id, adj);
                            return ft;
                        }
                    }
                    break;
                }
                TyKind::Error => return t,
                _ => {}
            }
            match self.autoderef_step(t) {
                Some(next) => {
                    adj.push(Adjustment { kind: Adjust::Deref, target: next });
                    t = self.structurally_resolve(next, base.span);
                }
                None => break,
            }
        }
        let shown = self.ty_str(bt);
        if self.has_method(bt, ident) {
            self.err(ident.span, format!("attempted to take value of method `{ident}` on type `{shown}`"))
                .with_code(ErrCode(615))
                .with_help(format!("use parentheses to call the method: `{ident}()`"))
                .emit();
        } else if matches!(t.kind(), TyKind::Int(_) | TyKind::Uint(_) | TyKind::Float(_) | TyKind::Bool | TyKind::Char | TyKind::Infer(_)) {
            self.err(ident.span, format!("`{shown}` is a primitive type and therefore doesn't have fields")).with_code(ErrCode(610)).emit();
        } else {
            self.err(ident.span, format!("no field `{ident}` on type `{shown}`"))
                .with_code(ErrCode(609))
                .with_span_label(ident.span, "unknown field")
                .emit();
        }
        tcx.types.error
    }

    pub(super) fn check_index(&self, e: &hir::Expr, base: &hir::Expr, idx: &hir::Expr) -> Ty<'tcx> {
        let tcx = self.tcx;
        let bt = self.check_expr(base);
        let it = self.check_expr(idx);
        let mut t = self.structurally_resolve(bt, base.span);
        let mut adj = Vec::new();
        loop {
            let elem = match *t.kind() {
                TyKind::Array(elem, _) | TyKind::Slice(elem) => Some((elem, tcx.mk_ty(TyKind::Slice(elem)))),
                TyKind::Str => Some((tcx.types.error, tcx.types.str_)),
                TyKind::Error => return t,
                _ => None,
            };
            if let Some((elem, sliced)) = elem {
                let i = self.infcx.shallow_resolve(it);
                self.write_adjustments(base.hir_id, adj);
                // A range of indices gives a slice.
                if let TyKind::Adt(d, _) = *i.kind() {
                    let li = tcx.lang_items();
                    let is_range = [LangItem::Range, LangItem::RangeFrom, LangItem::RangeTo, LangItem::RangeFull, LangItem::RangeToInclusive]
                        .iter()
                        .any(|&l| li.get(l) == Some(d.did()));
                    if is_range {
                        return sliced;
                    }
                }
                if t.kind() == &TyKind::Str {
                    self.err(e.span, "a `string` can only be indexed by a range").with_code(ErrCode(277)).emit();
                    return tcx.types.error;
                }
                self.demand_coerce(idx, it, tcx.types.usize);
                return elem;
            }
            match self.autoderef_step(t) {
                Some(next) => {
                    adj.push(Adjustment { kind: Adjust::Deref, target: next });
                    t = self.structurally_resolve(next, base.span);
                }
                None => break,
            }
        }
        if let Some(elem) = self.check_overloaded_index(e, base, bt, it) {
            return elem;
        }
        self.err(e.span, format!("cannot index into a value of type `{}`", self.ty_str(bt))).with_code(ErrCode(608)).emit();
        tcx.types.error
    }

    // -----------------------------------------------------------------------
    // Struct literals
    // -----------------------------------------------------------------------

    /// The type a struct literal or pattern builds, and which variant.
    /// For a class, the type is `Class<C>` and the variant is `C`'s.
    pub fn resolve_struct_path(&self, q: &hir::QPath, span: Span) -> Option<(Ty<'tcx>, AdtDef<'tcx>, GenericArgsRef<'tcx>, usize)> {
        let tcx = self.tcx;
        if let hir::QPath::Resolved(None, path) = q {
            if let Res::Def(DefKind::Variant, v) = path.res {
                let adt = tcx.adt_def(tcx.parent(v));
                let args = self.infcx.fresh_args_for_item(adt.did());
                let index = adt.variants().iter().position(|x| x.def_id == v)?;
                return Some((tcx.mk_adt(adt, args), adt, args, index));
            }
        }
        if let hir::QPath::TypeRelative(base, seg) = q {
            let bt = self.lower_ty(base);
            if let TyKind::Adt(adt, args) = *self.infcx.shallow_resolve(bt).kind() {
                if let Some(index) = adt.variants().iter().position(|v| v.name == seg.ident.name) {
                    if adt.is_enum() {
                        return Some((bt, adt, args, index));
                    }
                }
            }
        }
        let ty = self.lower_qpath_ty(q, span);
        let t = self.infcx.shallow_resolve(ty);
        match *t.kind() {
            TyKind::Adt(d, args) if tcx.is_lang_item(d.did(), LangItem::Class) && args.len() == 1 => {
                match *self.infcx.shallow_resolve(args[0].expect_ty()).kind() {
                    TyKind::Adt(inner, iargs) => Some((t, inner, iargs, 0)),
                    _ => None,
                }
            }
            TyKind::Adt(adt, args) if !adt.is_enum() => Some((t, adt, args, 0)),
            TyKind::Error => None,
            _ => {
                self.err(span, format!("expected struct, variant or union type, found `{}`", self.ty_str(t))).with_code(ErrCode(574)).emit();
                None
            }
        }
    }

    pub(super) fn check_struct_expr(&self, e: &hir::Expr, q: &hir::QPath, fields: &[hir::ExprField], tail: &hir::StructTailExpr) -> Ty<'tcx> {
        let tcx = self.tcx;
        let Some((ty, adt, args, vi)) = self.resolve_struct_path(q, e.span) else {
            for f in fields {
                self.check_expr(&f.expr);
            }
            if let hir::StructTailExpr::Base(b) = tail {
                self.check_expr(b);
            }
            return tcx.types.error;
        };
        let variant = &adt.variants()[vi];
        let name = if adt.is_enum() { format!("variant `{}`", tcx.def_path_str(variant.def_id)) } else { format!("{} `{}`", adt.descr(), tcx.def_path_str(adt.did())) };
        let mut seen = vec![false; variant.fields.len()];
        for f in fields {
            match variant.fields.iter().position(|vf| vf.name == f.ident.name) {
                Some(i) => {
                    if seen[i] {
                        self.err(f.ident.span, format!("field `{}` specified more than once", f.ident)).with_code(ErrCode(62)).emit();
                    }
                    seen[i] = true;
                    self.write_field_index(f.hir_id, i);
                    let ft = self.instantiate_type_of(variant.fields[i].did, args, f.span);
                    self.check_expr_coercible_to_type(&f.expr, ft);
                }
                None => {
                    self.check_expr(&f.expr);
                    self.err(f.ident.span, format!("{name} has no field named `{}`", f.ident))
                        .with_code(ErrCode(if adt.is_enum() { 559 } else { 560 }))
                        .with_span_label(f.ident.span, "unknown field")
                        .emit();
                }
            }
        }
        if adt.kind() == AdtKind::Union {
            if fields.len() != 1 {
                self.err(e.span, "union expressions should have exactly one field").with_code(ErrCode(784)).emit();
            }
        } else {
            match tail {
                hir::StructTailExpr::Base(b) => {
                    self.check_expr_coercible_to_type(b, ty);
                }
                hir::StructTailExpr::None => {
                    let missing: Vec<String> =
                        variant.fields.iter().zip(&seen).filter(|(_, s)| !**s).map(|(f, _)| format!("`{}`", f.name)).collect();
                    if !missing.is_empty() {
                        let what = if missing.len() == 1 { "field" } else { "fields" };
                        self.err(e.span, format!("missing {what} {} in initializer of `{}`", missing.join(", "), tcx.def_path_str(variant.def_id)))
                            .with_code(ErrCode(63))
                            .with_span_label(e.span, format!("missing {}", missing.join(", ")))
                            .emit();
                    }
                }
            }
        }
        ty
    }

    /// Does the type have a method with this name (for diagnostics)?
    pub(super) fn has_method(&self, ty: Ty<'tcx>, name: Ident) -> bool {
        self.infcx.probe(|| self.probe_method(ty, name, None).is_some())
    }
}
