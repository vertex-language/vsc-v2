//! Patterns. Mutable bindings are `var x` (exception 5).

use vsc_ast::token::{Delimiter, TokenKind};
use vsc_ast::{
    BindingMode, ByRef, DUMMY_NODE_ID, Expr, ExprKind, Mutability, P, Pat, PatField, PatFieldsRest, PatKind,
    RangeEnd,
};
use vsc_errors::Applicability;
use vsc_span::{Ident, Span, kw};

use super::path::PathStyle;
use super::{PResult, Parser};

impl<'a> Parser<'a> {
    fn mk_pat(&self, kind: PatKind, span: Span) -> P<Pat> {
        P::new(Pat { id: DUMMY_NODE_ID, kind, span })
    }

    /// `A | B | C`, with an optional leading `|`: match arms, `for` loops,
    /// `if let`.
    pub fn parse_pat_allow_top_alt(&mut self) -> PResult<'a, P<Pat>> {
        let lo = self.token.span;
        self.eat(TokenKind::Or);
        let first = self.parse_pat_no_top_alt()?;
        if !self.check(TokenKind::Or) {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.eat(TokenKind::Or) {
            alts.push(self.parse_pat_no_top_alt()?);
        }
        Ok(self.mk_pat(PatKind::Or(alts), lo.to(self.prev_token.span)))
    }

