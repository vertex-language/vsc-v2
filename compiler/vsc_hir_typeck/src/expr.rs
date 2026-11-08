//! Checking expressions (rustc's `expr.rs`, `callee.rs`, `op.rs`,
//! `cast.rs`, `_match.rs` and `closure.rs`).

use vsc_ast::token::LitKind;
use vsc_ast::{BinOpKind, BorrowKind, UnOp};
use vsc_errors::ErrCode;
use vsc_hir::def::{CtorKind, DefKind, Res};
use vsc_hir::{self as hir, HirId, LangItem};
use vsc_middle::ty::*;
use vsc_span::def_id::DefId;
use vsc_span::{Ident, Span};

use crate::coercion::closure_sig;
use crate::{Expectation, FnCtxt};

/// Collects the types of branches into one: `if`/`else`, `match` arms,
/// array elements, `break` values (rustc's `CoerceMany`).
struct CoerceMany<'tcx> {
    target: Option<Ty<'tcx>>,
}

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    pub fn check_expr(&self, e: &hir::Expr) -> Ty<'tcx> {
        self.check_expr_with_expectation(e, Expectation::None)
    }

    pub fn check_expr_coercible_to_type(&self, e: &hir::Expr, ty: Ty<'tcx>) -> Ty<'tcx> {
        let t = self.check_expr_with_expectation(e, Expectation::HasType(ty));
        self.demand_coerce(e, t, ty)
    }

    pub fn check_expr_with_expectation(&self, e: &hir::Expr, expected: Expectation<'tcx>) -> Ty<'tcx> {
        let old_diverges = self.diverges.replace(false);
        let ty = self.check_expr_kind(e, expected);
        if matches!(self.infcx.shallow_resolve(ty).kind(), TyKind::Never) {
            self.diverges.set(true);
        }
        self.diverges.set(self.diverges.get() || old_diverges);
        self.write_ty(e.hir_id, e.span, ty);
        ty
    }

    fn check_expr_kind(&self, e: &hir::Expr, expected: Expectation<'tcx>) -> Ty<'tcx> {
        let tcx = self.tcx;
        match &e.kind {
            hir::ExprKind::Lit(lit) => self.check_lit(lit, expected, e.span),
            hir::ExprKind::Path(q) => self.check_expr_path(q, e.hir_id, e.span),
            hir::ExprKind::Call(callee, args) => self.check_call(e, callee, args),
            hir::ExprKind::MethodCall(seg, recv, args, span) => self.check_method_call(e, seg, recv, args, *span),
            hir::ExprKind::Tup(es) => {
                let expected_elems = match expected.only_has_type().map(|t| *self.infcx.shallow_resolve(t).kind()) {
                    Some(TyKind::Tuple(tys)) if tys.len() == es.len() => Some(tys),
                    _ => None,
                };
                let tys: Vec<_> = es
                    .iter()
                    .enumerate()
                    .map(|(i, x)| match expected_elems {
                        Some(tys) => self.check_expr_coercible_to_type(x, tys[i]),
                        None => self.check_expr(x),
                    })
                    .collect();
                tcx.mk_tup(&tys)
            }
            hir::ExprKind::Array(es) => {
                let elem_expected = match expected.only_has_type().map(|t| *self.infcx.shallow_resolve(t).kind()) {
                    Some(TyKind::Array(t, _) | TyKind::Slice(t)) => Some(t),
                    _ => None,
                };
                let mut many = CoerceMany { target: elem_expected };
                for x in es {
                    let t = self.check_expr_with_expectation(x, elem_expected.map_or(Expectation::None, Expectation::HasType));
                    self.coerce_many(&mut many, x, t, "array elements have incompatible types");
                }
                let elem = many.target.unwrap_or_else(|| self.infcx.next_ty_var());
                let len = tcx.mk_const(ConstKind::Value(tcx.types.usize, es.len() as u128));
                tcx.mk_ty(TyKind::Array(elem, len))
            }
            hir::ExprKind::Repeat(x, n) => {
                let elem_expected = match expected.only_has_type().map(|t| *self.infcx.shallow_resolve(t).kind()) {
                    Some(TyKind::Array(t, _) | TyKind::Slice(t)) => Expectation::HasType(t),
                    _ => Expectation::None,
                };
                let t = self.check_expr_with_expectation(x, elem_expected);
                let len = self.lower_array_len(n);
                tcx.mk_ty(TyKind::Array(t, len))
            }
            hir::ExprKind::Binary(op, a, b) => self.check_binop(e, op.node, a, b, false),
            hir::ExprKind::AssignOp(op, a, b) => {
                // The left side is checked once, by the operator; checking
                // it again would record its adjustments twice.
                self.check_binop(e, op.node, a, b, true);
                if !is_place(a) {
                    self.err(a.span, "invalid left-hand side of assignment")
                        .with_code(ErrCode(67))
                        .with_span_label(a.span, "cannot assign to this expression")
                        .emit();
                }
                tcx.types.unit
            }
            hir::ExprKind::Unary(op, x) => self.check_unop(e, *op, x, expected),
            hir::ExprKind::Cast(x, t) => {
                let to = self.lower_ty(t);
                let from = self.check_expr(x);
                self.deferred_casts.borrow_mut().push((e.span, from, to));
                to
            }
            hir::ExprKind::DropTemps(x) => self.check_expr_with_expectation(x, expected),
            hir::ExprKind::Let(l) => {
                let init = self.check_expr(&l.init);
                let ty = match &l.ty {
                    Some(t) => {
                        let t = self.lower_ty(t);
                        self.demand_coerce(&l.init, init, t)
                    }
                    None => init,
                };
                self.check_pat_top(&l.pat, ty);
                tcx.types.bool
            }
            hir::ExprKind::If(cond, then, els) => self.check_if(cond, then, els.as_deref(), expected),
            hir::ExprKind::Loop(body, _, source, _) => {
                let target = match source {
                    hir::LoopSource::Loop => Some(expected.only_has_type().unwrap_or_else(|| self.infcx.next_ty_var())),
                    _ => None,
                };
                let (_, may_break) = self.with_breakable(e.hir_id, target, || self.check_block(body, Expectation::HasType(tcx.types.unit)));
                match (source, may_break) {
                    (hir::LoopSource::Loop, true) => target.unwrap(),
                    (hir::LoopSource::Loop, false) => tcx.types.never,
                    _ => tcx.types.unit,
                }
            }
            hir::ExprKind::Match(scrutinee, arms, _) => self.check_match(scrutinee, arms, expected),
            hir::ExprKind::Closure(c) => self.check_closure(e, c, expected),
            hir::ExprKind::Block(b, label) => {
                if label.is_some() {
                    let target = expected.only_has_type().unwrap_or_else(|| self.infcx.next_ty_var());
                    let (ty, may_break) = self.with_breakable(e.hir_id, Some(target), || self.check_block(b, expected));
                    if may_break {
                        if self.try_coerce(None, ty, target).is_err() {
                            self.report_mismatch(b.span, target, ty);
                        }
                        target
                    } else {
                        ty
                    }
                } else {
                    self.check_block(b, expected)
                }
            }
            hir::ExprKind::Assign(lhs, rhs, _) => {
                let lt = self.check_place(lhs);
                self.check_expr_coercible_to_type(rhs, lt);
                tcx.types.unit
            }
            hir::ExprKind::Field(base, ident) => self.check_field(e, base, *ident),
            hir::ExprKind::Index(base, idx, _) => self.check_index(e, base, idx),
            hir::ExprKind::AddrOf(kind, m, x) => {
                let inner_expected = match expected.only_has_type().map(|t| *self.infcx.shallow_resolve(t).kind()) {
                    // An unsized target (`&[T]` from `&[T; N]`) is only a
                    // hint for array literals, as rustc's
                    // `ExpectRvalueLikeUnsized`; others get no expectation.
                    Some(TyKind::Ref(_, t, _) | TyKind::RawPtr(t, _))
                        if matches!(self.infcx.shallow_resolve(t).kind(), TyKind::Slice(_) | TyKind::Str | TyKind::Dynamic(..))
                            && !matches!(x.kind, hir::ExprKind::Array(_) | hir::ExprKind::Repeat(..)) =>
                    {
                        Expectation::None
                    }
                    Some(TyKind::Ref(_, t, _) | TyKind::RawPtr(t, _)) => Expectation::HasType(t),
                    _ => Expectation::None,
                };
                let t = self.check_expr_with_expectation(x, inner_expected);
                match kind {
                    BorrowKind::Ref => tcx.mk_ref(tcx.regions.re_erased, t, *m),
                    BorrowKind::Raw => tcx.mk_ty(TyKind::RawPtr(t, *m)),
                }
            }
            hir::ExprKind::Break(dest, value) => {
                let target = dest.target_id.and_then(|id| {
                    let mut bs = self.breakables.borrow_mut();
                    let b = bs.iter_mut().rev().find(|b| b.id == id)?;
                    b.may_break = true;
                    Some(b.target)
                });
                match (target, value) {
                    (Some(Some(t)), Some(v)) => {
                        self.check_expr_coercible_to_type(v, t);
                    }
                    (Some(Some(t)), None) => {
                        if self.infcx.commit_if_ok(|| self.infcx.unify(t, tcx.types.unit)).is_err() {
                            self.err(e.span, "mismatched types")
                                .with_code(ErrCode(308))
                                .with_span_label(e.span, format!("expected `{}`, found `()`", self.ty_str(t)))
                                .with_help("give the `break` a value of the expected type")
                                .emit();
                        }
                    }
                    (_, Some(v)) => {
                        self.check_expr(v);
                    }
                    _ => {}
                }
                tcx.types.never
            }
            hir::ExprKind::Continue(_) => tcx.types.never,
            hir::ExprKind::Ret(v) => {
                let ret = *self.ret_tys.borrow().last().expect("`return` outside a body");
                match v {
                    Some(v) => {
                        self.check_expr_coercible_to_type(v, ret);
                    }
                    None => {
                        if self.infcx.commit_if_ok(|| self.infcx.unify(ret, tcx.types.unit)).is_err() {
                            self.err(e.span, "`return` without a value in a function whose return type is not `()`")
                                .with_code(ErrCode(69))
                                .with_span_label(e.span, format!("return type is `{}`", self.ty_str(ret)))
                                .emit();
                        }
                    }
                }
                tcx.types.never
            }
            hir::ExprKind::InlineAsm(asm) => {
                for (op, _) in &asm.operands {
                    match op {
                        hir::InlineAsmOperand::In { expr, .. } | hir::InlineAsmOperand::SymFn { expr } => {
                            self.check_expr(expr);
                        }
                        hir::InlineAsmOperand::InOut { expr, .. } => {
                            self.check_place(expr);
                        }
                        hir::InlineAsmOperand::Out { expr, .. } => {
                            if let Some(x) = expr {
                                self.check_place(x);
                            }
                        }
                        hir::InlineAsmOperand::SplitInOut { in_expr, out_expr, .. } => {
                            self.check_expr(in_expr);
                            if let Some(x) = out_expr {
                                self.check_place(x);
                            }
                        }
                        hir::InlineAsmOperand::Const { .. } => {}
                        hir::InlineAsmOperand::Label { block } => {
                            self.check_block(block, Expectation::HasType(tcx.types.unit));
                        }
                    }
                }
                if asm.options.iter().any(|(o, _)| o.as_str() == "noreturn") { tcx.types.never } else { tcx.types.unit }
            }
            hir::ExprKind::Struct(q, fields, tail) => self.check_struct_expr(e, q, fields, tail),
            hir::ExprKind::ConstBlock(c) => {
                let body = tcx.hir_crate.body(c.body);
                self.check_expr_with_expectation(&body.value, expected)
            }
            hir::ExprKind::Await(x) => {
                self.check_expr(x);
                self.err(e.span, "`.await` isn't supported yet").with_span_label(e.span, "not supported yet").emit();
                tcx.types.error
            }
            hir::ExprKind::Err => tcx.types.error,
        }
    }

    // -----------------------------------------------------------------------
    // Literals
    // -----------------------------------------------------------------------

    fn check_lit(&self, lit: &vsc_ast::token::Lit, expected: Expectation<'tcx>, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        let expected = expected.only_has_type().map(|t| self.infcx.shallow_resolve(t));
        match lit.kind {
            LitKind::Bool => tcx.types.bool,
            LitKind::Char => tcx.types.char,
            LitKind::Byte => tcx.mk_ty(TyKind::Uint(UintTy::U8)),
            LitKind::Str | LitKind::StrRaw(_) => tcx.mk_ref(tcx.regions.re_erased, tcx.types.str_, Mutability::Not),
            LitKind::ByteStr | LitKind::ByteStrRaw(_) => {
                let len = tcx.mk_const(ConstKind::Value(tcx.types.usize, lit.symbol.as_str().len() as u128));
                let arr = tcx.mk_ty(TyKind::Array(tcx.mk_ty(TyKind::Uint(UintTy::U8)), len));
                tcx.mk_ref(tcx.regions.re_erased, arr, Mutability::Not)
            }
            LitKind::CStr | LitKind::CStrRaw(_) => match tcx.require_lang_item(LangItem::CStr, span) {
                Some(d) => tcx.mk_ref(tcx.regions.re_erased, tcx.mk_adt(tcx.adt_def(d), &[]), Mutability::Not),
                None => tcx.types.error,
            },
            LitKind::Integer | LitKind::Float => {
                let ty = self.check_number_lit(lit, expected, span);
                if lit.kind == LitKind::Integer {
                    if let Some(v) = vsc_middle::thir::parse_int_lit(lit.symbol.as_str()) {
                        self.int_literals.borrow_mut().push((span, v, false, ty));
                    }
                }
                ty
            }
            LitKind::Err => tcx.types.error,
        }
    }

    fn check_number_lit(&self, lit: &vsc_ast::token::Lit, expected: Option<Ty<'tcx>>, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        {
            {
                if let Some(suffix) = lit.suffix {
                    return match suffix_ty(tcx, suffix.as_str(), lit.kind == LitKind::Float) {
                        Some(t) => t,
                        None => {
                            let what = if lit.kind == LitKind::Float { "float" } else { "number" };
                            self.err(span, format!("invalid suffix `{suffix}` for {what} literal"))
                                .with_span_label(span, format!("invalid suffix `{suffix}`"))
                                .with_help("the suffix must be one of the numeric types (`uint32`, `int64`, `float32`, ...; Rust's `u32` spellings work too)")
                                .emit();
                            tcx.types.error
                        }
                    };
                }
                match (lit.kind, expected.map(|t| *t.kind())) {
                    (LitKind::Integer, Some(TyKind::Int(_) | TyKind::Uint(_))) => expected.unwrap(),
                    (LitKind::Integer, _) => self.infcx.next_int_var(),
                    (_, Some(TyKind::Float(_))) => expected.unwrap(),
                    _ => self.infcx.next_float_var(),
                }
            }
        }
    }

    /// An integer literal must fit its type (rustc's `overflowing_literals`,
    /// an error by default).
    pub fn check_int_literal(&self, span: Span, value: u128, negated: bool, ty: Ty<'tcx>) {
        use vsc_middle::ty::{IntTy, UintTy};
        let ty = self.infcx.resolve_vars_if_possible(ty);
        let ptr = self.tcx.target.pointer_width;
        let (bits, signed) = match *ty.kind() {
            TyKind::Int(i) => (match i { IntTy::I8 => 8, IntTy::I16 => 16, IntTy::I32 => 32, IntTy::I64 => 64, IntTy::I128 => 128, IntTy::Isize => ptr }, true),
            TyKind::Uint(u) => (match u { UintTy::U8 => 8, UintTy::U16 => 16, UintTy::U32 => 32, UintTy::U64 => 64, UintTy::U128 => 128, UintTy::Usize => ptr }, false),
            _ => return,
        };
        let (min, max): (i128, u128) = if signed {
            (if bits == 128 { i128::MIN } else { -(1i128 << (bits - 1)) }, (1u128 << (bits - 1)) - 1)
        } else {
            (0, if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 })
        };
        // A negated literal may reach the minimum: `-128int8`.
        let fits = if negated { signed && value <= (max + 1) } else { value <= max };
        if fits {
            return;
        }
        let shown = if negated { format!("-{value}") } else { value.to_string() };
        let name = self.ty_str(ty);
        let mut err = self
            .err(span, format!("literal out of range for `{name}`"))
            .with_note(format!("the literal `{shown}` does not fit into the type `{name}` whose range is `{min}..={max}`"));
        let wider = ["int64", "int128"].into_iter().chain(["uint64", "uint128"]).find(|w| {
            let (wb, ws) = match *w { "int64" => (64, true), "int128" => (128, true), "uint64" => (64, false), _ => (128, false) };
            ws == signed && wb > bits && (wb == 128 || value <= if ws { (1u128 << (wb - 1)) - 1 + negated as u128 } else { (1u128 << wb) - 1 })
        });
        if let Some(w) = wider {
            err = err.with_help(format!("consider using the type `{w}` instead"));
        }
        err.emit();
    }

    // -----------------------------------------------------------------------
    // Paths
    // -----------------------------------------------------------------------

    pub fn check_expr_path(&self, q: &hir::QPath, id: HirId, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        match q {
            hir::QPath::Resolved(None, path) => {
                let seg_args = path.segments.last().and_then(|s| s.args.as_deref());
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
    fn item_value_ty(&self, kind: DefKind, def: DefId, args: GenericArgsRef<'tcx>, span: Span) -> Ty<'tcx> {
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
    fn args_for(&self, def: DefId, explicit: Option<&hir::GenericArgs>, span: Span) -> GenericArgsRef<'tcx> {
        let fresh = self.infcx.fresh_args_for_item(def);
        let Some(explicit) = explicit else { return fresh };
        let g = self.tcx.generics_of(def);
        let own = self.lower_own_args(def, explicit, span);
        let mut v = fresh.to_vec();
        v[g.parent_count..].copy_from_slice(&own[g.parent_count..]);
        self.tcx.mk_args(&v)
    }

    fn value_path(&self, res: Res<HirId>, explicit: Option<&hir::GenericArgs>, id: HirId, span: Span) -> Ty<'tcx> {
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
                let args = self.infcx.fresh_args_for_item(ctor);
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
    fn type_relative_value(&self, self_ty: Ty<'tcx>, seg: &hir::PathSegment, id: HirId, span: Span) -> Ty<'tcx> {
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
    fn lang_item_value(&self, item: LangItem, id: HirId, span: Span) -> Ty<'tcx> {
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

    fn check_call(&self, e: &hir::Expr, callee: &hir::Expr, args: &[hir::Expr]) -> Ty<'tcx> {
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
        self.check_args(e.span, &sig, sig.inputs(), args, "function");
        sig.output()
    }

    /// `f(x)` where `f` isn't a function but implements one of the function
    /// traits, as a parameter `F: Fn(int32) -> int32` does: a call of
    /// `Fn.call(&f, (x,))`, `FnMut.callMut(&mut f, (x,))` or
    /// `FnOnce.callOnce(f, (x,))`, trying them in that order (rustc's
    /// `try_overloaded_call_traits`).
    fn check_overloaded_call(&self, e: &hir::Expr, callee: &hir::Expr, t: Ty<'tcx>, derefs: Vec<Ty<'tcx>>, args: &[hir::Expr]) -> Ty<'tcx> {
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

    fn not_callable(&self, callee: &hir::Expr, t: Ty<'tcx>, args: &[hir::Expr]) -> Ty<'tcx> {
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

    fn check_binop(&self, e: &hir::Expr, op: BinOpKind, a: &hir::Expr, b: &hir::Expr, is_assign: bool) -> Ty<'tcx> {
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
                err = err.with_note(
                    "operators on other types use the operator traits (`Add`, `PartialEq`, …), which come from the standard library; vsc-v2 can't build it yet",
                );
            }
            err.emit();
            return if op.is_comparison() { tcx.types.bool } else { tcx.types.error };
        }
        if op.is_comparison() { tcx.types.bool } else { l }
    }

    fn check_unop(&self, e: &hir::Expr, op: UnOp, x: &hir::Expr, expected: Expectation<'tcx>) -> Ty<'tcx> {
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
                _ => {
                    self.err(e.span, format!("cannot apply unary operator `-` to type `{}`", self.ty_str(t))).with_code(ErrCode(600)).emit();
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

    fn check_if(&self, cond: &hir::Expr, then: &hir::Expr, els: Option<&hir::Expr>, expected: Expectation<'tcx>) -> Ty<'tcx> {
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

    fn check_match(&self, scrutinee: &hir::Expr, arms: &[hir::Arm], expected: Expectation<'tcx>) -> Ty<'tcx> {
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

    fn coerce_many(&self, many: &mut CoerceMany<'tcx>, e: &hir::Expr, ty: Ty<'tcx>, msg: &str) {
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

    fn check_stmt(&self, s: &hir::Stmt) {
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

    fn check_closure(&self, e: &hir::Expr, c: &hir::Closure, expected: Expectation<'tcx>) -> Ty<'tcx> {
        let tcx = self.tcx;
        if c.kind != hir::ClosureKind::Closure {
            self.err(e.span, "`async` isn't supported yet").with_span_label(e.span, "not supported yet").emit();
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

    // -----------------------------------------------------------------------
    // Places, fields and indexing
    // -----------------------------------------------------------------------

    /// The left side of an assignment: must be a place.
    fn check_place(&self, e: &hir::Expr) -> Ty<'tcx> {
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

    fn check_field(&self, e: &hir::Expr, base: &hir::Expr, ident: Ident) -> Ty<'tcx> {
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

    fn check_index(&self, e: &hir::Expr, base: &hir::Expr, idx: &hir::Expr) -> Ty<'tcx> {
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

    fn check_struct_expr(&self, e: &hir::Expr, q: &hir::QPath, fields: &[hir::ExprField], tail: &hir::StructTailExpr) -> Ty<'tcx> {
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
    fn has_method(&self, ty: Ty<'tcx>, name: Ident) -> bool {
        self.infcx.probe(|| self.probe_method(ty, name, None).is_some())
    }
}

/// Can `e` be assigned to?
fn is_place(e: &hir::Expr) -> bool {
    match &e.kind {
        hir::ExprKind::Path(hir::QPath::Resolved(None, p)) => matches!(p.res, Res::Local(_) | Res::Def(DefKind::Static { .. }, _) | Res::Err),
        hir::ExprKind::Field(b, _) | hir::ExprKind::Index(b, _, _) => is_place(b) || !matches!(b.kind, hir::ExprKind::Path(_)),
        hir::ExprKind::Unary(UnOp::Deref, _) => true,
        hir::ExprKind::Err => true,
        _ => false,
    }
}

/// The type a numeric literal's suffix names: Rust's (`u8`) or Vertex's
/// (`uint8`) spelling.
fn suffix_ty<'tcx>(tcx: TyCtxt<'tcx>, s: &str, is_float: bool) -> Option<Ty<'tcx>> {
    use vsc_hir::def::FloatTy;
    let kind = match s {
        "f32" | "float32" => TyKind::Float(FloatTy::F32),
        "f64" | "float64" => TyKind::Float(FloatTy::F64),
        _ if is_float => return None,
        "i8" | "int8" => TyKind::Int(IntTy::I8),
        "i16" | "int16" => TyKind::Int(IntTy::I16),
        "i32" | "int32" => TyKind::Int(IntTy::I32),
        "i64" | "int64" => TyKind::Int(IntTy::I64),
        "i128" | "int128" => TyKind::Int(IntTy::I128),
        "isize" | "int" => TyKind::Int(IntTy::Isize),
        "u8" | "uint8" => TyKind::Uint(UintTy::U8),
        "u16" | "uint16" => TyKind::Uint(UintTy::U16),
        "u32" | "uint32" => TyKind::Uint(UintTy::U32),
        "u64" | "uint64" => TyKind::Uint(UintTy::U64),
        "u128" | "uint128" => TyKind::Uint(UintTy::U128),
        "usize" | "uint" => TyKind::Uint(UintTy::Usize),
        _ => return None,
    };
    Some(tcx.mk_ty(kind))
}

