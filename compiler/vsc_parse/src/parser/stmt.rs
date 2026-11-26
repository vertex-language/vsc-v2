//! Statements and blocks (exception 10: statements end at line breaks).

use vsc_ast::token::TokenKind;
use vsc_ast::{AttrVec, Block, BlockCheckMode, DUMMY_NODE_ID, Local, LocalKind, P, Stmt, StmtKind};
use vsc_errors::Applicability;
use vsc_span::kw;

use super::{PResult, Parser, Restrictions};

impl<'a> Parser<'a> {
    /// `{ statements }`. A statement that fails to parse is reported and
    /// skipped to the end of its line.
    pub fn parse_block(&mut self) -> PResult<'a, P<Block>> {
        let lo = self.token.span;
        if !self.check(TokenKind::OpenBrace) {
            return Err(self.expected_found("`{`"));
        }
        self.bump();
        let stmts = self.with_nl(true, |this| this.with_res(Restrictions::NONE, |this| this.parse_stmts_until_close()));
        self.expect(TokenKind::CloseBrace)?;
        Ok(P::new(Block { stmts, id: DUMMY_NODE_ID, rules: BlockCheckMode::Default, span: lo.to(self.prev_token.span) }))
    }

    fn parse_stmts_until_close(&mut self) -> Vec<Stmt> {
        let mut stmts = Vec::new();
        loop {
            while self.eat(TokenKind::Semi) {}
            if self.check(TokenKind::CloseBrace) || self.check(TokenKind::Eof) {
                break;
            }
            let before = self.pos;
            match self.parse_full_stmt() {
                Ok(stmt) => {
                    stmts.push(stmt);
                    if let Err(err) = self.expect_terminator("statements") {
                        err.emit();
                        self.recover_stmt();
                    }
                }
                Err(err) => {
                    err.emit();
                    self.recover_stmt();
                }
            }
            if self.pos == before && !self.check(TokenKind::CloseBrace) {
                // Make progress no matter what.
                self.bump();
            }
        }
        stmts
    }

    /// One statement: `let`/`var`, an item, or an expression.
    pub fn parse_full_stmt(&mut self) -> PResult<'a, Stmt> {
        let lo = self.token.span;
        let attrs = self.parse_outer_attributes()?;
        if self.check_keyword(kw::Let) || self.check_keyword(kw::Var) {
            let local = self.parse_local(attrs)?;
            let span = lo.to(self.prev_token.span);
            return Ok(Stmt { id: DUMMY_NODE_ID, kind: StmtKind::Let(local), span });
        }
        if self.is_item_start() {
            let item = self.parse_item_common(attrs)?;
            let span = lo.to(self.prev_token.span);
            return Ok(Stmt { id: DUMMY_NODE_ID, kind: StmtKind::Item(item), span });
        }
        let mut expr = self.parse_expr_res(Restrictions::STMT_EXPR)?;
        if !attrs.is_empty() {
            let mut all = attrs;
            all.append(&mut expr.attrs);
            expr.attrs = all;
        }
        let span = lo.to(self.prev_token.span);
        Ok(Stmt { id: DUMMY_NODE_ID, kind: StmtKind::Expr(expr), span })
    }

    /// `let pat: T = init`, `var pat = init`, `let pat = init else { … }`.
    fn parse_local(&mut self, attrs: AttrVec) -> PResult<'a, P<Local>> {
        let lo = self.token.span;
        let mut is_var = self.check_keyword(kw::Var);
        self.bump();
        if !is_var && self.check_keyword(kw::Mut) {
            let span = lo.to(self.token.span);
            self.dcx()
                .struct_span_err(span, "mutable bindings are declared with `var` in Vertex")
                .with_span_suggestion(span, "use `var`", "var", Applicability::MachineApplicable)
                .emit();
            self.bump();
            is_var = true;
        }
        let pat = self.parse_pat_no_top_alt()?;
        let ty = if self.eat(TokenKind::Colon) { Some(self.parse_ty()?) } else { None };
        let kind = if self.eat(TokenKind::Eq) {
            let init = self.parse_expr()?;
            if self.check_keyword(kw::Else) {
                self.bump();
                let els = self.parse_block()?;
                LocalKind::InitElse(init, els)
            } else {
                LocalKind::Init(init)
            }
        } else {
            LocalKind::Decl
        };
        Ok(P::new(Local { id: DUMMY_NODE_ID, is_var, pat, ty, kind, span: lo.to(self.prev_token.span), attrs }))
    }
}
