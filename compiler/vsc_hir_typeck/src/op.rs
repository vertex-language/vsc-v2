//! Operators on values that aren't primitives (rustc's `op.rs` and
//! `place_op.rs`). `a + b` is `Add.add(a, b)`, `a == b` is
//! `PartialEq.eq(&a, &b)`, `a += b` is `AddAssign.addAssign(&mut a, b)`,
//! `-a` is `Neg.neg(a)`, and `a[i]` is `*Index.index(&a, i)`, or
//! `*IndexMut.indexMut(&mut a, i)` where the place is used mutably. The
//! method is recorded on the operator's expression, as for a method call,
//! and THIR lowers it to a call.

use vsc_ast::BinOpKind;
use vsc_hir::def::DefKind;
use vsc_hir::{self as hir, LangItem};
use vsc_middle::ty::*;
use vsc_span::Span;
use vsc_span::def_id::DefId;
use vsc_trait_selection::Selection;

use crate::FnCtxt;

/// The trait and method of a binary operator, compound or not.
fn binop_method(op: BinOpKind, is_assign: bool) -> Option<(LangItem, &'static str)> {
    use BinOpKind as B;
    Some(match (op, is_assign) {
        (B::Add, false) => (LangItem::Add, "add"),
        (B::Sub, false) => (LangItem::Sub, "sub"),
        (B::Mul, false) => (LangItem::Mul, "mul"),
        (B::Div, false) => (LangItem::Div, "div"),
        (B::Rem, false) => (LangItem::Rem, "rem"),
        (B::BitAnd, false) => (LangItem::BitAnd, "bitand"),
        (B::BitOr, false) => (LangItem::BitOr, "bitor"),
        (B::BitXor, false) => (LangItem::BitXor, "bitxor"),
        (B::Shl, false) => (LangItem::Shl, "shl"),
        (B::Shr, false) => (LangItem::Shr, "shr"),
        (B::Add, true) => (LangItem::AddAssign, "addAssign"),
        (B::Sub, true) => (LangItem::SubAssign, "subAssign"),
        (B::Mul, true) => (LangItem::MulAssign, "mulAssign"),
        (B::Div, true) => (LangItem::DivAssign, "divAssign"),
        (B::Rem, true) => (LangItem::RemAssign, "remAssign"),
        (B::BitAnd, true) => (LangItem::BitAndAssign, "bitandAssign"),
        (B::BitOr, true) => (LangItem::BitOrAssign, "bitorAssign"),
        (B::BitXor, true) => (LangItem::BitXorAssign, "bitxorAssign"),
        (B::Shl, true) => (LangItem::ShlAssign, "shlAssign"),
        (B::Shr, true) => (LangItem::ShrAssign, "shrAssign"),
        (B::Eq, false) => (LangItem::PartialEq, "eq"),
        (B::Ne, false) => (LangItem::PartialEq, "ne"),
        (B::Lt, false) => (LangItem::PartialOrd, "lt"),
        (B::Le, false) => (LangItem::PartialOrd, "le"),
        (B::Gt, false) => (LangItem::PartialOrd, "gt"),
        (B::Ge, false) => (LangItem::PartialOrd, "ge"),
        _ => return None,
    })
}

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    /// The method `name` of the operator trait `item` for these types, if
    /// they implement it; the bound is registered.
    fn lookup_op_method(&self, item: LangItem, name: &str, self_ty: Ty<'tcx>, rhs: Option<Ty<'tcx>>, span: Span) -> Option<(DefId, GenericArgsRef<'tcx>)> {
        let tcx = self.tcx;
        let trait_def = tcx.lang_items().get(item)?;
        let mut args = vec![GenericArg::Type(self_ty)];
        if let Some(r) = rhs {
            args.push(GenericArg::Type(r));
        }
        let args = tcx.mk_args(&args);
        let tr = TraitRef { def_id: trait_def, args };
        let selcx = self.selcx();
        if self.infcx.probe(|| matches!(selcx.select(tr, span), Selection::NoSolution | Selection::Ambiguous)) {
            return None;
        }
        let method = tcx.associated_items(trait_def).iter().find(|i| matches!(i.kind, AssocKind::Fn { .. }) && i.name.as_str() == name)?.def_id;
        self.register_trait(tr, span);
        Some((method, args))
    }

    /// `<self_ty as Trait<…>>.Output`, normalized.
    fn op_output(&self, item: LangItem, args: GenericArgsRef<'tcx>, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        let Some(trait_def) = tcx.lang_items().get(item) else { return tcx.types.error };
        let Some(output) = tcx.associated_items(trait_def).iter().find(|i| i.kind == AssocKind::Type).map(|i| i.def_id) else {
            return tcx.types.error;
        };
        self.normalize(tcx.mk_ty(TyKind::Alias(AliasTy { def_id: output, args })), span)
    }

    fn borrow(&self, e: &hir::Expr, ty: Ty<'tcx>, m: Mutability) {
        let tcx = self.tcx;
        self.write_adjustments(e.hir_id, vec![Adjustment { kind: Adjust::Borrow(m), target: tcx.mk_ref(tcx.regions.re_erased, ty, m) }]);
    }

    /// A binary operator through its trait. `None` if the types don't
    /// implement it.
    pub(crate) fn check_overloaded_binop(
        &self,
        e: &hir::Expr,
        op: BinOpKind,
        a: &hir::Expr,
        b: &hir::Expr,
        l: Ty<'tcx>,
        r: Ty<'tcx>,
        is_assign: bool,
    ) -> Option<Ty<'tcx>> {
        let tcx = self.tcx;
        let (item, name) = binop_method(op, is_assign)?;
        let (method, args) = self.lookup_op_method(item, name, l, Some(r), e.span)?;
        self.write_resolution(e.hir_id, DefKind::AssocFn, method);
        self.write_args(e.hir_id, args);
        if op.is_comparison() {
            // `PartialEq.eq(&a, &b)`
            self.borrow(a, l, Mutability::Not);
            self.borrow(b, r, Mutability::Not);
            Some(tcx.types.bool)
        } else if is_assign {
            // `AddAssign.addAssign(&mut a, b)`
            self.borrow(a, l, Mutability::Mut);
            self.convert_place_to_mutable(a);
            Some(tcx.types.unit)
        } else {
            Some(self.op_output(item, args, e.span))
        }
    }

    /// `-x` or `!x` through `Neg` or `Not`.
    pub(crate) fn check_overloaded_unop(&self, e: &hir::Expr, op: vsc_ast::UnOp, t: Ty<'tcx>) -> Option<Ty<'tcx>> {
        let (item, name) = match op {
            vsc_ast::UnOp::Neg => (LangItem::Neg, "neg"),
            vsc_ast::UnOp::Not => (LangItem::Not, "not"),
            vsc_ast::UnOp::Deref => return None,
        };
        let (method, args) = self.lookup_op_method(item, name, t, None, e.span)?;
        self.write_resolution(e.hir_id, DefKind::AssocFn, method);
        self.write_args(e.hir_id, args);
        Some(self.op_output(item, args, e.span))
    }

    /// `base[idx]` through `Index`, autoderefing `base` until a type
    /// implements it. Returns the element type.
    pub(crate) fn check_overloaded_index(&self, e: &hir::Expr, base: &hir::Expr, base_ty: Ty<'tcx>, idx_ty: Ty<'tcx>) -> Option<Ty<'tcx>> {
        let tcx = self.tcx;
        let mut t = self.infcx.shallow_resolve(base_ty);
        let mut adj: Vec<Adjustment<'tcx>> = Vec::new();
        loop {
            if let Some((method, args)) = self.lookup_op_method(LangItem::Index, "index", t, Some(idx_ty), e.span) {
                adj.push(Adjustment { kind: Adjust::Borrow(Mutability::Not), target: tcx.mk_ref(tcx.regions.re_erased, t, Mutability::Not) });
                self.write_adjustments(base.hir_id, adj);
                self.write_resolution(e.hir_id, DefKind::AssocFn, method);
                self.write_args(e.hir_id, args);
                return Some(self.op_output(LangItem::Index, args, e.span));
            }
            let next = self.autoderef_step(t)?;
            adj.push(Adjustment { kind: Adjust::Deref, target: next });
            t = self.infcx.shallow_resolve(next);
        }
    }

    /// A place used mutably (assigned to, borrowed `&mut`, or the receiver
    /// of a `&mut self` method): its overloaded indexing becomes `IndexMut`
    /// (rustc's `convert_place_derefs_to_mutable`).
    pub(crate) fn convert_place_to_mutable(&self, e: &hir::Expr) {
        let tcx = self.tcx;
        match &e.kind {
            hir::ExprKind::Index(base, _, _) => {
                let index = tcx.lang_items().get(LangItem::Index);
                let resolved = self.results.borrow().type_dependent_defs.get(&e.hir_id).copied();
                if let (Some((_, method)), Some(index)) = (resolved, index) {
                    if tcx.opt_parent(method) == Some(index) {
                        let index_mut = tcx.lang_items().get(LangItem::IndexMut).and_then(|d| {
                            tcx.associated_items(d).iter().find(|i| matches!(i.kind, AssocKind::Fn { .. })).map(|i| (d, i.def_id))
                        });
                        if let Some((trait_def, m)) = index_mut {
                            let args = self.results.borrow().node_args.get(&e.hir_id).copied();
                            if let Some(args) = args {
                                self.register_trait(TraitRef { def_id: trait_def, args }, e.span);
                            }
                            self.write_resolution(e.hir_id, DefKind::AssocFn, m);
                            let mut results = self.results.borrow_mut();
                            if let Some(adj) = results.adjustments.get_mut(&base.hir_id) {
                                if let Some(last) = adj.last_mut() {
                                    if let (Adjust::Borrow(_), TyKind::Ref(r, t, _)) = (last.kind, *last.target.kind()) {
                                        last.kind = Adjust::Borrow(Mutability::Mut);
                                        last.target = tcx.mk_ref(r, t, Mutability::Mut);
                                    }
                                }
                            }
                        }
                    }
                }
                self.convert_place_to_mutable(base);
            }
            hir::ExprKind::Field(base, _) => self.convert_place_to_mutable(base),
            _ => {}
        }
    }
}
