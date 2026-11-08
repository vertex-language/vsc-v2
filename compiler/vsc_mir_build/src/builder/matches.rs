//! Pattern matching (rustc's `builder/matches`), arm by arm: each arm's
//! pattern is tested against the scrutinee's place, jumping to the next
//! arm on the first test that fails. A pattern's bindings are made once all
//! its tests pass (an or-pattern's, once its alternative matched).

use vsc_hir::HirId;
use vsc_middle::mir::*;
use vsc_middle::thir::{self, ArmId, ByRef, ExprId, Pat, PatConst, PatKind, RangeEnd};
use vsc_middle::ty::{self, AdtDef, Mutability, Ty, TyKind};
use vsc_span::{Span, Symbol};

use super::Builder;

/// A binding to make once its pattern has matched.
pub struct PendingBinding<'tcx> {
    var: HirId,
    name: Symbol,
    mutability: Mutability,
    mode: ByRef,
    /// The variable's type.
    ty: Ty<'tcx>,
    place: Place<'tcx>,
    span: Span,
}

impl<'a, 'tcx> Builder<'a, 'tcx> {
    pub fn match_expr(&mut self, dest: Place<'tcx>, block: BasicBlock, scrutinee: ExprId, arms: &[ArmId], span: Span) -> BasicBlock {
        let (b, place) = self.as_place(block, scrutinee);
        let join = self.new_block();
        let mut cur = b;
        for &arm in arms {
            let arm = &self.thir.arms[arm];
            let next = self.new_block();
            let mut binds = Vec::new();
            let mut ok = self.test_pattern(cur, place.clone(), &arm.pattern, next, &mut binds);
            self.emit_bindings(ok, binds);
            if let Some(g) = arm.guard {
                let (yes, no) = self.then_else_break(ok, g);
                self.goto(no, next, span);
                ok = yes;
            }
            let end = self.expr_into_dest(dest.clone(), ok, arm.body);
            self.goto(end, join, span);
            cur = next;
        }
        // Exhaustiveness was checked: nothing gets past the last arm.
        self.terminate(cur, TerminatorKind::Unreachable, span);
        join
    }

    pub fn let_stmt(&mut self, block: BasicBlock, pat: &Pat<'tcx>, init: Option<ExprId>, else_block: Option<thir::BlockId>, span: Span) -> BasicBlock {
        let Some(init) = init else {
            // `let x` without a value: just the variables.
            self.declare_bindings(pat);
            return block;
        };
        if let (PatKind::Binding { var, name, mutability, mode: ByRef::No, ty, subpattern: None }, None) = (&pat.kind, else_block) {
            let local = self.var_local(*var, *name, *mutability, *ty, pat.span);
            return self.expr_into_dest(Place::from_local(local), block, init);
        }
        let (b, place) = self.as_place(block, init);
        match else_block {
            None => self.bind_irrefutable(b, place, pat),
            Some(els) => {
                let fail = self.new_block();
                let mut binds = Vec::new();
                let ok = self.test_pattern(b, place, pat, fail, &mut binds);
                self.emit_bindings(ok, binds);
                // The `else` block diverges.
                let never = self.tcx.types.never;
                let tmp = self.temp(never, span);
                let end = self.into_block(Place::from_local(tmp), fail, els, never);
                self.terminate(end, TerminatorKind::Unreachable, span);
                ok
            }
        }
    }

    /// Binds an irrefutable pattern (a `let`'s, a parameter's).
    pub fn bind_irrefutable(&mut self, block: BasicBlock, place: Place<'tcx>, pat: &Pat<'tcx>) -> BasicBlock {
        let unreachable = self.new_block();
        let mut binds = Vec::new();
        let ok = self.test_pattern(block, place, pat, unreachable, &mut binds);
        self.terminate(unreachable, TerminatorKind::Unreachable, pat.span);
        self.emit_bindings(ok, binds);
        ok
    }

    fn declare_bindings(&mut self, pat: &Pat<'tcx>) {
        let mut binds = Vec::new();
        let dummy = self.new_block();
        let _ = self.test_pattern(dummy, Place::from_local(RETURN_PLACE), pat, dummy, &mut binds);
        for b in binds {
            self.var_local(b.var, b.name, b.mutability, b.ty, b.span);
        }
        self.terminate(dummy, TerminatorKind::Unreachable, pat.span);
    }

