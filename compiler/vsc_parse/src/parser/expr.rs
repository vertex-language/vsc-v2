//! Expressions.

use vsc_ast::token::{Delimiter, IdentIsRaw, Lit, LitKind, TokenKind};
use vsc_ast::util::parser::{AssocOp, ExprPrecedence, Fixity};
use vsc_ast::{
    AnonConst, Arm, AttrVec, Block, BlockCheckMode, BorrowKind, CaptureBy, Closure, DUMMY_NODE_ID, Expr,
    ExprField, ExprKind, FnDecl, FnRetTy, Label, MethodCall, Mutability, P, Param, PathSegment, RangeLimits,
    StructExpr, StructRest, Ty, TyKind, UnOp,
};
use vsc_errors::Applicability;
use vsc_span::{Ident, Span, Spanned, Symbol, kw, sym};

use super::path::PathStyle;
use super::{PResult, Parser, Restrictions};

fn next_prec(p: ExprPrecedence) -> ExprPrecedence {
    use ExprPrecedence::*;
    match p {
        Jump => Assign,
        Assign => Range,
        Range => LOr,
        LOr => LAnd,
        LAnd => Compare,
        Compare => BitOr,
        BitOr => BitXor,
        BitXor => BitAnd,
        BitAnd => Shift,
        Shift => Sum,
        Sum => Product,
        Product => Cast,
        Cast => Prefix,
        Prefix | Unambiguous => Unambiguous,
    }
}

impl<'a> Parser<'a> {
    pub(crate) fn mk_expr(&self, span: Span, kind: ExprKind) -> P<Expr> {
        P::new(Expr { id: DUMMY_NODE_ID, kind, span, attrs: AttrVec::new() })
    }

    fn mk_expr_err(&self, span: Span) -> P<Expr> {
        self.mk_expr(span, ExprKind::Err)
    }

