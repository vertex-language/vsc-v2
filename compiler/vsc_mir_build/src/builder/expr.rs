//! Building expressions (rustc's `builder/expr/{into,as_place,as_operand,
//! as_rvalue,as_temp}.rs`).

use vsc_ast::UnOp as AstUnOp;
use vsc_ast::token::LitKind;
use vsc_errors::ErrCode;
use vsc_middle::mir::*;
use vsc_middle::thir::{self, ExprId, ExprKind, LogicalOp};
use vsc_middle::ty::{self, FloatTy, Mutability, Ty, TyKind};
use vsc_span::Span;

use super::Builder;

impl<'a, 'tcx> Builder<'a, 'tcx> {
    // -----------------------------------------------------------------------
    // Into a destination
    // -----------------------------------------------------------------------

    /// Evaluates `expr` into `dest`; returns the block where control
    /// continues.
    pub fn expr_into_dest(&mut self, dest: Place<'tcx>, block: BasicBlock, expr: ExprId) -> BasicBlock {
        let e = &self.thir.exprs[expr];
        let span = e.span;
        match &e.kind {
            ExprKind::Use { source } => self.expr_into_dest(dest, block, *source),
            ExprKind::Block { block: b } => self.into_block(dest, block, *b, e.ty),
            ExprKind::If { cond, then, else_opt } => {
                let (then_bb, else_bb) = self.then_else_break(block, *cond);
                let then_end = self.expr_into_dest(dest.clone(), then_bb, *then);
                let else_end = match else_opt {
                    Some(x) => self.expr_into_dest(dest, else_bb, *x),
                    None => {
                        self.push_unit(else_bb, dest, span);
                        else_bb
                    }
                };
                let join = self.new_block();
                self.goto(then_end, join, span);
                self.goto(else_end, join, span);
                join
            }
            ExprKind::Loop { body, id } => {
                let head = self.new_block();
                self.goto(block, head, span);
                let exit = self.new_block();
                self.push_breakable(*id, exit, Some(head), dest);
                let tmp = self.temp(self.tcx.types.unit, span);
                let end = self.expr_into_dest(Place::from_local(tmp), head, *body);
                self.goto(end, head, span);
                self.breakables.pop();
                exit
            }
            ExprKind::Match { scrutinee, arms, .. } => self.match_expr(dest, block, *scrutinee, arms, span),
            ExprKind::LogicalOp { op, lhs, rhs } => {
                let (b, l) = self.as_operand(block, *lhs);
                let short = self.new_block();
                let long = self.new_block();
                let (zero_target, other_target) = match op {
                    LogicalOp::And => (short, long),
                    LogicalOp::Or => (long, short),
                };
                self.terminate(
                    b,
                    TerminatorKind::SwitchInt { discr: l, targets: SwitchTargets { values: vec![(0, zero_target)], otherwise: other_target } },
                    span,
                );
                let value = matches!(op, LogicalOp::Or);
                let c = self.bool_const(value, span);
                self.push_assign(short, dest.clone(), Rvalue::Use(c), span);
                let long_end = self.expr_into_dest(dest, long, *rhs);
                let join = self.new_block();
                self.goto(short, join, span);
                self.goto(long_end, join, span);
                join
            }
            ExprKind::Break { target, value } => {
                let Some(i) = self.breakables.iter().rposition(|b| b.target == *target) else {
                    self.terminate(block, TerminatorKind::Unreachable, span);
                    return self.new_block();
                };
                let (bdest, bblock) = (self.breakables[i].dest.clone(), self.breakables[i].break_block);
                let b = match value {
                    Some(v) => self.expr_into_dest(bdest, block, *v),
                    None => {
                        if self.place_ty(&bdest) == self.tcx.types.unit {
                            self.push_unit(block, bdest, span);
                        }
                        block
                    }
                };
                self.goto(b, bblock, span);
                self.new_block()
            }
            ExprKind::Continue { target } => {
                match self.breakables.iter().rev().find(|b| b.target == *target).and_then(|b| b.continue_block) {
                    Some(head) => self.goto(block, head, span),
                    None => self.terminate(block, TerminatorKind::Unreachable, span),
                }
                self.new_block()
            }
            ExprKind::Return { value } => {
                let b = match value {
                    Some(v) => self.expr_into_dest(Place::from_local(RETURN_PLACE), block, *v),
                    None => {
                        self.push_unit(block, Place::from_local(RETURN_PLACE), span);
                        block
                    }
                };
                self.terminate(b, TerminatorKind::Return, span);
                self.new_block()
            }
            ExprKind::Call { fun, args, fn_span } => {
                // Calling a function value doesn't consume it.
                let (mut b, func) = match self.as_operand(block, *fun) {
                    (b, Operand::Move(p)) => (b, Operand::Copy(p)),
                    other => other,
                };
                let mut ops = Vec::with_capacity(args.len());
                for a in args {
                    let (nb, op) = self.as_operand(b, *a);
                    b = nb;
                    ops.push(op);
                }
                let diverges = matches!(e.ty.kind(), TyKind::Never);
                let next = self.new_block();
                let target = (!diverges).then_some(next);
                self.terminate(b, TerminatorKind::Call { func, args: ops, destination: dest, target, fn_span: *fn_span }, span);
                next
            }
            ExprKind::Assign { lhs, rhs } => {
                let (b, value) = self.as_operand(block, *rhs);
                let (b, place) = self.as_place(b, *lhs);
                self.push_assign(b, place, Rvalue::Use(value), span);
                self.push_unit(b, dest, span);
                b
            }
            ExprKind::AssignOp { op, lhs, rhs } => {
                let (b, place) = self.as_place(block, *lhs);
                let (b, r) = self.as_operand(b, *rhs);
                let ty = self.thir.exprs[*lhs].ty;
                let l = self.operand_of_place(place.clone(), ty);
                let (b, rv) = self.build_binary_op(b, BinOp::from_ast(*op), l, r, ty, span);
                self.push_assign(b, place, rv, span);
                self.push_unit(b, dest, span);
                b
            }
            ExprKind::NeverToAny { source } => {
                // The source never finishes; what follows is unreachable.
                let ty = self.thir.exprs[*source].ty;
                let tmp = self.temp(ty, span);
                let b = self.expr_into_dest(Place::from_local(tmp), block, *source);
                self.terminate(b, TerminatorKind::Unreachable, span);
                self.new_block()
            }
            ExprKind::Let { .. } => {
                let (then_bb, else_bb) = self.then_else_break(block, expr);
                let join = self.new_block();
                let t = self.bool_const(true, span);
                let f = self.bool_const(false, span);
                self.push_assign(then_bb, dest.clone(), Rvalue::Use(t), span);
                self.push_assign(else_bb, dest, Rvalue::Use(f), span);
                self.goto(then_bb, join, span);
                self.goto(else_bb, join, span);
                join
            }
            ExprKind::InlineAsm { operands } => {
                let mut b = block;
                let mut ops = Vec::new();
                for o in operands {
                    let (nb, op) = self.as_operand(b, *o);
                    b = nb;
                    ops.push(op);
                }
                let diverges = matches!(e.ty.kind(), TyKind::Never);
                let next = self.new_block();
                self.terminate(b, TerminatorKind::InlineAsm { operands: ops, target: (!diverges).then_some(next) }, span);
                if !diverges {
                    self.push_unit(next, dest, span);
                }
                next
            }
            _ => {
                let (b, rv) = self.as_rvalue(block, expr);
                self.push_assign(b, dest, rv, span);
                b
            }
        }
    }