    pub fn emit_bindings(&mut self, block: BasicBlock, binds: Vec<PendingBinding<'tcx>>) {
        for b in binds {
            let local = self.var_local(b.var, b.name, b.mutability, b.ty, b.span);
            let rv = match b.mode {
                ByRef::No => {
                    let op = self.operand_of_place(b.place, b.ty);
                    Rvalue::Use(op)
                }
                ByRef::Yes(Mutability::Not) => Rvalue::Ref(BorrowKind::Shared, b.place),
                ByRef::Yes(Mutability::Mut) => Rvalue::Ref(BorrowKind::Mut, b.place),
            };
            self.push_assign(block, Place::from_local(local), rv, b.span);
        }
    }

    /// Tests `pat` against `place`, going to `fail` if it doesn't match;
    /// returns the block where it does. Bindings are collected, not made.
    pub fn test_pattern(
        &mut self,
        block: BasicBlock,
        place: Place<'tcx>,
        pat: &Pat<'tcx>,
        fail: BasicBlock,
        binds: &mut Vec<PendingBinding<'tcx>>,
    ) -> BasicBlock {
        let tcx = self.tcx;
        let span = pat.span;
        match &pat.kind {
            PatKind::Wild | PatKind::Error => block,
            PatKind::Binding { var, name, mutability, mode, ty, subpattern } => {
                binds.push(PendingBinding { var: *var, name: *name, mutability: *mutability, mode: *mode, ty: *ty, place: place.clone(), span });
                match subpattern {
                    Some(s) => self.test_pattern(block, place, s, fail, binds),
                    None => block,
                }
            }
            PatKind::Deref { subpattern } => self.test_pattern(block, place.deref(), subpattern, fail, binds),
            PatKind::Leaf { subpatterns } => {
                let mut b = block;
                for sp in subpatterns {
                    let field = place.field(sp.field, self.field_ty(pat.ty, None, sp.field, sp.pattern.ty));
                    b = self.test_pattern(b, field, &sp.pattern, fail, binds);
                }
                b
            }
            PatKind::Variant { adt_def, args, variant_index, subpatterns } => {
                let mut b = block;
                if adt_def.variants().len() > 1 {
                    let d = self.temp(tcx.types.isize, span);
                    self.push_assign(b, Place::from_local(d), Rvalue::Discriminant(place.clone()), span);
                    let next = self.new_block();
                    let value = discriminant_value(tcx, *adt_def, *variant_index) as u128;
                    self.terminate(
                        b,
                        TerminatorKind::SwitchInt {
                            discr: Operand::Move(Place::from_local(d)),
                            targets: SwitchTargets { values: vec![(value, next)], otherwise: fail },
                        },
                        span,
                    );
                    b = next;
                }
                let downcast = place.project(PlaceElem::Downcast(*variant_index));
                for sp in subpatterns {
                    let fty = self.variant_field_ty(*adt_def, args, *variant_index, sp.field);
                    b = self.test_pattern(b, downcast.field(sp.field, fty), &sp.pattern, fail, binds);
                }
                b
            }
            PatKind::Constant { value } => match *value {
                PatConst::Int(bits) => self.switch_on(block, place, pat.ty, bits, fail, span),
                PatConst::Bool(v) => self.switch_on(block, place, pat.ty, v as u128, fail, span),
                PatConst::Str(sym) => self.compare(block, place, pat.ty, ConstValue::Str(sym), BinOp::Eq, fail, span),
                PatConst::Float(sym) => {
                    let f: f64 = sym.as_str().parse().unwrap_or(0.0);
                    let bits = match pat.ty.kind() {
                        TyKind::Float(ty::FloatTy::F32) => (f as f32).to_bits() as u128,
                        _ => f.to_bits() as u128,
                    };
                    self.compare(block, place, pat.ty, ConstValue::Scalar(bits), BinOp::Eq, fail, span)
                }
                PatConst::Opaque(d) => self.compare(block, place, pat.ty, ConstValue::Unevaluated(d, &[]), BinOp::Eq, fail, span),
            },
            PatKind::Range { lo, hi, end } => {
                let mut b = block;
                if let Some(PatConst::Int(v)) = lo {
                    b = self.compare(b, place.clone(), pat.ty, ConstValue::Scalar(*v), BinOp::Ge, fail, span);
                }
                if let Some(PatConst::Int(v)) = hi {
                    let op = if *end == RangeEnd::Included { BinOp::Le } else { BinOp::Lt };
                    b = self.compare(b, place, pat.ty, ConstValue::Scalar(*v), op, fail, span);
                }
                b
            }
            PatKind::Slice { prefix, slice, suffix } => {
                let usize_ = tcx.types.usize;
                let len = self.temp(usize_, span);
                self.push_assign(block, Place::from_local(len), Rvalue::Len(place.clone()), span);
                let n = (prefix.len() + suffix.len()) as u128;
                let op = if slice.is_some() { BinOp::Ge } else { BinOp::Eq };
                let b = self.compare(block, Place::from_local(len), usize_, ConstValue::Scalar(n), op, fail, span);
                self.slice_elements(b, place, prefix, slice.as_deref(), suffix, None, fail, binds)
            }
            PatKind::Array { prefix, slice, suffix } => {
                let n = match *pat.ty.kind() {
                    TyKind::Array(_, c) => match *c.kind() {
                        ty::ConstKind::Value(_, n) => Some(n as u64),
                        _ => None,
                    },
                    _ => None,
                };
                self.slice_elements(block, place, prefix, slice.as_deref(), suffix, n, fail, binds)
            }
            PatKind::Or { pats } => {
                let join = self.new_block();
                let mut cur = block;
                for (i, alt) in pats.iter().enumerate() {
                    let alt_fail = if i + 1 == pats.len() { fail } else { self.new_block() };
                    let mut alt_binds = Vec::new();
                    let ok = self.test_pattern(cur, place.clone(), alt, alt_fail, &mut alt_binds);
                    self.emit_bindings(ok, alt_binds);
                    self.goto(ok, join, span);
                    cur = alt_fail;
                }
                join
            }
        }
    }