    /// A pattern without top-level `|`: `let` bindings and parameters.
    pub fn parse_pat_no_top_alt(&mut self) -> PResult<'a, P<Pat>> {
        let lo = self.token.span;
        let kind = if self.eat_keyword(kw::Underscore) {
            PatKind::Wild
        } else if self.check(TokenKind::DotDot) {
            self.bump();
            if self.can_begin_range_pat_bound() {
                // `..X` (exclusive upper bound).
                let hi = self.parse_range_pat_bound()?;
                PatKind::Range(None, Some(hi), RangeEnd::Excluded)
            } else {
                PatKind::Rest
            }
        } else if self.eat(TokenKind::DotDotEq) {
            let hi = self.parse_range_pat_bound()?;
            PatKind::Range(None, Some(hi), RangeEnd::Included)
        } else if self.check(TokenKind::And) || self.check(TokenKind::AndAnd) {
            return self.parse_pat_deref();
        } else if self.check(TokenKind::OpenParen) {
            self.parse_pat_tuple_or_parens()?
        } else if self.check(TokenKind::OpenBracket) {
            let (pats, _) = self.parse_delim_comma_seq(Delimiter::Bracket, |p| p.parse_pat_allow_top_alt())?;
            PatKind::Slice(pats)
        } else if self.check_keyword(kw::Ref) {
            self.bump();
            let mutbl = if self.eat_keyword(kw::Mut) { Mutability::Mut } else { Mutability::Not };
            self.parse_pat_ident(BindingMode(ByRef::Yes(mutbl), Mutability::Not))?
        } else if self.eat_keyword(kw::Var) {
            self.parse_pat_ident(BindingMode::VAR)?
        } else if self.check_keyword(kw::Mut) {
            let span = self.token.span;
            self.bump();
            self.dcx()
                .struct_span_err(span, "mutable bindings are written `var` in Vertex")
                .with_span_suggestion(span, "use `var`", "var", Applicability::MachineApplicable)
                .emit();
            self.parse_pat_ident(BindingMode::VAR)?
        } else if self.token.can_begin_literal_maybe_minus() {
            let lit = self.parse_literal_maybe_minus()?;
            return self.maybe_parse_range_pat(lo, lit);
        } else if self.check(TokenKind::Lt) || self.check(TokenKind::Shl) {
            let (qself, path) = self.parse_qpath(PathStyle::Pat)?;
            self.parse_pat_with_path(lo, Some(qself), path)?
        } else if self.check_path_segment_ident() {
            // A lone name is a binding, unless something follows that makes
            // it a path: `.`, `(`, `{`, or a range.
            let next = self.look_ahead(1);
            // Rust's `Foo::Bar` is a path too; the path parser reports the `::`.
            let rust_path_sep = next.kind == TokenKind::Colon
                && self.look_ahead(2).kind == TokenKind::Colon
                && self.look_ahead(2).span.lo() == next.span.hi();
            let is_binding = self.token.is_non_keyword_ident()
                && !rust_path_sep
                && !matches!(
                    next.kind,
                    TokenKind::Dot
                        | TokenKind::OpenParen
                        | TokenKind::OpenBrace
                        | TokenKind::DotDot
                        | TokenKind::DotDotEq
                        | TokenKind::DotDotDot
                );
            if is_binding {
                self.parse_pat_ident(BindingMode::NONE)?
            } else {
                let path = self.parse_path(PathStyle::Pat)?;
                self.parse_pat_with_path(lo, None, path)?
            }
        } else {
            return Err(self.expected_found("pattern"));
        };
        Ok(self.mk_pat(kind, lo.to(self.prev_token.span)))
    }

    /// `name` or `name @ pattern`.
    fn parse_pat_ident(&mut self, mode: BindingMode) -> PResult<'a, PatKind> {
        let ident = self.parse_ident()?;
        let sub = if self.eat(TokenKind::At) { Some(self.parse_pat_no_top_alt()?) } else { None };
        Ok(PatKind::Ident(mode, ident, sub))
    }

    /// `&pat`, `&mut pat`, and `&&pat` (two references).
    fn parse_pat_deref(&mut self) -> PResult<'a, P<Pat>> {
        let lo = self.token.span;
        if self.check(TokenKind::AndAnd) {
            self.break_and_eat(TokenKind::And);
            let inner = self.parse_pat_deref()?;
            return Ok(self.mk_pat(PatKind::Ref(inner, Mutability::Not), lo.to(self.prev_token.span)));
        }
        self.bump(); // `&`
        let mutbl = if self.eat_keyword(kw::Mut) { Mutability::Mut } else { Mutability::Not };
        let inner = self.parse_pat_no_top_alt()?;
        Ok(self.mk_pat(PatKind::Ref(inner, mutbl), lo.to(self.prev_token.span)))
    }

    fn parse_pat_tuple_or_parens(&mut self) -> PResult<'a, PatKind> {
        let (mut pats, trailing) = self.parse_delim_comma_seq(Delimiter::Parenthesis, |p| p.parse_pat_allow_top_alt())?;
        // `(pat)` is a parenthesized pattern; `(..)` and `(pat,)` are tuples.
        Ok(if pats.len() == 1 && !trailing && !matches!(pats[0].kind, PatKind::Rest) {
            PatKind::Paren(pats.pop().unwrap())
        } else {
            PatKind::Tuple(pats)
        })
    }

    /// After a path: `Path(…)`, `Path { … }`, a range, or the path itself.
    fn parse_pat_with_path(
        &mut self,
        lo: Span,
        qself: Option<P<vsc_ast::QSelf>>,
        path: vsc_ast::Path,
    ) -> PResult<'a, PatKind> {
        if self.check(TokenKind::OpenParen) {
            let (pats, _) = self.parse_delim_comma_seq(Delimiter::Parenthesis, |p| p.parse_pat_allow_top_alt())?;
            return Ok(PatKind::TupleStruct(qself, path, pats));
        }
        if self.check(TokenKind::OpenBrace) {
            let (fields, rest) = self.parse_pat_fields()?;
            return Ok(PatKind::Struct(qself, path, fields, rest));
        }
        if matches!(self.token.kind, TokenKind::DotDot | TokenKind::DotDotEq | TokenKind::DotDotDot) {
            let span = path.span;
            let begin = P::new(Expr { id: DUMMY_NODE_ID, kind: ExprKind::Path(qself, path), span, attrs: vec![] });
            let pat = self.maybe_parse_range_pat(lo, begin)?;
            return Ok(pat.kind);
        }
        Ok(PatKind::Path(qself, path))
    }

    /// `{ a, b: pat, ref c, .. }`. Fields are comma-separated, as in Rust.
    fn parse_pat_fields(&mut self) -> PResult<'a, (Vec<PatField>, PatFieldsRest)> {
        let mut rest = PatFieldsRest::None;
        let (fields, _) = self.parse_delim_comma_seq(Delimiter::Brace, |p| -> PResult<'a, Option<PatField>> {
            let attrs = p.parse_outer_attributes()?;
            let lo = p.token.span;
            if p.eat(TokenKind::DotDot) {
                rest = PatFieldsRest::Rest;
                if !p.check(TokenKind::CloseBrace) {
                    p.dcx().span_err(p.token.span, "`..` must be the last part of a struct pattern");
                }
                return Ok(None);
            }
            // `0: pat` for tuple-like variants.
            if let TokenKind::Literal(lit) = p.token.kind {
                if lit.kind == vsc_ast::token::LitKind::Integer && p.look_ahead(1).kind == TokenKind::Colon {
                    let ident = Ident::new(lit.symbol, p.token.span);
                    p.bump();
                    p.bump();
                    let pat = p.parse_pat_allow_top_alt()?;
                    return Ok(Some(PatField {
                        ident,
                        pat,
                        is_shorthand: false,
                        attrs,
                        id: DUMMY_NODE_ID,
                        span: lo.to(p.prev_token.span),
                    }));
                }
            }
            if p.check_ident() && p.look_ahead(1).kind == TokenKind::Colon {
                let ident = p.parse_ident()?;
                p.bump();
                let pat = p.parse_pat_allow_top_alt()?;
                return Ok(Some(PatField {
                    ident,
                    pat,
                    is_shorthand: false,
                    attrs,
                    id: DUMMY_NODE_ID,
                    span: lo.to(p.prev_token.span),
                }));
            }
            // Shorthand: `x`, `ref x`, `ref mut x`, `var x`.
            let mode = if p.eat_keyword(kw::Ref) {
                let m = if p.eat_keyword(kw::Mut) { Mutability::Mut } else { Mutability::Not };
                BindingMode(ByRef::Yes(m), Mutability::Not)
            } else if p.eat_keyword(kw::Var) {
                BindingMode::VAR
            } else {
                BindingMode::NONE
            };
            let ident = p.parse_ident()?;
            let span = lo.to(p.prev_token.span);
            let pat = p.mk_pat(PatKind::Ident(mode, ident, None), span);
            Ok(Some(PatField { ident, pat, is_shorthand: true, attrs, id: DUMMY_NODE_ID, span }))
        })?;
        Ok((fields.into_iter().flatten().collect(), rest))
    }

    fn can_begin_range_pat_bound(&self) -> bool {
        self.token.can_begin_literal_maybe_minus() || self.check_path_segment_ident() || self.check(TokenKind::Lt)
    }

    /// A range pattern's bound: a literal (maybe negative) or a path.
    fn parse_range_pat_bound(&mut self) -> PResult<'a, P<Expr>> {
        if self.token.can_begin_literal_maybe_minus() {
            return self.parse_literal_maybe_minus();
        }
        let lo = self.token.span;
        let (qself, path) = if self.check(TokenKind::Lt) {
            let (q, p) = self.parse_qpath(PathStyle::Pat)?;
            (Some(q), p)
        } else {
            (None, self.parse_path(PathStyle::Pat)?)
        };
        Ok(P::new(Expr { id: DUMMY_NODE_ID, kind: ExprKind::Path(qself, path), span: lo.to(self.prev_token.span), attrs: vec![] }))
    }

    /// After a range pattern's lower bound: `..=`, `..`, or nothing.
    fn maybe_parse_range_pat(&mut self, lo: Span, begin: P<Expr>) -> PResult<'a, P<Pat>> {
        let kind = if self.eat(TokenKind::DotDotEq) {
            let end = self.parse_range_pat_bound()?;
            PatKind::Range(Some(begin), Some(end), RangeEnd::Included)
        } else if self.check(TokenKind::DotDotDot) {
            let span = self.token.span;
            self.bump();
            self.dcx()
                .struct_span_err(span, "`...` range patterns are not allowed")
                .with_span_suggestion(span, "use `..=` for an inclusive range", "..=", Applicability::MachineApplicable)
                .emit();
            let end = self.parse_range_pat_bound()?;
            PatKind::Range(Some(begin), Some(end), RangeEnd::Included)
        } else if self.eat(TokenKind::DotDot) {
            let end = if self.can_begin_range_pat_bound() { Some(self.parse_range_pat_bound()?) } else { None };
            PatKind::Range(Some(begin), end, RangeEnd::Excluded)
        } else {
            PatKind::Expr(begin)
        };
        Ok(self.mk_pat(kind, lo.to(self.prev_token.span)))
    }
}