    pub fn into_block(&mut self, dest: Place<'tcx>, block: BasicBlock, b: thir::BlockId, block_ty: Ty<'tcx>) -> BasicBlock {
        let blk = &self.thir.blocks[b];
        let span = blk.span;
        let exit = blk.target.map(|_| self.new_block());
        if let (Some(target), Some(exit)) = (blk.target, exit) {
            self.push_breakable(target, exit, None, dest.clone());
        }
        let mut cur = block;
        for &s in &blk.stmts {
            cur = self.stmt(cur, s);
        }
        cur = match blk.expr {
            // Exception 10: a block of type `()` discards its value.
            Some(tail) if self.thir.exprs[tail].ty != block_ty && block_ty == self.tcx.types.unit => {
                let tail_ty = self.thir.exprs[tail].ty;
                let tmp = self.temp(tail_ty, span);
                let end = self.expr_into_dest(Place::from_local(tmp), cur, tail);
                self.push_unit(end, dest.clone(), span);
                end
            }
            Some(tail) => self.expr_into_dest(dest.clone(), cur, tail),
            None => {
                if block_ty == self.tcx.types.unit {
                    self.push_unit(cur, dest.clone(), span);
                }
                cur
            }
        };
        if let Some(exit) = exit {
            self.breakables.pop();
            self.goto(cur, exit, span);
            return exit;
        }
        cur
    }