    fn slice_elements(
        &mut self,
        block: BasicBlock,
        place: Place<'tcx>,
        prefix: &[Pat<'tcx>],
        slice: Option<&Pat<'tcx>>,
        suffix: &[Pat<'tcx>],
        array_len: Option<u64>,
        fail: BasicBlock,
        binds: &mut Vec<PendingBinding<'tcx>>,
    ) -> BasicBlock {
        let (p, s) = (prefix.len() as u64, suffix.len() as u64);
        let min_length = array_len.unwrap_or(p + s);
        let mut b = block;
        for (i, x) in prefix.iter().enumerate() {
            let elem = place.project(PlaceElem::ConstantIndex { offset: i as u64, min_length, from_end: false });
            b = self.test_pattern(b, elem, x, fail, binds);
        }
        for (j, x) in suffix.iter().enumerate() {
            let elem = match array_len {
                Some(n) => place.project(PlaceElem::ConstantIndex { offset: n - s + j as u64, min_length, from_end: false }),
                None => place.project(PlaceElem::ConstantIndex { offset: s - j as u64, min_length, from_end: true }),
            };
            b = self.test_pattern(b, elem, x, fail, binds);
        }
        if let Some(rest) = slice {
            let sub = match array_len {
                Some(n) => PlaceElem::Subslice { from: p, to: n - s, from_end: false },
                None => PlaceElem::Subslice { from: p, to: s, from_end: true },
            };
            b = self.test_pattern(b, place.project(sub), rest, fail, binds);
        }
        b
    }

    /// `switchInt(place) -> [value: next, otherwise: fail]`.
    fn switch_on(&mut self, block: BasicBlock, place: Place<'tcx>, ty: Ty<'tcx>, value: u128, fail: BasicBlock, span: Span) -> BasicBlock {
        let next = self.new_block();
        let discr = self.operand_of_place(place, ty);
        let mask = int_mask(self.tcx, ty);
        self.terminate(
            block,
            TerminatorKind::SwitchInt { discr, targets: SwitchTargets { values: vec![(value & mask, next)], otherwise: fail } },
            span,
        );
        next
    }

    /// `place op value`, going to `fail` when false.
    #[allow(clippy::too_many_arguments)]
    fn compare(&mut self, block: BasicBlock, place: Place<'tcx>, ty: Ty<'tcx>, value: ConstValue<'tcx>, op: BinOp, fail: BasicBlock, span: Span) -> BasicBlock {
        let value = match value {
            ConstValue::Scalar(v) => ConstValue::Scalar(v & int_mask(self.tcx, ty)),
            v => v,
        };
        let c = Operand::Constant(Box::new(ConstOperand { span, ty, value }));
        let t = self.temp(self.tcx.types.bool, span);
        let lhs = self.operand_of_place(place, ty);
        self.push_assign(block, Place::from_local(t), Rvalue::BinaryOp(op, Box::new((lhs, c))), span);
        let next = self.new_block();
        self.terminate(
            block,
            TerminatorKind::SwitchInt { discr: Operand::Move(Place::from_local(t)), targets: SwitchTargets { values: vec![(0, fail)], otherwise: next } },
            span,
        );
        next
    }

    /// A struct's or tuple's field type.
    fn field_ty(&self, ty: Ty<'tcx>, _variant: Option<usize>, field: usize, fallback: Ty<'tcx>) -> Ty<'tcx> {
        match *ty.kind() {
            TyKind::Tuple(tys) => tys.get(field).copied().unwrap_or(fallback),
            TyKind::Adt(adt, args) if !adt.is_enum() => self.variant_field_ty(adt, args, 0, field),
            _ => fallback,
        }
    }

    fn variant_field_ty(&self, adt: AdtDef<'tcx>, args: ty::GenericArgsRef<'tcx>, variant: usize, field: usize) -> Ty<'tcx> {
        let tcx = self.tcx;
        match adt.variants()[variant].fields.get(field) {
            Some(f) => tcx.erase_regions(tcx.instantiate(tcx.type_of(f.did), args)),
            None => tcx.types.error,
        }
    }
}

