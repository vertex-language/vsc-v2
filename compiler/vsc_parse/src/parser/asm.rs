//! `asm(…)`, `nakedAsm(…)` and `globalAsm(…)` (exception 12): Rust's inline
//! assembly macros as built-ins.

use vsc_ast::token::{Lit, LitKind, TokenKind};
use vsc_ast::{AnonConst, AsmMacro, DUMMY_NODE_ID, InlineAsm, InlineAsmOperand, InlineAsmRegOrRegClass, P};
use vsc_span::{Ident, kw, sym};

use super::path::PathStyle;
use super::{PResult, Parser};

impl<'a> Parser<'a> {
    pub fn parse_asm(&mut self) -> PResult<'a, P<InlineAsm>> {
        let lo = self.token.span;
        let name = self.token.ident().map(|(i, _)| i.name);
        let asm_macro = match name {
            Some(n) if n == sym::asm => AsmMacro::Asm,
            Some(n) if n == sym::nakedAsm => AsmMacro::NakedAsm,
            _ => AsmMacro::GlobalAsm,
        };
        self.bump();
        if !self.check(TokenKind::OpenParen) {
            return Err(self.expected_found("`(`"));
        }
        self.bump();
        let mut asm = self.with_nl(false, |this| this.parse_asm_args(asm_macro))?;
        self.with_nl(false, |this| this.expect(TokenKind::CloseParen))?;
        asm.span = lo.to(self.prev_token.span);
        Ok(P::new(asm))
    }

    fn parse_asm_args(&mut self, asm_macro: AsmMacro) -> PResult<'a, InlineAsm> {
        let mut asm = InlineAsm {
            asm_macro,
            templates: Vec::new(),
            operands: Vec::new(),
            clobber_abis: Vec::new(),
            options: Vec::new(),
            span: self.token.span,
        };
        let first = self.parse_asm_template()?;
        asm.templates.push(first);
        loop {
            if !self.eat(TokenKind::Comma) || self.check(TokenKind::CloseParen) {
                break;
            }
            // More template strings come before any operand.
            if asm.operands.is_empty() && asm.clobber_abis.is_empty() && asm.options.is_empty() && self.check_template() {
                let t = self.parse_asm_template()?;
                asm.templates.push(t);
                continue;
            }
            let lo = self.token.span;
            if self.token.is_keyword(sym::clobberAbi) && self.look_ahead(1).kind == TokenKind::OpenParen {
                self.bump();
                let (abis, _) = self.parse_delim_comma_seq(vsc_ast::token::Delimiter::Parenthesis, |p| {
                    let span = p.token.span;
                    match p.parse_str_lit() {
                        Some(s) => Ok((s.symbol, span)),
                        None => Err(p.expected_found("ABI string")),
                    }
                })?;
                asm.clobber_abis.extend(abis);
                continue;
            }
            if self.token.is_keyword(sym::options) && self.look_ahead(1).kind == TokenKind::OpenParen {
                self.bump();
                let (opts, _) = self.parse_delim_comma_seq(vsc_ast::token::Delimiter::Parenthesis, |p| {
                    let span = p.token.span;
                    match p.token.ident() {
                        Some((ident, _)) => {
                            p.bump();
                            Ok((ident.name, span))
                        }
                        None => Err(p.expected_found("asm option")),
                    }
                })?;
                asm.options.extend(opts);
                continue;
            }
            let name = if self.check_ident()
                && self.look_ahead(1).kind == TokenKind::Eq
                && self.look_ahead(2).kind != TokenKind::Eq
            {
                let ident = self.parse_ident()?;
                self.bump(); // `=`
                Some(ident)
            } else {
                None
            };
            let op = self.parse_asm_operand(name)?;
            asm.operands.push((op, lo.to(self.prev_token.span)));
        }
        Ok(asm)
    }

    fn check_template(&self) -> bool {
        matches!(self.token.kind, TokenKind::Literal(Lit { kind: LitKind::Str | LitKind::StrRaw(_), .. }))
    }

    fn parse_asm_template(&mut self) -> PResult<'a, (Lit, vsc_span::Span)> {
        let _attrs = self.parse_outer_attributes()?;
        match self.token.kind {
            TokenKind::Literal(lit @ Lit { kind: LitKind::Str | LitKind::StrRaw(_), .. }) => {
                let span = self.token.span;
                self.bump();
                Ok((lit, span))
            }
            _ => Err(self.expected_found("assembly template string")),
        }
    }

    fn parse_asm_operand(&mut self, name: Option<Ident>) -> PResult<'a, InlineAsmOperand> {
        let dir = self.token.ident().map(|(i, _)| i.name);
        let is = |s| dir == Some(s);
        if self.check_keyword(kw::In) {
            self.bump();
            let reg = self.parse_asm_reg()?;
            let expr = self.parse_expr()?;
            return Ok(InlineAsmOperand::In { name, reg, expr });
        }
        if is(sym::out) || is(sym::lateout) {
            let late = is(sym::lateout);
            self.bump();
            let reg = self.parse_asm_reg()?;
            let expr = if self.eat_keyword(kw::Underscore) { None } else { Some(self.parse_expr()?) };
            return Ok(InlineAsmOperand::Out { name, reg, late, expr });
        }
        if is(sym::inout) || is(sym::inlateout) {
            let late = is(sym::inlateout);
            self.bump();
            let reg = self.parse_asm_reg()?;
            let in_expr = self.parse_expr()?;
            if self.eat(TokenKind::FatArrow) {
                let out_expr = if self.eat_keyword(kw::Underscore) { None } else { Some(self.parse_expr()?) };
                return Ok(InlineAsmOperand::SplitInOut { name, reg, late, in_expr, out_expr });
            }
            return Ok(InlineAsmOperand::InOut { name, reg, late, expr: in_expr });
        }
        if self.eat_keyword(kw::Const) {
            let value = self.parse_expr()?;
            return Ok(InlineAsmOperand::Const { name, anon_const: AnonConst { id: DUMMY_NODE_ID, value } });
        }
        if self.token.is_keyword(sym::sym) {
            self.bump();
            let (qself, path) = if self.check(TokenKind::Lt) {
                let (q, p) = self.parse_qpath(PathStyle::Expr)?;
                (Some(q), p)
            } else {
                (None, self.parse_path(PathStyle::Expr)?)
            };
            return Ok(InlineAsmOperand::Sym { name, id: DUMMY_NODE_ID, qself, path });
        }
        if is(sym::label) {
            self.bump();
            let block = self.parse_block()?;
            return Ok(InlineAsmOperand::Label { name, block });
        }
        Err(self.expected_found("operand, `clobberAbi`, `options`, or additional template string"))
    }

    /// `(reg)` or `("eax")`.
    fn parse_asm_reg(&mut self) -> PResult<'a, InlineAsmRegOrRegClass> {
        self.expect(TokenKind::OpenParen)?;
        let reg = if let Some(s) = self.parse_str_lit() {
            InlineAsmRegOrRegClass::Reg(s.symbol)
        } else if let Some((ident, _)) = self.token.ident() {
            self.bump();
            InlineAsmRegOrRegClass::RegClass(ident.name)
        } else {
            return Err(self.expected_found("register class or explicit register"));
        };
        self.expect(TokenKind::CloseParen)?;
        Ok(reg)
    }
}