    fn stmt(&mut self, block: BasicBlock, s: thir::StmtId) -> BasicBlock {
        match &self.thir.stmts[s].kind {
            thir::StmtKind::Expr { expr } => {
                let e = &self.thir.exprs[*expr];
                let tmp = self.temp(e.ty, e.span);
                self.expr_into_dest(Place::from_local(tmp), block, *expr)
            }
            thir::StmtKind::Let { pattern, initializer, else_block, span } => {
                self.let_stmt(block, pattern, *initializer, *else_block, *span)
            }
        }
    }

    /// For a condition: the block where it holds and the block where it
    /// doesn't. `let` conditions bind their variables in the first.
    pub fn then_else_break(&mut self, block: BasicBlock, cond: ExprId) -> (BasicBlock, BasicBlock) {
        let e = &self.thir.exprs[cond];
        let span = e.span;
        match &e.kind {
            ExprKind::Use { source } => self.then_else_break(block, *source),
            ExprKind::LogicalOp { op: LogicalOp::And, lhs, rhs } => {
                let (t1, e1) = self.then_else_break(block, *lhs);
                let (t2, e2) = self.then_else_break(t1, *rhs);
                let els = self.new_block();
                self.goto(e1, els, span);
                self.goto(e2, els, span);
                (t2, els)
            }
            ExprKind::Let { expr, pat } => {
                let (b, place) = self.as_place(block, *expr);
                let fail = self.new_block();
                let mut binds = Vec::new();
                let ok = self.test_pattern(b, place, pat, fail, &mut binds);
                self.emit_bindings(ok, binds);
                (ok, fail)
            }
            _ => {
                let (b, op) = self.as_operand(block, cond);
                let then = self.new_block();
                let els = self.new_block();
                self.terminate(b, TerminatorKind::SwitchInt { discr: op, targets: SwitchTargets { values: vec![(0, els)], otherwise: then } }, span);
                (then, els)
            }
        }
    }

    // -----------------------------------------------------------------------
    // As a place
    // -----------------------------------------------------------------------