/// A variant's discriminant: its written value, or one more than the
/// previous variant's (starting at 0), as in Rust.
pub fn discriminant_value(tcx: ty::TyCtxt<'_>, adt: AdtDef<'_>, variant: usize) -> i128 {
    let mut value: i128 = -1;
    for (i, v) in adt.variants().iter().enumerate() {
        value = match v.discr.and_then(|d| literal_discriminant(tcx, d)) {
            Some(x) => x,
            None => value + 1,
        };
        if i == variant {
            break;
        }
    }
    value
}

/// A discriminant written as an integer literal (possibly negative).
fn literal_discriminant(tcx: ty::TyCtxt<'_>, anon: vsc_span::def_id::DefId) -> Option<i128> {
    let thir = tcx.thir_body(anon);
    let mut e = &thir.exprs[thir.value?];
    loop {
        match &e.kind {
            thir::ExprKind::Literal { lit, neg } => {
                let v = thir::parse_int_lit(lit.symbol.as_str())? as i128;
                return Some(if *neg { -v } else { v });
            }
            thir::ExprKind::Use { source } => e = &thir.exprs[*source],
            thir::ExprKind::Block { block } => e = &thir.exprs[thir.blocks[*block].expr?],
            _ => return None,
        }
    }
}

fn int_mask(tcx: ty::TyCtxt<'_>, ty: Ty<'_>) -> u128 {
    use vsc_middle::ty::{IntTy, UintTy};
    let bits = match *ty.kind() {
        TyKind::Int(IntTy::I8) | TyKind::Uint(UintTy::U8) => 8,
        TyKind::Int(IntTy::I16) | TyKind::Uint(UintTy::U16) => 16,
        TyKind::Int(IntTy::I32) | TyKind::Uint(UintTy::U32) | TyKind::Char => 32,
        TyKind::Int(IntTy::I64) | TyKind::Uint(UintTy::U64) => 64,
        TyKind::Int(IntTy::Isize) | TyKind::Uint(UintTy::Usize) => tcx.target.pointer_width,
        TyKind::Bool => 1,
        _ => 128,
    };
    if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 }
}