    pub fn parse_expr(&mut self) -> PResult<'a, P<Expr>> {
        self.parse_expr_res(Restrictions::NONE)
    }

    pub fn parse_expr_res(&mut self, r: Restrictions) -> PResult<'a, P<Expr>> {
        self.with_res(r, |this| this.parse_expr_assoc_with(ExprPrecedence::Assign, None))
    }

    /// An `if`/`while` condition: no struct literals, and `let` is allowed.
    fn parse_expr_cond(&mut self) -> PResult<'a, P<Expr>> {
        self.parse_expr_res(Restrictions::NO_STRUCT_LITERAL.union(Restrictions::ALLOW_LET))
    }

    /// At the start of a statement, a block-like expression is complete: a
    /// following operator starts something else (as in Rust).
    fn expr_is_complete(&self, e: &Expr) -> bool {
        self.restrictions.contains(Restrictions::STMT_EXPR) && e.is_block_like()
    }

    /// Binary operators, by precedence climbing.
    fn parse_expr_assoc_with(&mut self, min_prec: ExprPrecedence, lhs: Option<P<Expr>>) -> PResult<'a, P<Expr>> {
        let mut lhs = match lhs {
            Some(e) => e,
            None => {
                if matches!(self.token.kind, TokenKind::DotDot | TokenKind::DotDotEq | TokenKind::DotDotDot) {
                    return self.parse_expr_prefix_range();
                }
                self.parse_expr_prefix()?
            }
        };
        let lo = lhs.span;
        loop {
            if self.expr_is_complete(&lhs) || self.line_ended() {
                break;
            }
            let Some(op) = AssocOp::from_token(self.token.kind) else { break };
            let prec = op.precedence();
            if prec < min_prec {
                break;
            }
            // `a == b == c` doesn't chain.
            if op.fixity() == Fixity::None {
                if let ExprKind::Binary(prev, ..) = &lhs.kind {
                    if prev.node.is_comparison() && AssocOp::Binary(prev.node).precedence() == prec {
                        self.dcx()
                            .struct_span_err(self.token.span, "comparison operators cannot be chained")
                            .with_help("split the comparison into two with `&&`, or add parentheses")
                            .emit();
                    }
                }
            }
            let op_span = self.token.span;
            self.bump();
            match op {
                AssocOp::Cast => {
                    let ty = self.parse_as_cast_ty()?;
                    lhs = self.mk_expr(lo.to(self.prev_token.span), ExprKind::Cast(lhs, ty));
                    continue;
                }
                AssocOp::Range(limits) => {
                    let rhs = if self.is_at_start_of_range_rhs() {
                        Some(self.with_res(self.restrictions.without(Restrictions::STMT_EXPR), |this| {
                            this.parse_expr_assoc_with(ExprPrecedence::LOr, None)
                        })?)
                    } else {
                        None
                    };
                    if limits == RangeLimits::Closed && rhs.is_none() {
                        self.dcx()
                            .struct_span_err(op_span, "inclusive range with no end")
                            .with_help("an inclusive range must have an end: `a..=b`")
                            .emit();
                    }
                    lhs = self.mk_expr(lo.to(self.prev_token.span), ExprKind::Range(Some(lhs), rhs, limits));
                    break;
                }
                _ => {}
            }
            let rhs_prec = match op.fixity() {
                Fixity::Right => prec,
                Fixity::Left | Fixity::None => next_prec(prec),
            };
            let rhs = self.with_res(self.restrictions.without(Restrictions::STMT_EXPR), |this| {
                this.parse_expr_assoc_with(rhs_prec, None)
            })?;
            let span = lo.to(rhs.span);
            lhs = match op {
                AssocOp::Binary(kind) => {
                    self.mk_expr(span, ExprKind::Binary(Spanned { node: kind, span: op_span }, lhs, rhs))
                }
                AssocOp::AssignOp(kind) => {
                    self.mk_expr(span, ExprKind::AssignOp(Spanned { node: kind, span: op_span }, lhs, rhs))
                }
                AssocOp::Assign => self.mk_expr(span, ExprKind::Assign(lhs, rhs, op_span)),
                AssocOp::Cast | AssocOp::Range(_) => unreachable!(),
            };
        }
        Ok(lhs)
    }

    fn is_at_start_of_range_rhs(&self) -> bool {
        self.token.can_begin_expr()
            && !(self.check(TokenKind::OpenBrace) && self.restrictions.contains(Restrictions::NO_STRUCT_LITERAL))
            && !self.line_ended()
    }

    /// `..b`, `..=b`, `..`.
    fn parse_expr_prefix_range(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        let limits = match self.token.kind {
            TokenKind::DotDot => RangeLimits::HalfOpen,
            TokenKind::DotDotEq => RangeLimits::Closed,
            _ => {
                self.dcx()
                    .struct_span_err(lo, "unexpected token: `...`")
                    .with_span_suggestion(lo, "use `..=` for an inclusive range", "..=", Applicability::MaybeIncorrect)
                    .emit();
                RangeLimits::Closed
            }
        };
        self.bump();
        let rhs = if self.is_at_start_of_range_rhs() {
            Some(self.with_res(self.restrictions.without(Restrictions::STMT_EXPR), |this| {
                this.parse_expr_assoc_with(ExprPrecedence::LOr, None)
            })?)
        } else {
            None
        };
        if limits == RangeLimits::Closed && rhs.is_none() {
            self.dcx().span_err(lo, "inclusive range with no end");
        }
        Ok(self.mk_expr(lo.to(self.prev_token.span), ExprKind::Range(None, rhs, limits)))
    }

    /// Unary operators, `&` borrows, and `let` in conditions.
    fn parse_expr_prefix(&mut self) -> PResult<'a, P<Expr>> {
        let attrs = if self.check(TokenKind::At) { self.parse_outer_attributes()? } else { AttrVec::new() };
        let lo = self.token.span;
        let mut expr = match self.token.kind {
            TokenKind::Bang => self.parse_expr_unary(lo, UnOp::Not)?,
            TokenKind::Minus => self.parse_expr_unary(lo, UnOp::Neg)?,
            TokenKind::Star => self.parse_expr_unary(lo, UnOp::Deref)?,
            TokenKind::Tilde => {
                self.dcx()
                    .struct_span_err(lo, "`~` cannot be used as a unary operator")
                    .with_span_suggestion(lo, "use `!` to perform bitwise not", "!", Applicability::MachineApplicable)
                    .emit();
                self.parse_expr_unary(lo, UnOp::Not)?
            }
            TokenKind::And | TokenKind::AndAnd => self.parse_expr_borrow()?,
            _ if self.check_keyword(kw::Let) => self.parse_expr_let()?,
            _ => self.parse_expr_dot_or_call()?,
        };
        if !attrs.is_empty() {
            let mut all = attrs;
            all.append(&mut expr.attrs);
            expr.attrs = all;
            expr.span = lo.to(expr.span);
        }
        Ok(expr)
    }

    fn parse_expr_unary(&mut self, lo: Span, op: UnOp) -> PResult<'a, P<Expr>> {
        self.bump();
        let operand = self.with_res(self.restrictions.without(Restrictions::STMT_EXPR), |this| this.parse_expr_prefix())?;
        Ok(self.mk_expr(lo.to(operand.span), ExprKind::Unary(op, operand)))
    }

    /// `&x`, `&mut x`, `&raw const x`, `&raw mut x`, and `&&x` (two borrows).
    fn parse_expr_borrow(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        if self.check(TokenKind::AndAnd) {
            self.break_and_eat(TokenKind::And);
            let inner = self.parse_expr_borrow()?;
            return Ok(self.mk_expr(lo.to(inner.span), ExprKind::AddrOf(BorrowKind::Ref, Mutability::Not, inner)));
        }
        self.bump(); // `&`
        let (kind, mutbl) = if self.token.is_keyword(kw::Raw)
            && (self.look_ahead(1).is_keyword(kw::Const) || self.look_ahead(1).is_keyword(kw::Mut))
        {
            self.bump();
            let m = if self.eat_keyword(kw::Mut) {
                Mutability::Mut
            } else {
                self.bump();
                Mutability::Not
            };
            (BorrowKind::Raw, m)
        } else if self.eat_keyword(kw::Mut) {
            (BorrowKind::Ref, Mutability::Mut)
        } else {
            (BorrowKind::Ref, Mutability::Not)
        };
        let operand = self.with_res(self.restrictions.without(Restrictions::STMT_EXPR), |this| this.parse_expr_prefix())?;
        Ok(self.mk_expr(lo.to(operand.span), ExprKind::AddrOf(kind, mutbl, operand)))
    }

    /// `let pat = expr` in a condition.
    fn parse_expr_let(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        if !self.restrictions.contains(Restrictions::ALLOW_LET) {
            self.dcx()
                .struct_span_err(lo, "expected expression, found `let` statement")
                .with_note("only supported directly in conditions of `if` and `while` expressions")
                .emit();
        }
        self.bump();
        let pat = self.parse_pat_allow_top_alt()?;
        self.expect(TokenKind::Eq)?;
        // The scrutinee binds tighter than `&&`, so `let a = b && c` chains.
        let scrutinee = self.with_res(self.restrictions.without(Restrictions::ALLOW_LET), |this| {
            this.parse_expr_assoc_with(ExprPrecedence::Compare, None)
        })?;
        let span = lo.to(scrutinee.span);
        Ok(self.mk_expr(span, ExprKind::Let(pat, scrutinee, span)))
    }

    /// A primary expression followed by `.field`, `.method()`, `(args)`,
    /// `[index]`, `?` and `.await`.
    fn parse_expr_dot_or_call(&mut self) -> PResult<'a, P<Expr>> {
        let base = self.parse_expr_bottom()?;
        self.parse_expr_dot_or_call_with(base)
    }

    fn parse_expr_dot_or_call_with(&mut self, mut e: P<Expr>) -> PResult<'a, P<Expr>> {
        let lo = e.span;
        loop {
            if self.check(TokenKind::Question) && !self.at_line_break() {
                self.bump();
                e = self.mk_expr(lo.to(self.prev_token.span), ExprKind::Try(e));
                continue;
            }
            // `.` continues even on the next line (exception 10, rule 2).
            if self.check(TokenKind::Dot) {
                e = self.parse_dot_suffix(lo, e)?;
                continue;
            }
            if self.expr_is_complete(&e) {
                break;
            }
            if self.check(TokenKind::OpenParen) && !self.at_line_break() {
                let (args, _) =
                    self.with_res(Restrictions::NONE, |this| this.parse_delim_comma_seq(Delimiter::Parenthesis, |p| p.parse_expr()))?;
                e = self.mk_expr(lo.to(self.prev_token.span), ExprKind::Call(e, args));
                continue;
            }
            if self.check(TokenKind::OpenBracket) && !self.at_line_break() {
                let bracket_lo = self.token.span;
                self.bump();
                let index = self.with_nl(false, |this| this.with_res(Restrictions::NONE, |this| this.parse_expr()))?;
                self.with_nl(false, |this| this.expect(TokenKind::CloseBracket))?;
                let span = bracket_lo.to(self.prev_token.span);
                e = self.mk_expr(lo.to(self.prev_token.span), ExprKind::Index(e, index, span));
                continue;
            }
            break;
        }
        Ok(e)
    }

    /// After `.`: a field, a tuple index, a method call or `.await`.
    fn parse_dot_suffix(&mut self, lo: Span, e: P<Expr>) -> PResult<'a, P<Expr>> {
        self.bump(); // `.`
        if self.check_keyword(kw::Await) {
            let span = self.token.span;
            self.bump();
            return Ok(self.mk_expr(lo.to(span), ExprKind::Await(e, span)));
        }
        if let TokenKind::Literal(Lit { kind, symbol, suffix }) = self.token.kind {
            let span = self.token.span;
            match kind {
                LitKind::Integer if suffix.is_none() => {
                    self.bump();
                    return Ok(self.mk_expr(lo.to(span), ExprKind::Field(e, Ident::new(symbol, span))));
                }
                // `x.0.1` lexes as `x` `.` `0.1`.
                LitKind::Float if suffix.is_none() => {
                    let text = symbol.as_str();
                    if let Some((a, b)) = text.split_once('.') {
                        if !a.is_empty() && !b.is_empty() && a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit()) {
                            self.bump();
                            let mid = vsc_span::BytePos(span.lo().0 + a.len() as u32);
                            let first = Ident::new(Symbol::intern(a), Span::new(span.lo(), mid));
                            let second = Ident::new(Symbol::intern(b), Span::new(vsc_span::BytePos(mid.0 + 1), span.hi()));
                            let inner = self.mk_expr(lo.to(first.span), ExprKind::Field(e, first));
                            return Ok(self.mk_expr(lo.to(span), ExprKind::Field(inner, second)));
                        }
                    }
                }
                _ => {}
            }
            self.dcx().span_err(span, "unexpected token after `.`: expected a field name or tuple index");
            self.bump();
            return Ok(self.mk_expr_err(lo.to(span)));
        }
        let ident = self.parse_path_segment_ident()?;
        if self.check(TokenKind::OpenParen) && !self.at_line_break() {
            let (args, _) =
                self.with_res(Restrictions::NONE, |this| this.parse_delim_comma_seq(Delimiter::Parenthesis, |p| p.parse_expr()))?;
            let span = lo.to(self.prev_token.span);
            let call = MethodCall { seg: PathSegment::from_ident(ident), receiver: e, args, span: ident.span.to(span) };
            return Ok(self.mk_expr(span, ExprKind::MethodCall(P::new(call))));
        }
        Ok(self.mk_expr(lo.to(ident.span), ExprKind::Field(e, ident)))
    }

    /// Literals, paths, groups, blocks and keyword expressions.
    fn parse_expr_bottom(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        if let TokenKind::Literal(lit) = self.token.kind {
            self.bump();
            return Ok(self.mk_expr(lo, ExprKind::Lit(lit)));
        }
        if self.token.is_bool_lit() {
            let lit = Lit::new(LitKind::Bool, self.token.ident().unwrap().0.name, None);
            self.bump();
            return Ok(self.mk_expr(lo, ExprKind::Lit(lit)));
        }
        match self.token.kind {
            TokenKind::OpenParen => return self.parse_expr_tuple_parens(),
            TokenKind::OpenBracket => {
                let kind = self.parse_expr_array_or_repeat()?;
                return Ok(self.mk_expr(lo.to(self.prev_token.span), kind));
            }
            TokenKind::OpenBrace => {
                let block = self.parse_block()?;
                return Ok(self.mk_expr(block.span, ExprKind::Block(block, None)));
            }
            TokenKind::Lifetime(..) => return self.parse_expr_labeled(),
            TokenKind::Lt | TokenKind::Shl => {
                let (qself, path) = self.parse_qpath(PathStyle::Expr)?;
                return Ok(self.mk_expr(lo.to(self.prev_token.span), ExprKind::Path(Some(qself), path)));
            }
            TokenKind::Pound => {
                return Err(self.expected_found("expression"));
            }
            _ => {}
        }
        let Some((ident, is_raw)) = self.token.ident() else {
            return Err(self.expected_found("expression"));
        };
        if is_raw == IdentIsRaw::No {
            let name = ident.name;
            if name == kw::Func || name == kw::Async && self.look_ahead(1).is_keyword(kw::Func) {
                return self.parse_expr_closure();
            }
            if name == kw::Async {
                return self.parse_expr_async_block();
            }
            if name == kw::Fn && self.look_ahead(1).kind == TokenKind::OpenParen {
                self.dcx()
                    .struct_span_err(lo, "function literals are written `func(…) { … }` in Vertex")
                    .with_span_suggestion(lo, "use `func`", "func", Applicability::MachineApplicable)
                    .emit();
                return self.parse_expr_closure();
            }
            if name == kw::Move {
                self.dcx()
                    .struct_span_err(lo, "Vertex has no `move`")
                    .with_note("a function literal that escapes takes ownership of its captures (exception 14)")
                    .with_span_suggestion(lo.with_hi(self.look_ahead(1).span.lo()), "remove `move`", "", Applicability::MachineApplicable)
                    .emit();
                self.bump();
                return self.parse_expr_bottom();
            }
            if name == kw::If {
                return self.parse_expr_if();
            }
            if name == kw::Match {
                return self.parse_expr_match();
            }
            if name == kw::Loop || name == kw::While || name == kw::For {
                return self.parse_expr_loop_kind(None, lo);
            }
            if name == kw::Unsafe && self.look_ahead(1).kind == TokenKind::OpenBrace {
                self.bump();
                let mut block = self.parse_block()?;
                block.rules = BlockCheckMode::Unsafe;
                return Ok(self.mk_expr(lo.to(block.span), ExprKind::Block(block, None)));
            }
            if name == kw::Const && self.look_ahead(1).kind == TokenKind::OpenBrace {
                self.bump();
                let block = self.parse_block()?;
                let span = block.span;
                let value = self.mk_expr(span, ExprKind::Block(block, None));
                return Ok(self.mk_expr(lo.to(span), ExprKind::ConstBlock(AnonConst { id: DUMMY_NODE_ID, value })));
            }
            if name == kw::Return {
                self.bump();
                let value = if self.expr_can_follow_jump() { Some(self.parse_expr()?) } else { None };
                return Ok(self.mk_expr(lo.to(self.prev_token.span), ExprKind::Ret(value)));
            }
            if name == kw::Break {
                self.bump();
                let label = self.parse_opt_label_use();
                let value = if self.expr_can_follow_jump()
                    && !(self.check(TokenKind::OpenBrace) && self.restrictions.contains(Restrictions::NO_STRUCT_LITERAL))
                {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                return Ok(self.mk_expr(lo.to(self.prev_token.span), ExprKind::Break(label, value)));
            }
            if name == kw::Continue {
                self.bump();
                let label = self.parse_opt_label_use();
                return Ok(self.mk_expr(lo.to(self.prev_token.span), ExprKind::Continue(label)));
            }
            if name == kw::Underscore {
                self.bump();
                return Ok(self.mk_expr(lo, ExprKind::Underscore));
            }
            if name == sym::vec && self.look_ahead(1).kind == TokenKind::OpenBracket && !self.look_ahead(1).line_break_before {
                self.bump();
                let inner_lo = self.token.span;
                let kind = self.parse_expr_array_or_repeat()?;
                let inner = self.mk_expr(inner_lo.to(self.prev_token.span), kind);
                return Ok(self.mk_expr(lo.to(self.prev_token.span), ExprKind::Vec(inner)));
            }
            if (name == sym::asm || name == sym::nakedAsm) && self.look_ahead(1).kind == TokenKind::OpenParen {
                let asm = self.parse_asm()?;
                return Ok(self.mk_expr(lo.to(self.prev_token.span), ExprKind::InlineAsm(asm)));
            }
        }
        if self.check_path_segment_ident() {
            return self.parse_expr_path_start();
        }
        Err(self.expected_found("expression"))
    }

    /// Can a value follow `return` or `break` here? Not after a line break,
    /// and not before `}` or `;`.
    fn expr_can_follow_jump(&self) -> bool {
        self.token.can_begin_expr()
            && !self.at_line_break()
            && !self.check(TokenKind::CloseBrace)
            && !self.check(TokenKind::Semi)
    }

    fn parse_opt_label_use(&mut self) -> Option<Label> {
        let ident = self.token.lifetime()?;
        if self.at_line_break() {
            return None;
        }
        self.bump();
        Some(Label { ident })
    }

    /// A path, then maybe a struct literal: `Point { x: 1, y: 2 }`.
    fn parse_expr_path_start(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        let path = self.parse_path(PathStyle::Expr)?;
        if self.check(TokenKind::OpenBrace)
            && !self.restrictions.contains(Restrictions::NO_STRUCT_LITERAL)
            && !self.at_line_break()
        {
            return self.parse_expr_struct(None, path);
        }
        Ok(self.mk_expr(lo.to(path.span), ExprKind::Path(None, path)))
    }

    /// `Path { field: expr, shorthand, ..base }`. Fields keep commas.
    fn parse_expr_struct(&mut self, qself: Option<P<vsc_ast::QSelf>>, path: vsc_ast::Path) -> PResult<'a, P<Expr>> {
        let lo = path.span;
        let open_index = self.pos;
        self.bump(); // `{`
        let (fields, rest) = self.with_nl(false, |this| this.with_res(Restrictions::NONE, |this| -> PResult<'a, _> {
            let mut fields = Vec::new();
            let mut rest = StructRest::None;
            loop {
                if this.check(TokenKind::CloseBrace) {
                    break;
                }
                if this.eat(TokenKind::DotDot) {
                    rest = StructRest::Base(this.parse_expr()?);
                    if this.eat(TokenKind::Comma) && !this.check(TokenKind::CloseBrace) {
                        this.dcx().span_err(this.token.span, "cannot use a comma after the base struct");
                    }
                    break;
                }
                match this.parse_expr_field() {
                    Ok(f) => fields.push(f),
                    Err(err) => {
                        err.emit();
                        this.jump_to_close_of(open_index);
                        break;
                    }
                }
                if !this.eat(TokenKind::Comma) {
                    if !this.check(TokenKind::CloseBrace) {
                        let err = this.expected_found("`,` or `}`");
                        err.emit();
                        this.jump_to_close_of(open_index);
                    }
                    break;
                }
            }
            this.expect(TokenKind::CloseBrace)?;
            Ok((fields, rest))
        }))?;
        let span = lo.to(self.prev_token.span);
        Ok(self.mk_expr(span, ExprKind::Struct(P::new(StructExpr { qself, path, fields, rest }))))
    }

    fn parse_expr_field(&mut self) -> PResult<'a, ExprField> {
        let attrs = self.parse_outer_attributes()?;
        let lo = self.token.span;
        // `0: expr` for tuple structs.
        if let TokenKind::Literal(Lit { kind: LitKind::Integer, symbol, suffix: None }) = self.token.kind {
            if self.look_ahead(1).kind == TokenKind::Colon {
                let ident = Ident::new(symbol, self.token.span);
                self.bump();
                self.bump();
                let expr = self.parse_expr()?;
                return Ok(ExprField { attrs, id: DUMMY_NODE_ID, span: lo.to(expr.span), ident, expr, is_shorthand: false });
            }
        }
        let ident = self.parse_ident()?;
        if self.eat(TokenKind::Colon) {
            let expr = self.parse_expr()?;
            return Ok(ExprField { attrs, id: DUMMY_NODE_ID, span: lo.to(expr.span), ident, expr, is_shorthand: false });
        }
        let path = vsc_ast::Path::from_ident(ident);
        let expr = self.mk_expr(ident.span, ExprKind::Path(None, path));
        Ok(ExprField { attrs, id: DUMMY_NODE_ID, span: ident.span, ident, expr, is_shorthand: true })
    }

    /// `()`, `(e)`, `(e,)`, `(a, b)`.
    fn parse_expr_tuple_parens(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        let (mut es, trailing) =
            self.with_res(Restrictions::NONE, |this| this.parse_delim_comma_seq(Delimiter::Parenthesis, |p| p.parse_expr()))?;
        let span = lo.to(self.prev_token.span);
        let kind = if es.len() == 1 && !trailing { ExprKind::Paren(es.pop().unwrap()) } else { ExprKind::Tup(es) };
        Ok(self.mk_expr(span, kind))
    }

    /// `[a, b, c]` or `[x; n]`.
    fn parse_expr_array_or_repeat(&mut self) -> PResult<'a, ExprKind> {
        let open_index = self.pos;
        self.bump(); // `[`
        self.with_nl(false, |this| this.with_res(Restrictions::NONE, |this| -> PResult<'a, _> {
            if this.eat(TokenKind::CloseBracket) {
                return Ok(ExprKind::Array(Vec::new()));
            }
            let first = this.parse_expr()?;
            if this.eat(TokenKind::Semi) {
                let count = this.parse_expr()?;
                this.expect(TokenKind::CloseBracket)?;
                return Ok(ExprKind::Repeat(first, AnonConst { id: DUMMY_NODE_ID, value: count }));
            }
            let mut elems = vec![first];
            while this.eat(TokenKind::Comma) {
                if this.check(TokenKind::CloseBracket) {
                    break;
                }
                match this.parse_expr() {
                    Ok(e) => elems.push(e),
                    Err(err) => {
                        err.emit();
                        this.jump_to_close_of(open_index);
                        break;
                    }
                }
            }
            this.expect(TokenKind::CloseBracket)?;
            Ok(ExprKind::Array(elems))
        }))
    }

    /// `'label: loop { … }`, `'label: while …`, `'label: for …`, `'label: { … }`.
    fn parse_expr_labeled(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        let ident = self.token.lifetime().unwrap();
        self.bump();
        self.expect(TokenKind::Colon)?;
        let label = Some(Label { ident });
        if self.check_keyword(kw::Loop) || self.check_keyword(kw::While) || self.check_keyword(kw::For) {
            return self.parse_expr_loop_kind(label, lo);
        }
        if self.check(TokenKind::OpenBrace) {
            let block = self.parse_block()?;
            return Ok(self.mk_expr(lo.to(block.span), ExprKind::Block(block, label)));
        }
        Err(self.expected_found("`while`, `for`, `loop` or `{` after a label"))
    }

    fn parse_expr_loop_kind(&mut self, label: Option<Label>, lo: Span) -> PResult<'a, P<Expr>> {
        if self.eat_keyword(kw::Loop) {
            let kw_span = self.prev_token.span;
            let body = self.parse_block_expecting("loop")?;
            return Ok(self.mk_expr(lo.to(body.span), ExprKind::Loop(body, label, kw_span)));
        }
        if self.eat_keyword(kw::While) {
            let cond = self.parse_expr_cond()?;
            let body = self.parse_block_expecting("while")?;
            return Ok(self.mk_expr(lo.to(body.span), ExprKind::While(cond, body, label)));
        }
        self.expect_keyword(kw::For)?;
        let pat = self.parse_pat_allow_top_alt()?;
        if !self.eat_keyword(kw::In) {
            let mut err = self.expected_found("`in`");
            err.note("a `for` loop is written `for pattern in iterator { … }`");
            return Err(err);
        }
        let iter = self.parse_expr_res(Restrictions::NO_STRUCT_LITERAL)?;
        let body = self.parse_block_expecting("for")?;
        Ok(self.mk_expr(lo.to(body.span), ExprKind::ForLoop { pat, iter, body, label }))
    }

    /// A block after a keyword, with a helpful error if it's missing.
    fn parse_block_expecting(&mut self, what: &str) -> PResult<'a, P<Block>> {
        if self.check(TokenKind::OpenBrace) {
            return self.parse_block();
        }
        let mut err = self.expected_found("`{`");
        if self.token.line_break_before {
            err.note(format!("the body of `{what}` starts with `{{` on the same line"));
        }
        Err(err)
    }

    /// `if cond { … } else if … else { … }`. `else` may start the next line.
    fn parse_expr_if(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        self.bump(); // `if`
        let cond = self.parse_expr_cond()?;
        let then = self.parse_block_expecting("if")?;
        let els = if self.check_keyword(kw::Else) {
            self.bump();
            if self.check_keyword(kw::If) {
                Some(self.parse_expr_if()?)
            } else {
                let block = self.parse_block_expecting("else")?;
                Some(self.mk_expr(block.span, ExprKind::Block(block, None)))
            }
        } else {
            None
        };
        Ok(self.mk_expr(lo.to(self.prev_token.span), ExprKind::If(cond, then, els)))
    }

    /// `match x { arms }`. Arms are separated by line breaks (exception 10).
    fn parse_expr_match(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        self.bump(); // `match`
        let scrutinee = self.parse_expr_res(Restrictions::NO_STRUCT_LITERAL)?;
        if !self.check(TokenKind::OpenBrace) {
            return Err(self.expected_found("`{`"));
        }
        self.bump();
        let arms = self.with_nl(true, |this| -> PResult<'a, _> {
            let mut arms = Vec::new();
            let mut reported_comma = false;
            loop {
                while this.eat(TokenKind::Semi) {}
                if this.check(TokenKind::CloseBrace) || this.check(TokenKind::Eof) {
                    break;
                }
                match this.parse_arm() {
                    Ok(arm) => arms.push(arm),
                    Err(err) => {
                        err.emit();
                        this.recover_stmt();
                        continue;
                    }
                }
                if this.check(TokenKind::Comma) {
                    if !reported_comma {
                        reported_comma = true;
                        let span = this.token.span;
                        let mut err = this.dcx().struct_span_err(span, "match arms are separated by line breaks, not `,`");
                        this.comma_separator_suggestion(&mut err, span);
                        err.emit();
                    }
                    this.bump();
                    continue;
                }
                if let Err(err) = this.expect_terminator("match arms") {
                    err.emit();
                    this.recover_stmt();
                }
            }
            this.expect(TokenKind::CloseBrace)?;
            Ok(arms)
        })?;
        Ok(self.mk_expr(lo.to(self.prev_token.span), ExprKind::Match(scrutinee, arms)))
    }

    fn parse_arm(&mut self) -> PResult<'a, Arm> {
        let attrs = self.parse_outer_attributes()?;
        let lo = self.token.span;
        let pat = self.parse_pat_allow_top_alt()?;
        let guard = if self.eat_keyword(kw::If) {
            Some(self.parse_expr_res(Restrictions::ALLOW_LET)?)
        } else {
            None
        };
        if !self.eat(TokenKind::FatArrow) {
            let mut err = self.expected_found("`=>`");
            if self.check(TokenKind::RArrow) || self.check(TokenKind::Ge) {
                err.help("match arms use `=>`");
            }
            return Err(err);
        }
        let body = self.parse_expr_res(Restrictions::STMT_EXPR)?;
        Ok(Arm { attrs, pat, guard, body, span: lo.to(self.prev_token.span), id: DUMMY_NODE_ID })
    }

    /// `func(x, y: int64) -> T { … }` and `async func(…) { … }` (exception 14).
    fn parse_expr_closure(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        let asyncness = if self.eat_keyword(kw::Async) { Some(self.prev_token.span) } else { None };
        if !self.eat_keyword(kw::Func) {
            // `fn(` was reported by the caller.
            self.bump();
        }
        let (inputs, _) = self.parse_delim_comma_seq(Delimiter::Parenthesis, |p| p.parse_closure_param())?;
        let output = if self.eat(TokenKind::RArrow) {
            FnRetTy::Ty(self.parse_ty_no_plus()?)
        } else {
            FnRetTy::Default(self.prev_token.span.shrink_to_hi())
        };
        let fn_decl_span = lo.to(self.prev_token.span);
        if !self.check(TokenKind::OpenBrace) {
            let mut err = self.expected_found("`{`");
            err.note("a function literal's body is a block: `func(x) { x + 1 }`");
            return Err(err);
        }
        let body = self.with_res(Restrictions::NONE, |this| this.parse_block())?;
        let closure = Closure {
            capture_clause: CaptureBy::Ref,
            asyncness,
            fn_decl: P::new(FnDecl { inputs, output }),
            body,
            fn_decl_span,
        };
        Ok(self.mk_expr(lo.to(self.prev_token.span), ExprKind::Closure(P::new(closure))))
    }

    fn parse_closure_param(&mut self) -> PResult<'a, Param> {
        let attrs = self.parse_outer_attributes()?;
        let lo = self.token.span;
        let pat = self.parse_pat_no_top_alt()?;
        let ty = if self.eat(TokenKind::Colon) {
            self.parse_ty()?
        } else {
            P::new(Ty { id: DUMMY_NODE_ID, kind: TyKind::Infer, span: pat.span.shrink_to_hi() })
        };
        Ok(Param { attrs, ty, pat, id: DUMMY_NODE_ID, span: lo.to(self.prev_token.span), flat_receiver: false })
    }

    /// `async { … }`
    fn parse_expr_async_block(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        self.bump(); // `async`
        if self.check_keyword(kw::Move) {
            let span = self.token.span;
            self.dcx()
                .struct_span_err(span, "Vertex has no `move`")
                .with_note("an `async` block that escapes takes ownership of its captures (exception 14)")
                .emit();
            self.bump();
        }
        if !self.check(TokenKind::OpenBrace) {
            return Err(self.expected_found("`{` or `func` after `async`"));
        }
        let block = self.with_res(Restrictions::NONE, |this| this.parse_block())?;
        Ok(self.mk_expr(lo.to(block.span), ExprKind::Async(block)))
    }

    /// A literal, or `-` and a literal (in patterns and const arguments).
    pub fn parse_literal_maybe_minus(&mut self) -> PResult<'a, P<Expr>> {
        let lo = self.token.span;
        let neg = self.eat(TokenKind::Minus);
        let lit = match self.token.kind {
            TokenKind::Literal(lit) => lit,
            _ if self.token.is_bool_lit() => Lit::new(LitKind::Bool, self.token.ident().unwrap().0.name, None),
            _ => return Err(self.expected_found("literal")),
        };
        let lit_span = self.token.span;
        self.bump();
        let e = self.mk_expr(lit_span, ExprKind::Lit(lit));
        Ok(if neg { self.mk_expr(lo.to(lit_span), ExprKind::Unary(UnOp::Neg, e)) } else { e })
    }

    pub fn parse_anon_const_expr(&mut self) -> PResult<'a, AnonConst> {
        Ok(AnonConst { id: DUMMY_NODE_ID, value: self.parse_expr()? })
    }
}