    pub fn as_place(&mut self, block: BasicBlock, expr: ExprId) -> (BasicBlock, Place<'tcx>) {
        let e = &self.thir.exprs[expr];
        let span = e.span;
        match &e.kind {
            ExprKind::VarRef { id } => match self.var_indices.get(id) {
                Some(&l) => (block, Place::from_local(l)),
                None => self.as_temp_place(block, expr),
            },
            ExprKind::UpvarRef { var_hir_id, .. } => match self.upvars.get(var_hir_id).copied() {
                Some((field, field_ty, by_ref)) => {
                    let env = Place::from_local(Local::from_usize(1)).deref().field(field, field_ty);
                    (block, if by_ref { env.deref() } else { env })
                }
                None => self.as_temp_place(block, expr),
            },
            ExprKind::Deref { arg } => {
                let (b, p) = self.as_place(block, *arg);
                (b, p.deref())
            }
            ExprKind::Field { lhs, field, .. } => {
                let (b, p) = self.as_place(block, *lhs);
                (b, p.field(*field, e.ty))
            }
            ExprKind::Index { lhs, index } => {
                let index_ty = self.thir.exprs[*index].ty;
                if !matches!(index_ty.kind(), TyKind::Uint(ty::UintTy::Usize)) {
                    self.tcx
                        .dcx()
                        .struct_span_err(span, "indexing by a range isn't supported in MIR yet")
                        .with_note("slicing needs the standard library's `Index` implementations, which vsc-v2 can't build yet")
                        .emit();
                    return self.as_temp_place(block, expr);
                }
                let (b, base) = self.as_place(block, *lhs);
                let (b, idx) = self.as_operand(b, *index);
                let usize_ = self.tcx.types.usize;
                let idx_local = self.temp(usize_, span);
                self.push_assign(b, Place::from_local(idx_local), Rvalue::Use(idx), span);
                let len = self.temp(usize_, span);
                self.push_assign(b, Place::from_local(len), Rvalue::Len(base.clone()), span);
                let lt = self.temp(self.tcx.types.bool, span);
                self.push_assign(
                    b,
                    Place::from_local(lt),
                    Rvalue::BinaryOp(BinOp::Lt, Box::new((Operand::Copy(Place::from_local(idx_local)), Operand::Copy(Place::from_local(len))))),
                    span,
                );
                let next = self.new_block();
                let msg = AssertKind::BoundsCheck { len: Operand::Copy(Place::from_local(len)), index: Operand::Copy(Place::from_local(idx_local)) };
                self.terminate(
                    b,
                    TerminatorKind::Assert { cond: Operand::Move(Place::from_local(lt)), expected: true, msg: Box::new(msg), target: next },
                    span,
                );
                (next, base.project(PlaceElem::Index(idx_local)))
            }
            ExprKind::StaticRef { def_id } => {
                // A static is reached through its address.
                let mutable = matches!(self.tcx.def_kind(*def_id), vsc_hir::def::DefKind::Static { mutability } if mutability.is_mut());
                let ptr_ty = if mutable {
                    self.tcx.mk_ty(TyKind::RawPtr(e.ty, Mutability::Mut))
                } else {
                    self.tcx.mk_ref(self.tcx.regions.re_erased, e.ty, Mutability::Not)
                };
                let tmp = self.temp(ptr_ty, span);
                let c = Operand::Constant(Box::new(ConstOperand { span, ty: ptr_ty, value: ConstValue::StaticRef(*def_id) }));
                self.push_assign(block, Place::from_local(tmp), Rvalue::Use(c), span);
                (block, Place::from_local(tmp).deref())
            }
            _ => self.as_temp_place(block, expr),
        }
    }

    fn as_temp_place(&mut self, block: BasicBlock, expr: ExprId) -> (BasicBlock, Place<'tcx>) {
        let (b, l) = self.as_temp(block, expr);
        (b, Place::from_local(l))
    }

    pub fn as_temp(&mut self, block: BasicBlock, expr: ExprId) -> (BasicBlock, Local) {
        let e = &self.thir.exprs[expr];
        let tmp = self.temp(e.ty, e.span);
        let b = self.expr_into_dest(Place::from_local(tmp), block, expr);
        (b, tmp)
    }

    // -----------------------------------------------------------------------
    // As an operand
    // -----------------------------------------------------------------------

