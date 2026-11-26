//! Attributes (exception 9: `@name`, not `#[name]`).

use vsc_ast::token::{AttrStyle, TokenKind};
use vsc_ast::{AttrArgs, AttrKind, AttrVec, Attribute, NormalAttr, P, Safety};
use vsc_errors::Applicability;
use vsc_span::kw;

use super::path::PathStyle;
use super::{PResult, Parser};

impl<'a> Parser<'a> {
    /// Outer attributes and outer doc comments.
    pub fn parse_outer_attributes(&mut self) -> PResult<'a, AttrVec> {
        let mut attrs = AttrVec::new();
        loop {
            match self.token.kind {
                TokenKind::At => attrs.push(self.parse_attribute()?),
                TokenKind::DocComment(kind, AttrStyle::Outer, text) => {
                    attrs.push(Attribute { kind: AttrKind::DocComment(kind, text), style: AttrStyle::Outer, span: self.token.span });
                    self.bump();
                }
                TokenKind::DocComment(_, AttrStyle::Inner, _) => {
                    self.dcx()
                        .struct_span_err(self.token.span, "inner doc comments are only allowed at the top of a file")
                        .with_help("use `///` to document the item that follows")
                        .emit();
                    self.bump();
                }
                TokenKind::Pound if self.look_ahead(1).kind == TokenKind::OpenBracket
                    || (self.look_ahead(1).kind == TokenKind::Bang && self.look_ahead(2).kind == TokenKind::OpenBracket) =>
                {
                    attrs.push(self.recover_rust_attribute()?);
                }
                _ => break,
            }
        }
        Ok(attrs)
    }

    /// `@path`, `@path(tokens)`, `@path = expr`, `@unsafe(path …)`.
    fn parse_attribute(&mut self) -> PResult<'a, Attribute> {
        let lo = self.token.span;
        self.bump(); // `@`
        self.parse_attr_item(lo)
    }

    /// An attribute without its `@`: `path`, `path(tokens)`, `path = expr`,
    /// `unsafe(path …)`. Also what `@cfgAttr(pred, …)` holds.
    pub fn parse_attr_item(&mut self, lo: vsc_span::Span) -> PResult<'a, Attribute> {
        let (unsafety, path, args) = if self.check_keyword(kw::Unsafe) && self.look_ahead(1).kind == TokenKind::OpenParen {
            let unsafe_span = self.token.span;
            self.bump();
            self.bump(); // `(`
            let (path, args) = self.with_nl(false, |this| -> PResult<'a, _> {
                let path = this.parse_path(PathStyle::Mod)?;
                let args = this.parse_attr_args()?;
                this.expect(TokenKind::CloseParen)?;
                Ok((path, args))
            })?;
            (Safety::Unsafe(unsafe_span), path, args)
        } else {
            let path = self.parse_path(PathStyle::Mod)?;
            let args = self.parse_attr_args()?;
            (Safety::Default, path, args)
        };
        Ok(Attribute {
            kind: AttrKind::Normal(P::new(NormalAttr { unsafety, path, args })),
            style: AttrStyle::Outer,
            span: lo.to(self.prev_token.span),
        })
    }

    /// An attribute's arguments, which must start on its line.
    fn parse_attr_args(&mut self) -> PResult<'a, AttrArgs> {
        if self.token.line_break_before && self.nl {
            return Ok(AttrArgs::Empty);
        }
        if self.token.kind.open_delim().is_some() {
            return Ok(AttrArgs::Delimited(self.parse_delim_args()?));
        }
        if self.check(TokenKind::Eq) {
            let eq_span = self.token.span;
            self.bump();
            let expr = self.parse_expr()?;
            return Ok(AttrArgs::Eq { eq_span, expr });
        }
        Ok(AttrArgs::Empty)
    }

    /// Rust's `#[name(…)]` or `#![name]`: reported, and read as `@name(…)`.
    fn recover_rust_attribute(&mut self) -> PResult<'a, Attribute> {
        let lo = self.token.span;
        self.bump(); // `#`
        let inner = self.eat(TokenKind::Bang);
        let close = self.group_close_token().map(|t| t.span).unwrap_or(self.token.span);
        let span = lo.to(close);
        let snippet = self.psess.source_map.span_to_snippet(span).unwrap_or_default();
        let body = snippet
            .trim_start_matches('#')
            .trim_start_matches('!')
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let mut err = self.dcx().struct_span_err(span, "attributes are written `@name` in Vertex, not `#[name]`");
        err.span_suggestion(span, "use `@`", format!("@{body}"), Applicability::MaybeIncorrect);
        if inner {
            err.note("attributes for a whole package go on its `package` declaration");
        }
        err.emit();
        self.bump(); // `[`
        let (path, args) = self.with_nl(false, |this| -> PResult<'a, _> {
            let path = this.parse_path(PathStyle::Mod)?;
            let args = this.parse_attr_args()?;
            this.expect(TokenKind::CloseBracket)?;
            Ok((path, args))
        })?;
        Ok(Attribute {
            kind: AttrKind::Normal(P::new(NormalAttr { unsafety: Safety::Default, path, args })),
            style: AttrStyle::Outer,
            span: lo.to(self.prev_token.span),
        })
    }

    /// `//!` comments at the top of a file.
    pub fn parse_inner_doc_comments(&mut self) -> AttrVec {
        let mut attrs = AttrVec::new();
        while let TokenKind::DocComment(kind, AttrStyle::Inner, text) = self.token.kind {
            attrs.push(Attribute { kind: AttrKind::DocComment(kind, text), style: AttrStyle::Inner, span: self.token.span });
            self.bump();
        }
        attrs
    }
}
