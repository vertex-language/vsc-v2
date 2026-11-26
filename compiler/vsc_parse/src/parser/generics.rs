//! Generic parameters, bounds and where clauses.

use vsc_ast::token::TokenKind;
use vsc_ast::{
    AnonConst, DUMMY_NODE_ID, Expr, ExprKind, GenericBound, GenericBounds, GenericParam, GenericParamKind, Generics,
    Lifetime, P, PolyTraitRef, TraitBoundModifiers, TraitRef, WhereClause, WherePredicate, WherePredicateKind,
};
use vsc_errors::Applicability;
use vsc_span::{Ident, Span, Symbol, kw};

use super::path::PathStyle;
use super::{PResult, Parser};

impl<'a> Parser<'a> {
    /// `<A, B: Bound, const N: uint, src: Lifetime>`, or nothing.
    pub fn parse_generics(&mut self) -> PResult<'a, Generics> {
        if !(self.check(TokenKind::Lt) || self.check(TokenKind::Shl)) || self.token.line_break_before {
            let span = self.prev_token.span.shrink_to_hi();
            return Ok(Generics { params: vec![], where_clause: WhereClause { span, ..Default::default() }, span });
        }
        let lo = self.token.span;
        self.break_and_eat(TokenKind::Lt);
        let params = self.with_nl(false, |this| -> PResult<'a, _> {
            let mut params = Vec::new();
            loop {
                if this.check_or_compound(TokenKind::Gt) {
                    break;
                }
                params.push(this.parse_generic_param()?);
                if !this.eat(TokenKind::Comma) {
                    break;
                }
            }
            this.expect_gt()?;
            Ok(params)
        })?;
        let span = lo.to(self.prev_token.span);
        Ok(Generics { params, where_clause: WhereClause { span: span.shrink_to_hi(), ..Default::default() }, span })
    }

    fn parse_generic_param(&mut self) -> PResult<'a, GenericParam> {
        let attrs = self.parse_outer_attributes()?;
        let lo = self.token.span;

        if let Some(label) = self.token.lifetime() {
            // Rust's `'a`: report it and read it as a lifetime parameter.
            self.bump();
            let name = label.name.as_str().trim_start_matches('\'').to_string();
            self.dcx()
                .struct_span_err(label.span, "lifetimes have no `'` in Vertex")
                .with_span_suggestion(
                    label.span,
                    "remove the leading `'`",
                    name.clone(),
                    Applicability::MachineApplicable,
                )
                .emit();
            let mut bounds = Vec::new();
            if self.eat(TokenKind::Colon) {
                loop {
                    if !self.can_begin_bound() {
                        break;
                    }
                    bounds.push(self.parse_lifetime_bound()?);
                    if !self.check(TokenKind::Plus) || self.line_ended() {
                        break;
                    }
                    self.bump();
                }
            }
            return Ok(GenericParam {
                id: DUMMY_NODE_ID,
                ident: Ident::new(Symbol::intern(&name), label.span),
                attrs,
                bounds,
                kind: GenericParamKind::Lifetime,
                span: lo.to(self.prev_token.span),
            });
        }

        if self.eat_keyword(kw::Const) {
            let kw_span = self.prev_token.span;
            let ident = self.parse_ident()?;
            self.expect(TokenKind::Colon)?;
            let ty = self.parse_ty()?;
            let default = if self.eat(TokenKind::Eq) {
                let value = if self.check(TokenKind::OpenBrace) {
                    let block = self.parse_block()?;
                    let span = block.span;
                    P::new(Expr { id: DUMMY_NODE_ID, kind: ExprKind::Block(block, None), span, attrs: vec![] })
                } else if self.token.can_begin_literal_maybe_minus() {
                    self.parse_literal_maybe_minus()?
                } else {
                    let path = self.parse_path(PathStyle::Expr)?;
                    let span = path.span;
                    P::new(Expr { id: DUMMY_NODE_ID, kind: ExprKind::Path(None, path), span, attrs: vec![] })
                };
                Some(AnonConst { id: DUMMY_NODE_ID, value })
            } else {
                None
            };
            return Ok(GenericParam {
                id: DUMMY_NODE_ID,
                ident,
                attrs,
                bounds: vec![],
                kind: GenericParamKind::Const { ty, kw_span, default },
                span: lo.to(self.prev_token.span),
            });
        }

        let ident = self.parse_ident()?;
        let is_lifetime = ident.name.as_str().starts_with(|c: char| c.is_ascii_lowercase());
        let mut bounds = Vec::new();
        let mut kind = if is_lifetime {
            GenericParamKind::Lifetime
        } else {
            GenericParamKind::Type { default: None }
        };
        if self.eat(TokenKind::Colon) {
            if matches!(kind, GenericParamKind::Lifetime) {
                loop {
                    if !self.can_begin_bound() {
                        break;
                    }
                    bounds.push(self.parse_lifetime_bound()?);
                    if !self.check(TokenKind::Plus) || self.line_ended() {
                        break;
                    }
                    self.bump();
                }
            } else {
                bounds = self.parse_generic_bounds()?;
            }
        }
        if matches!(kind, GenericParamKind::Type { .. }) && self.eat(TokenKind::Eq) {
            kind = GenericParamKind::Type { default: Some(self.parse_ty()?) };
        }
        Ok(GenericParam { id: DUMMY_NODE_ID, ident, attrs, bounds, kind, span: lo.to(self.prev_token.span) })
    }

    fn parse_lifetime_bound(&mut self) -> PResult<'a, GenericBound> {
        if let Some(label) = self.token.lifetime() {
            self.bump();
            let name = label.name.as_str().trim_start_matches('\'');
            self.dcx()
                .struct_span_err(label.span, "lifetimes have no `'` in Vertex")
                .with_span_suggestion(
                    label.span,
                    "remove the leading `'`",
                    name.to_string(),
                    Applicability::MachineApplicable,
                )
                .emit();
            return Ok(GenericBound::Outlives(Lifetime { ident: Ident::new(Symbol::intern(name), label.span) }));
        }
        if self.eat_keyword(kw::Static) {
            let span = self.prev_token.span;
            return Ok(GenericBound::Outlives(Lifetime { ident: Ident::new(kw::Static, span) }));
        }
        let ident = self.parse_ident()?;
        Ok(GenericBound::Outlives(Lifetime { ident: ident }))
    }

    fn can_begin_bound(&self) -> bool {
        self.check_path_segment_ident()
            || self.check(TokenKind::Question)
            || self.check(TokenKind::OpenParen)
            || self.check(TokenKind::OpenBracket)
            || self.check(TokenKind::Lt)
            || self.check_keyword(kw::For)
            || self.check_keyword(kw::Use)
            || self.check_keyword(kw::Static)
            || self.token.lifetime().is_some()
    }

    /// `A + B + a`. An empty list is allowed (`T:`).
    pub fn parse_generic_bounds(&mut self) -> PResult<'a, GenericBounds> {
        let mut bounds = Vec::new();
        loop {
            if !self.can_begin_bound() {
                break;
            }
            bounds.push(self.parse_generic_bound()?);
            if !self.check(TokenKind::Plus) || self.line_ended() {
                break;
            }
            self.bump();
        }
        Ok(bounds)
    }

    pub fn parse_generic_bound(&mut self) -> PResult<'a, GenericBound> {
        let lo = self.token.span;
        if let Some(label) = self.token.lifetime() {
            self.bump();
            let name = label.name.as_str().trim_start_matches('\'');
            self.dcx()
                .struct_span_err(label.span, "lifetimes have no `'` in Vertex")
                .with_span_suggestion(
                    label.span,
                    "remove the leading `'`",
                    name.to_string(),
                    Applicability::MachineApplicable,
                )
                .emit();
            return Ok(GenericBound::Outlives(Lifetime { ident: Ident::new(Symbol::intern(name), label.span) }));
        }
        if self.eat_keyword(kw::Static) {
            let span = self.prev_token.span;
            return Ok(GenericBound::Outlives(Lifetime { ident: Ident::new(kw::Static, span) }));
        }
        if matches!(
            self.token.ident(),
            Some((ident, _))
                if !ident.is_reserved()
                    && !ident.name.is_builtin_name()
                    && ident.name.as_str().starts_with(|c: char| c.is_ascii_lowercase())
        ) && !matches!(
            self.look_ahead(1).kind,
            TokenKind::Dot | TokenKind::Colon | TokenKind::Lt | TokenKind::Shl | TokenKind::OpenParen
        ) {
            let ident = self.parse_ident()?;
            return Ok(GenericBound::Outlives(Lifetime { ident: ident }));
        }
        if self.eat_keyword(kw::Use) {
            // `use<T, src, Self>`: the parameters an `impl Trait` captures.
            if !self.break_and_eat(TokenKind::Lt) {
                return Err(self.expected_found("`<`"));
            }
            let idents = self.with_nl(false, |this| -> PResult<'a, _> {
                let mut idents = Vec::new();
                loop {
                    if this.check_or_compound(TokenKind::Gt) {
                        break;
                    }
                    if this.check_keyword(kw::SelfUpper) {
                        idents.push(Ident::new(kw::SelfUpper, this.token.span));
                        this.bump();
                    } else {
                        idents.push(this.parse_ident()?);
                    }
                    if !this.eat(TokenKind::Comma) {
                        break;
                    }
                }
                this.expect_gt()?;
                Ok(idents)
            })?;
            return Ok(GenericBound::Use(idents, lo.to(self.prev_token.span)));
        }
        let parens = self.eat(TokenKind::OpenParen);
        let maybe = self.eat(TokenKind::Question);
        let params = if self.check_keyword(kw::For) { self.parse_for_lifetimes()? } else { Vec::new() };
        let path = self.parse_path(PathStyle::Type)?;
        if parens {
            self.expect(TokenKind::CloseParen)?;
        }
        Ok(GenericBound::Trait(PolyTraitRef {
            bound_generic_params: params,
            modifiers: TraitBoundModifiers { maybe },
            span: lo.to(self.prev_token.span),
            trait_ref: TraitRef { path, ref_id: DUMMY_NODE_ID },
            parens,
        }))
    }

    /// A trait bound after `for<…>` has already been parsed.
    pub fn parse_generic_bound_after_for(&mut self, lo: Span, params: Vec<GenericParam>) -> PResult<'a, GenericBound> {
        let path = self.parse_path(PathStyle::Type)?;
        Ok(GenericBound::Trait(PolyTraitRef {
            bound_generic_params: params,
            modifiers: TraitBoundModifiers::default(),
            span: lo.to(self.prev_token.span),
            trait_ref: TraitRef { path, ref_id: DUMMY_NODE_ID },
            parens: false,
        }))
    }

    /// `where T: A, U: B`, or nothing.
    pub fn parse_where_clause(&mut self) -> PResult<'a, WhereClause> {
        if !self.check_keyword(kw::Where) {
            return Ok(WhereClause { span: self.prev_token.span.shrink_to_hi(), ..Default::default() });
        }
        let lo = self.token.span;
        self.bump();
        let predicates = self.with_nl(false, |this| -> PResult<'a, _> {
            let mut predicates = Vec::new();
            loop {
                if !(this.token.can_begin_type() || this.check_keyword(kw::For)) || this.check(TokenKind::OpenBrace) {
                    break;
                }
                let plo = this.token.span;
                let bound_generic_params =
                    if this.check_keyword(kw::For) { this.parse_for_lifetimes()? } else { Vec::new() };
                let bounded_ty = this.parse_ty_no_plus()?;
                this.expect(TokenKind::Colon)?;
                let bounds = this.parse_generic_bounds()?;
                predicates.push(WherePredicate {
                    id: DUMMY_NODE_ID,
                    span: plo.to(this.prev_token.span),
                    kind: WherePredicateKind::BoundPredicate { bound_generic_params, bounded_ty, bounds },
                });
                if !this.eat(TokenKind::Comma) {
                    break;
                }
            }
            Ok(predicates)
        })?;
        Ok(WhereClause { has_where_token: true, predicates, span: lo.to(self.prev_token.span) })
    }
}