    pub fn as_operand(&mut self, block: BasicBlock, expr: ExprId) -> (BasicBlock, Operand<'tcx>) {
        let e = &self.thir.exprs[expr];
        let span = e.span;
        let constant = |value| Operand::Constant(Box::new(ConstOperand { span, ty: e.ty, value }));
        match &e.kind {
            ExprKind::Literal { lit, neg } => (block, constant(self.lit_value(lit, *neg, e.ty))),
            ExprKind::ZstLiteral => (block, constant(ConstValue::ZeroSized)),
            ExprKind::NamedConst { def_id, args } => (block, constant(ConstValue::Unevaluated(*def_id, args))),
            ExprKind::ConstParam { def_id } => {
                let index = self.tcx.generics_of(self.tcx.parent(*def_id)).param_def_id_to_index.get(def_id).copied().unwrap_or(0);
                let c = self.tcx.mk_const(ty::ConstKind::Param(ty::ParamConst { index, name: self.tcx.item_name(*def_id) }));
                (block, constant(ConstValue::Ty(c)))
            }
            ExprKind::Tuple { fields } if fields.is_empty() => (block, constant(ConstValue::ZeroSized)),
            ExprKind::VarRef { .. } | ExprKind::UpvarRef { .. } | ExprKind::Deref { .. } | ExprKind::Field { .. } | ExprKind::Index { .. } | ExprKind::StaticRef { .. } => {
                let (b, p) = self.as_place(block, expr);
                (b, self.operand_of_place(p, e.ty))
            }
            _ => {
                let (b, l) = self.as_temp(block, expr);
                (b, Operand::Move(Place::from_local(l)))
            }
        }
    }

