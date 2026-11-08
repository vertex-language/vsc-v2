//! Walking THIR (rustc's `thir::visit`).

use super::*;

pub trait Visitor<'thir, 'tcx: 'thir>: Sized {
    fn thir(&self) -> &'thir Thir<'tcx>;

    fn visit_expr(&mut self, e: &'thir Expr<'tcx>) {
        walk_expr(self, e)
    }
    fn visit_stmt(&mut self, s: &'thir Stmt<'tcx>) {
        walk_stmt(self, s)
    }
    fn visit_block(&mut self, b: &'thir Block) {
        walk_block(self, b)
    }
    fn visit_arm(&mut self, a: &'thir Arm<'tcx>) {
        walk_arm(self, a)
    }
    fn visit_pat(&mut self, _p: &'thir Pat<'tcx>) {}
}

pub fn walk_expr<'thir, 'tcx: 'thir, V: Visitor<'thir, 'tcx>>(v: &mut V, e: &'thir Expr<'tcx>) {
    let thir = v.thir();
    let ex = |id: ExprId, v: &mut V| v.visit_expr(&thir.exprs[id]);
    match &e.kind {
        ExprKind::If { cond, then, else_opt } => {
            ex(*cond, v);
            ex(*then, v);
            if let Some(x) = else_opt {
                ex(*x, v);
            }
        }
        ExprKind::Call { fun, args, .. } => {
            ex(*fun, v);
            for a in args {
                ex(*a, v);
            }
        }
        ExprKind::Deref { arg }
        | ExprKind::Unary { arg, .. }
        | ExprKind::Borrow { arg, .. }
        | ExprKind::RawBorrow { arg, .. } => ex(*arg, v),
        ExprKind::Cast { source }
        | ExprKind::Use { source }
        | ExprKind::NeverToAny { source }
        | ExprKind::PointerCoercion { source, .. } => ex(*source, v),
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::LogicalOp { lhs, rhs, .. }
        | ExprKind::Assign { lhs, rhs }
        | ExprKind::AssignOp { lhs, rhs, .. } => {
            ex(*lhs, v);
            ex(*rhs, v);
        }
        ExprKind::Loop { body, .. } => ex(*body, v),
        ExprKind::Let { expr, pat } => {
            ex(*expr, v);
            v.visit_pat(pat);
        }
        ExprKind::Match { scrutinee, arms, .. } => {
            ex(*scrutinee, v);
            for a in arms {
                v.visit_arm(&thir.arms[*a]);
            }
        }
        ExprKind::Block { block } => v.visit_block(&thir.blocks[*block]),
        ExprKind::Field { lhs, .. } => ex(*lhs, v),
        ExprKind::Index { lhs, index } => {
            ex(*lhs, v);
            ex(*index, v);
        }
        ExprKind::Break { value, .. } | ExprKind::Return { value } => {
            if let Some(x) = value {
                ex(*x, v);
            }
        }
        ExprKind::Repeat { value, .. } | ExprKind::ClassNew { value } => ex(*value, v),
        ExprKind::Array { fields } | ExprKind::Tuple { fields } | ExprKind::InlineAsm { operands: fields } => {
            for f in fields {
                ex(*f, v);
            }
        }
        ExprKind::Adt(adt) => {
            for (_, f) in &adt.fields {
                ex(*f, v);
            }
            if let Some(b) = adt.base {
                ex(b, v);
            }
        }
        ExprKind::Continue { .. }
        | ExprKind::VarRef { .. }
        | ExprKind::UpvarRef { .. }
        | ExprKind::Closure { .. }
        | ExprKind::Literal { .. }
        | ExprKind::ZstLiteral
        | ExprKind::NamedConst { .. }
        | ExprKind::ConstParam { .. }
        | ExprKind::StaticRef { .. }
        | ExprKind::Error => {}
    }
}

pub fn walk_stmt<'thir, 'tcx: 'thir, V: Visitor<'thir, 'tcx>>(v: &mut V, s: &'thir Stmt<'tcx>) {
    let thir = v.thir();
    match &s.kind {
        StmtKind::Expr { expr } => v.visit_expr(&thir.exprs[*expr]),
        StmtKind::Let { pattern, initializer, else_block, .. } => {
            if let Some(i) = initializer {
                v.visit_expr(&thir.exprs[*i]);
            }
            v.visit_pat(pattern);
            if let Some(b) = else_block {
                v.visit_block(&thir.blocks[*b]);
            }
        }
    }
}

pub fn walk_block<'thir, 'tcx: 'thir, V: Visitor<'thir, 'tcx>>(v: &mut V, b: &'thir Block) {
    let thir = v.thir();
    for s in &b.stmts {
        v.visit_stmt(&thir.stmts[*s]);
    }
    if let Some(e) = b.expr {
        v.visit_expr(&thir.exprs[e]);
    }
}

pub fn walk_arm<'thir, 'tcx: 'thir, V: Visitor<'thir, 'tcx>>(v: &mut V, a: &'thir Arm<'tcx>) {
    let thir = v.thir();
    v.visit_pat(&a.pattern);
    if let Some(g) = a.guard {
        v.visit_expr(&thir.exprs[g]);
    }
    v.visit_expr(&thir.exprs[a.body]);
}
