//! Lowering blocks and statements (rustc's `block.rs`).
//!
//! Exception 10: Vertex has no `;` to mark a discarded value, so a block's
//! last expression statement is its value and the others are discarded.
//! Type checking discards the last one too when the block's type is `()`.

use vsc_ast::{self as ast};
use vsc_hir::{self as hir};

use crate::LoweringContext;

impl LoweringContext<'_> {
    pub(crate) fn lower_block(&mut self, b: &ast::Block) -> hir::Block {
        let hir_id = self.lower_node_id(b.id);
        let mut stmts = Vec::with_capacity(b.stmts.len());
        let mut expr = None;
        let last = b.stmts.len().wrapping_sub(1);
        for (i, s) in b.stmts.iter().enumerate() {
            let kind = match &s.kind {
                ast::StmtKind::Let(l) => hir::StmtKind::Let(Box::new(self.lower_local(l))),
                ast::StmtKind::Item(item) => hir::StmtKind::Item(self.lower_item(item)),
                ast::StmtKind::Expr(e) if i == last => {
                    expr = Some(Box::new(self.lower_expr(e)));
                    continue;
                }
                ast::StmtKind::Expr(e) => hir::StmtKind::Semi(self.lower_expr(e)),
            };
            stmts.push(hir::Stmt { hir_id: self.lower_node_id(s.id), kind, span: s.span });
        }
        let rules = match b.rules {
            ast::BlockCheckMode::Default => hir::BlockCheckMode::Default,
            ast::BlockCheckMode::Unsafe => hir::BlockCheckMode::Unsafe,
        };
        hir::Block { hir_id, stmts, expr, rules, span: b.span }
    }

    /// A block as an expression.
    pub(crate) fn lower_block_expr(&mut self, b: &ast::Block) -> hir::Expr {
        let block = self.lower_block(b);
        self.expr_block(block)
    }

    fn lower_local(&mut self, l: &ast::Local) -> hir::LetStmt {
        let hir_id = self.lower_node_id(l.id);
        self.lower_attrs(hir_id, &l.attrs);
        let ty = l.ty.as_ref().map(|t| self.lower_ty(t));
        let (init, els) = match &l.kind {
            ast::LocalKind::Decl => (None, None),
            ast::LocalKind::Init(e) => (Some(self.lower_expr(e)), None),
            ast::LocalKind::InitElse(e, b) => (Some(self.lower_expr(e)), Some(Box::new(self.lower_block(b)))),
        };
        let pat = self.lower_pat_var(&l.pat, l.is_var);
        hir::LetStmt { hir_id, pat, ty, init, els, span: l.span }
    }
}