    /// Copies values of types that are always copied; moves the rest.
    /// (Without the `Copy` trait from the library, user types move.)
    pub fn operand_of_place(&self, p: Place<'tcx>, ty: Ty<'tcx>) -> Operand<'tcx> {
        if is_copy(ty) { Operand::Copy(p) } else { Operand::Move(p) }
    }

    pub fn bool_const(&self, b: bool, span: Span) -> Operand<'tcx> {
        Operand::Constant(Box::new(ConstOperand { span, ty: self.tcx.types.bool, value: ConstValue::Scalar(b as u128) }))
    }

    fn lit_value(&self, lit: &vsc_ast::token::Lit, neg: bool, ty: Ty<'tcx>) -> ConstValue<'tcx> {
        let text = lit.symbol.as_str();
        match lit.kind {
            LitKind::Bool => ConstValue::Scalar((text == "true") as u128),
            LitKind::Char | LitKind::Byte => ConstValue::Scalar(thir::parse_char_lit(text).map_or(0, |c| c as u128)),
            LitKind::Integer => {
                let v = thir::parse_int_lit(text).unwrap_or(0);
                let v = if neg { (v as i128).wrapping_neg() as u128 } else { v };
                ConstValue::Scalar(v & int_mask(self.tcx, ty))
            }
            LitKind::Float => {
                let t: String = text.chars().filter(|&c| c != '_').collect();
                let f: f64 = t.parse().unwrap_or(0.0);
                let f = if neg { -f } else { f };
                match ty.kind() {
                    TyKind::Float(FloatTy::F32) => ConstValue::Scalar((f as f32).to_bits() as u128),
                    _ => ConstValue::Scalar(f.to_bits() as u128),
                }
            }
            _ => ConstValue::Str(lit.symbol),
        }
    }

    // -----------------------------------------------------------------------
    // As an rvalue
    // -----------------------------------------------------------------------

    pub fn as_rvalue(&mut self, block: BasicBlock, expr: ExprId) -> (BasicBlock, Rvalue<'tcx>) {
        let e = &self.thir.exprs[expr];
        let span = e.span;
        let tcx = self.tcx;
        match &e.kind {
            ExprKind::Binary { op, lhs, rhs } => {
                let (b, l) = self.as_operand(block, *lhs);
                let (b, r) = self.as_operand(b, *rhs);
                let lty = self.thir.exprs[*lhs].ty;
                self.build_binary_op(b, BinOp::from_ast(*op), l, r, lty, span)
            }
            ExprKind::Unary { op, arg } => {
                let (b, a) = self.as_operand(block, *arg);
                let op = match op {
                    AstUnOp::Not => UnOp::Not,
                    AstUnOp::Neg => UnOp::Neg,
                    AstUnOp::Deref => unreachable!("dereferences are places"),
                };
                (b, Rvalue::UnaryOp(op, a))
            }
            ExprKind::Cast { source } => {
                let from = self.thir.exprs[*source].ty;
                // A fieldless enum casts through its discriminant.
                if let TyKind::Adt(adt, _) = *from.kind() {
                    if adt.is_enum() {
                        let (b, p) = self.as_place(block, *source);
                        let d = self.temp(tcx.types.isize, span);
                        self.push_assign(b, Place::from_local(d), Rvalue::Discriminant(p), span);
                        return (b, Rvalue::Cast(CastKind::IntToInt, Operand::Move(Place::from_local(d)), e.ty));
                    }
                }
                let (b, op) = self.as_operand(block, *source);
                (b, Rvalue::Cast(cast_kind(from, e.ty), op, e.ty))
            }
            ExprKind::PointerCoercion { cast, source } => {
                let (b, op) = self.as_operand(block, *source);
                (b, Rvalue::Cast(CastKind::PointerCoercion(*cast), op, e.ty))
            }
            ExprKind::Borrow { borrow_kind, arg } => {
                let (b, p) = self.as_place(block, *arg);
                let k = match borrow_kind {
                    thir::BorrowKind::Shared => BorrowKind::Shared,
                    thir::BorrowKind::Mut => BorrowKind::Mut,
                };
                (b, Rvalue::Ref(k, p))
            }
            ExprKind::RawBorrow { mutability, arg } => {
                let (b, p) = self.as_place(block, *arg);
                (b, Rvalue::RawPtr(*mutability, p))
            }
            ExprKind::Tuple { fields } => {
                let (b, ops) = self.operands(block, fields);
                (b, Rvalue::Aggregate(Box::new(AggregateKind::Tuple), ops))
            }
            ExprKind::Array { fields } => {
                let elem = match *e.ty.kind() {
                    TyKind::Array(t, _) => t,
                    _ => tcx.types.error,
                };
                let (b, ops) = self.operands(block, fields);
                (b, Rvalue::Aggregate(Box::new(AggregateKind::Array(elem)), ops))
            }
            ExprKind::Repeat { value, count } => {
                let (b, op) = self.as_operand(block, *value);
                (b, Rvalue::Repeat(op, *count))
            }
            ExprKind::Adt(adt) => {
                let variant = &adt.adt_def.variants()[adt.variant_index];
                let mut b = block;
                let base_place = match adt.base {
                    Some(base) => {
                        let (nb, p) = self.as_place(b, base);
                        b = nb;
                        Some(p)
                    }
                    None => None,
                };
                let mut ops = Vec::new();
                let union = adt.adt_def.kind() == ty::AdtKind::Union;
                let fields: Vec<usize> = if union { adt.fields.iter().map(|(i, _)| *i).collect() } else { (0..variant.fields.len()).collect() };
                for i in fields.iter().copied() {
                    match adt.fields.iter().find(|(f, _)| *f == i) {
                        Some((_, x)) => {
                            let (nb, op) = self.as_operand(b, *x);
                            b = nb;
                            ops.push(op);
                        }
                        None => {
                            let fty = tcx.erase_regions(tcx.instantiate(tcx.type_of(variant.fields[i].did), adt.args));
                            match &base_place {
                                Some(p) => ops.push(self.operand_of_place(p.field(i, fty), fty)),
                                None => ops.push(self.unit_operand(span)),
                            }
                        }
                    }
                }
                let kind = AggregateKind::Adt {
                    def_id: adt.adt_def.did(),
                    variant_index: adt.variant_index,
                    args: adt.args,
                    active_field: if union { fields.first().copied() } else { None },
                };
                (b, Rvalue::Aggregate(Box::new(kind), ops))
            }
            ExprKind::ClassNew { value } => {
                let (b, op) = self.as_operand(block, *value);
                (b, Rvalue::ClassNew(op))
            }
            ExprKind::Closure { closure_id } => {
                let mut ops = Vec::new();
                for c in super::captures(tcx, closure_id.to_def_id()) {
                    let var_ty = match *c.field_ty.kind() {
                        TyKind::Ref(_, t, _) => t,
                        _ => c.field_ty,
                    };
                    let place = self.var_place(c.var, var_ty, span);
                    ops.push(match *c.field_ty.kind() {
                        TyKind::Ref(_, _, Mutability::Mut) if c.by_ref => Rvalue::Ref(BorrowKind::Mut, place),
                        _ if c.by_ref => Rvalue::Ref(BorrowKind::Shared, place),
                        _ => Rvalue::Use(self.operand_of_place(place, var_ty)),
                    });
                }
                // Captures are borrows taken into temporaries.
                let mut operands = Vec::new();
                for (rv, c) in ops.into_iter().zip(super::captures(tcx, closure_id.to_def_id())) {
                    let t = self.temp(c.field_ty, span);
                    self.push_assign(block, Place::from_local(t), rv, span);
                    operands.push(Operand::Move(Place::from_local(t)));
                }
                let args = match *e.ty.kind() {
                    TyKind::Closure(_, args) => args,
                    _ => &[],
                };
                (block, Rvalue::Aggregate(Box::new(AggregateKind::Closure(closure_id.to_def_id(), args)), operands))
            }
            ExprKind::Literal { .. }
            | ExprKind::ZstLiteral
            | ExprKind::NamedConst { .. }
            | ExprKind::ConstParam { .. }
            | ExprKind::VarRef { .. }
            | ExprKind::UpvarRef { .. }
            | ExprKind::Deref { .. }
            | ExprKind::Field { .. }
            | ExprKind::Index { .. }
            | ExprKind::StaticRef { .. } => {
                let (b, op) = self.as_operand(block, expr);
                (b, Rvalue::Use(op))
            }
            _ => {
                let (b, l) = self.as_temp(block, expr);
                (b, Rvalue::Use(Operand::Move(Place::from_local(l))))
            }
        }
    }

    /// A variable's place, from this body or captured.
    fn var_place(&mut self, var: vsc_hir::HirId, _ty: Ty<'tcx>, _span: Span) -> Place<'tcx> {
        if let Some(&l) = self.var_indices.get(&var) {
            return Place::from_local(l);
        }
        if let Some(&(field, field_ty, by_ref)) = self.upvars.get(&var) {
            let env = Place::from_local(Local::from_usize(1)).deref().field(field, field_ty);
            return if by_ref { env.deref() } else { env };
        }
        self.tcx.dcx().struct_span_err(_span, "a function literal captures a variable vsc-v2 can't find").with_code(ErrCode(0)).emit();
        Place::from_local(RETURN_PLACE)
    }

    fn operands(&mut self, block: BasicBlock, exprs: &[ExprId]) -> (BasicBlock, Vec<Operand<'tcx>>) {
        let mut b = block;
        let mut ops = Vec::with_capacity(exprs.len());
        for x in exprs {
            let (nb, op) = self.as_operand(b, *x);
            b = nb;
            ops.push(op);
        }
        (b, ops)
    }

    /// A binary operation, checked as a Rust debug build checks it.
    pub fn build_binary_op(
        &mut self,
        block: BasicBlock,
        op: BinOp,
        l: Operand<'tcx>,
        r: Operand<'tcx>,
        ty: Ty<'tcx>,
        span: Span,
    ) -> (BasicBlock, Rvalue<'tcx>) {
        let tcx = self.tcx;
        let is_int = matches!(ty.kind(), TyKind::Int(_) | TyKind::Uint(_));
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul if is_int => {
                let pair = tcx.mk_tup(&[ty, tcx.types.bool]);
                let t = self.temp(pair, span);
                self.push_assign(block, Place::from_local(t), Rvalue::CheckedBinaryOp(op, Box::new((l.clone(), r.clone()))), span);
                let next = self.new_block();
                let overflowed = Operand::Move(Place::from_local(t).field(1, tcx.types.bool));
                let (l, r) = (as_copy(l), as_copy(r));
                self.terminate(
                    block,
                    TerminatorKind::Assert { cond: overflowed, expected: false, msg: Box::new(AssertKind::Overflow(op, l, r)), target: next },
                    span,
                );
                (next, Rvalue::Use(Operand::Move(Place::from_local(t).field(0, ty))))
            }
            BinOp::Div | BinOp::Rem if is_int => {
                let zero = Operand::Constant(Box::new(ConstOperand { span, ty, value: ConstValue::Scalar(0) }));
                let is_zero = self.temp(tcx.types.bool, span);
                self.push_assign(block, Place::from_local(is_zero), Rvalue::BinaryOp(BinOp::Eq, Box::new((r.clone(), zero))), span);
                let next = self.new_block();
                let msg = if op == BinOp::Div { AssertKind::DivisionByZero(as_copy(l.clone())) } else { AssertKind::RemainderByZero(as_copy(l.clone())) };
                self.terminate(
                    block,
                    TerminatorKind::Assert { cond: Operand::Move(Place::from_local(is_zero)), expected: false, msg: Box::new(msg), target: next },
                    span,
                );
                (next, Rvalue::BinaryOp(op, Box::new((l, r))))
            }
            _ => (block, Rvalue::BinaryOp(op, Box::new((l, r)))),
        }
    }
}

