//! Checking expressions (rustc's `expr.rs`, `callee.rs`, `op.rs`,
//! `cast.rs`, `_match.rs` and `closure.rs`).

mod control;
mod ops_and_place;
mod path_and_call;

use vsc_ast::token::LitKind;
use vsc_ast::{BorrowKind, UnOp};
use vsc_errors::ErrCode;
use vsc_hir::def::{DefKind, Res};
use vsc_hir::{self as hir, LangItem};
use vsc_middle::ty::*;
use vsc_span::Span;

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
                self.convert_place_to_mutable(a);
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
                self.convert_place_to_mutable(lhs);
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
                if m.is_mut() {
                    self.convert_place_to_mutable(x);
                }
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
            hir::ExprKind::Await(x) => self.check_await(e, x),
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
                let byte_len = vsc_middle::thir::unescape_byte_str(
                    lit.symbol.as_str(),
                    matches!(lit.kind, LitKind::ByteStrRaw(_)),
                )
                .len() as u128;
                let len = tcx.mk_const(ConstKind::Value(tcx.types.usize, byte_len));
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
                                .with_help("the suffix must be one of the numeric types (`u32`, `isize`, `f32`, etc.)")
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

/// The type a numeric literal's suffix names. Suffixes are Rust's (`1u8`,
/// `2.5f32`), as the grammar keeps them.
fn suffix_ty<'tcx>(tcx: TyCtxt<'tcx>, s: &str, is_float: bool) -> Option<Ty<'tcx>> {
    use vsc_hir::def::FloatTy;
    let kind = match s {
        "f32" => TyKind::Float(FloatTy::F32),
        "f64" => TyKind::Float(FloatTy::F64),
        _ if is_float => return None,
        "i8" => TyKind::Int(IntTy::I8),
        "i16" => TyKind::Int(IntTy::I16),
        "i32" => TyKind::Int(IntTy::I32),
        "i64" => TyKind::Int(IntTy::I64),
        "i128" => TyKind::Int(IntTy::I128),
        "isize" => TyKind::Int(IntTy::Isize),
        "u8" => TyKind::Uint(UintTy::U8),
        "u16" => TyKind::Uint(UintTy::U16),
        "u32" => TyKind::Uint(UintTy::U32),
        "u64" => TyKind::Uint(UintTy::U64),
        "u128" => TyKind::Uint(UintTy::U128),
        "usize" => TyKind::Uint(UintTy::Usize),
        _ => return None,
    };
    Some(tcx.mk_ty(kind))
}


/// `Marker.hello`, for diagnostics.
fn callee_text(base: &hir::Ty, seg: &hir::PathSegment) -> String {
    format!("{}.{}", ty_text(base), seg.ident)
}

/// The name a type path was written with.
fn ty_text(t: &hir::Ty) -> String {
    match &t.kind {
        hir::TyKind::Path(hir::QPath::Resolved(None, p)) => p.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join("."),
        _ => "…".to_string(),
    }
}
