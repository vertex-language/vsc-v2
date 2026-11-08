//! Which function traits a function literal implements (rustc's
//! `upvar.rs`, without its precise capture analysis). A literal that
//! assigns to a variable it captures, borrows one mutably, or calls a
//! method or function value on one that needs `&mut`, is `FnMut`; any
//! other is `Fn`. (`FnOnce` literals, which move out of what they capture,
//! need exception 14's by-value captures, which aren't inferred yet.)

use vsc_data_structures::FxHashSet;
use vsc_hir::def::Res;
use vsc_hir::intravisit::{self, Visitor};
use vsc_hir::{self as hir, HirId};
use vsc_middle::ty::*;
use vsc_span::{Span, Symbol};

use crate::FnCtxt;

/// The bindings a body declares, its parameters included.
struct Bindings(FxHashSet<HirId>);

impl<'hir> Visitor<'hir> for Bindings {
    fn visit_pat(&mut self, p: &'hir hir::Pat) {
        if let hir::PatKind::Binding(_, id, _, _) = &p.kind {
            self.0.insert(*id);
        }
        intravisit::walk_pat(self, p);
    }
}

struct Mutations<'a, 'b, 'tcx> {
    fcx: &'a FnCtxt<'b, 'tcx>,
    own: FxHashSet<HirId>,
    mutated: Vec<(HirId, Span, Symbol)>,
}

/// The variable at the base of a place expression: `x` in `x.a[i]` or `*x`.
fn place_base(e: &hir::Expr) -> Option<(HirId, Symbol)> {
    match &e.kind {
        hir::ExprKind::Path(hir::QPath::Resolved(None, p)) => match p.res {
            Res::Local(id) => Some((id, p.segments.last()?.ident.name)),
            _ => None,
        },
        hir::ExprKind::Field(b, _) | hir::ExprKind::Index(b, _, _) | hir::ExprKind::Unary(vsc_ast::UnOp::Deref, b) => place_base(b),
        _ => None,
    }
}

impl Mutations<'_, '_, '_> {
    fn note(&mut self, e: &hir::Expr) {
        if let Some((id, name)) = place_base(e) {
            self.push(id, e.span, name);
        }
    }

    fn push(&mut self, id: HirId, span: Span, name: Symbol) {
        if !self.own.contains(&id) && !self.mutated.iter().any(|(v, _, _)| *v == id) {
            self.mutated.push((id, span, name));
        }
    }

    fn borrows_mutably(&self, e: &hir::Expr) -> bool {
        let results = self.fcx.results.borrow();
        results.adjustments.get(&e.hir_id).is_some_and(|adj| adj.iter().any(|a| a.kind == Adjust::Borrow(Mutability::Mut)))
    }
}

impl<'hir> Visitor<'hir> for Mutations<'_, '_, '_> {
    fn visit_expr(&mut self, e: &'hir hir::Expr) {
        match &e.kind {
            hir::ExprKind::Assign(lhs, _, _) | hir::ExprKind::AssignOp(_, lhs, _) => self.note(lhs),
            hir::ExprKind::AddrOf(_, Mutability::Mut, x) => self.note(x),
            hir::ExprKind::MethodCall(_, recv, _, _) if self.borrows_mutably(recv) => self.note(recv),
            hir::ExprKind::Call(callee, _) => {
                let fn_mut_literal = {
                    let results = self.fcx.results.borrow();
                    match results.node_types.get(&callee.hir_id).map(|t| self.fcx.infcx.shallow_resolve(*t).kind().clone()) {
                        Some(TyKind::Closure(def, _)) => self.fcx.infcx.closure_kinds.borrow().get(&def).is_some_and(|k| *k != ClosureKind::Fn),
                        _ => false,
                    }
                };
                if fn_mut_literal || self.borrows_mutably(callee) {
                    self.note(callee);
                }
            }
            // A literal inside this one mutates what it captures from here.
            hir::ExprKind::Closure(c) => {
                let inner = self.fcx.closure_mutations.borrow().get(&c.def_id).cloned().unwrap_or_default();
                for (id, span, name) in inner {
                    self.push(id, span, name);
                }
            }
            _ => {}
        }
        intravisit::walk_expr(self, e);
    }
}

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    /// A checked literal's kind, and the captured variables it mutates
    /// with where.
    pub fn closure_kind(&self, body: &hir::Body) -> (ClosureKind, Vec<(HirId, Span, Symbol)>) {
        let mut own = Bindings(FxHashSet::default());
        own.visit_body(body);
        let mut m = Mutations { fcx: self, own: own.0, mutated: Vec::new() };
        m.visit_body(body);
        let kind = if m.mutated.is_empty() { ClosureKind::Fn } else { ClosureKind::FnMut };
        (kind, m.mutated)
    }
}