/// An operand read again (in an assert's message): integers are copied.
fn as_copy(o: Operand<'_>) -> Operand<'_> {
    match o {
        Operand::Move(p) => Operand::Copy(p),
        o => o,
    }
}

/// Types whose values are copied rather than moved.
pub fn is_copy(ty: Ty<'_>) -> bool {
    match *ty.kind() {
        TyKind::Bool | TyKind::Char | TyKind::Int(_) | TyKind::Uint(_) | TyKind::Float(_) | TyKind::RawPtr(..) | TyKind::FnDef(..) | TyKind::FnPtr(_) | TyKind::Never => true,
        TyKind::Ref(_, _, m) => !m.is_mut(),
        TyKind::Tuple(tys) => tys.iter().all(|t| is_copy(*t)),
        TyKind::Array(t, _) => is_copy(t),
        _ => false,
    }
}

/// The bits of an integer type's values.
fn int_mask(tcx: vsc_middle::ty::TyCtxt<'_>, ty: Ty<'_>) -> u128 {
    use vsc_middle::ty::{IntTy, UintTy};
    let bits = match *ty.kind() {
        TyKind::Int(IntTy::I8) | TyKind::Uint(UintTy::U8) => 8,
        TyKind::Int(IntTy::I16) | TyKind::Uint(UintTy::U16) => 16,
        TyKind::Int(IntTy::I32) | TyKind::Uint(UintTy::U32) => 32,
        TyKind::Int(IntTy::I64) | TyKind::Uint(UintTy::U64) => 64,
        TyKind::Int(IntTy::Isize) | TyKind::Uint(UintTy::Usize) => tcx.target.pointer_width,
        _ => 128,
    };
    if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 }
}

fn cast_kind(from: Ty<'_>, to: Ty<'_>) -> CastKind {
    let int = |t: Ty<'_>| matches!(t.kind(), TyKind::Int(_) | TyKind::Uint(_) | TyKind::Bool | TyKind::Char);
    let float = |t: Ty<'_>| matches!(t.kind(), TyKind::Float(_));
    let ptr = |t: Ty<'_>| matches!(t.kind(), TyKind::RawPtr(..) | TyKind::Ref(..) | TyKind::FnPtr(_) | TyKind::FnDef(..));
    match () {
        _ if int(from) && int(to) => CastKind::IntToInt,
        _ if int(from) && float(to) => CastKind::IntToFloat,
        _ if float(from) && int(to) => CastKind::FloatToInt,
        _ if float(from) && float(to) => CastKind::FloatToFloat,
        _ if ptr(from) && int(to) => CastKind::PointerExposeProvenance,
        _ if int(from) && ptr(to) => CastKind::PointerWithExposedProvenance,
        _ => CastKind::PtrToPtr,
    }
}
