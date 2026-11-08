//! Paths (exception 7: `.` separates segments, never `::`).

use vsc_ast::token::{IdentIsRaw, TokenKind};
use vsc_ast::{
    AngleBracketedArg, AngleBracketedArgs, AnonConst, AssocItemConstraint, AssocItemConstraintKind, DUMMY_NODE_ID,
    Expr, ExprKind, FnRetTy, GenericArg, GenericArgs, P, ParenthesizedArgs, Path, PathSegment, QSelf,
};
use vsc_errors::Applicability;
use vsc_span::{Ident, kw};

use super::{PResult, Parser};

/// What a path is for, which decides what its segments may carry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PathStyle {
    /// In expressions: no generic arguments (there is no turbofish).
    Expr,
    /// In patterns: like expressions.
    Pat,
    /// In types: segments may have `<…>` or `(…) -> T`.
    Type,
    /// Attribute names and other simple paths: identifiers only.
    Mod,
}

impl<'a> Parser<'a> {
    pub fn parse_path(&mut self, style: PathStyle) -> PResult<'a, Path> {
        let lo = self.token.span;
        let mut segments = Vec::new();
        self.parse_path_segments(&mut segments, style)?;
        Ok(Path { span: lo.to(self.prev_token.span), segments })
    }

    /// `<T as Trait>.Rest`: returns the qualified self type and the path,
    /// which starts with the trait's segments.
    pub fn parse_qpath(&mut self, style: PathStyle) -> PResult<'a, (P<QSelf>, Path)> {
        let lo = self.token.span;
        if !self.break_and_eat(TokenKind::Lt) {
            return Err(self.expected_found("`<`"));
        }
        let (ty, trait_path) = self.with_nl(false, |this| -> PResult<'a, _> {
            let ty = this.parse_ty()?;
            let trait_path = if this.eat_keyword(kw::As) { Some(this.parse_path(PathStyle::Type)?) } else { None };
            this.expect_gt()?;
            Ok((ty, trait_path))
        })?;
        let mut segments = Vec::new();
        let position = match trait_path {
            Some(p) => {
                let n = p.segments.len();
                segments = p.segments;
                n
            }
            None => 0,
        };
        let path_lo = self.prev_token.span;
        if !self.eat_path_sep() {
            return Err(self.expected_found("`.` after a qualified type"));
        }
        self.parse_path_segments(&mut segments, style)?;
        let qself = P::new(QSelf { ty, path_span: path_lo.to(self.prev_token.span), position });
        Ok((qself, Path { span: lo.to(self.prev_token.span), segments }))
    }

    /// Can the current token start a path segment?
    pub fn check_path_segment_ident(&self) -> bool {
        self.token.is_non_keyword_ident() || self.token.is_path_segment_keyword()
    }

    /// Is the current `.` a path separator, followed by another segment?
    fn check_path_sep(&self) -> bool {
        if !self.check(TokenKind::Dot) {
            return false;
        }
        let next = self.look_ahead(1);
        next.is_non_keyword_ident() || next.is_path_segment_keyword()
    }

    /// Rust's `::` between segments: reported, and read as `.`.
    fn check_rust_path_sep(&self) -> bool {
        self.check(TokenKind::Colon)
            && self.look_ahead(1).kind == TokenKind::Colon
            && self.look_ahead(1).span.lo() == self.token.span.hi()
    }

    fn eat_path_sep(&mut self) -> bool {
        if self.check(TokenKind::Dot) {
            self.bump();
            return true;
        }
        if self.check_rust_path_sep() {
            self.report_rust_path_sep();
            self.bump();
            self.bump();
            return true;
        }
        false
    }

    fn report_rust_path_sep(&self) {
        let span = self.token.span.to(self.look_ahead(1).span);
        self.dcx()
            .struct_span_err(span, "paths are separated by `.` in Vertex, not `::`")
            .with_span_suggestion(span, "use `.`", ".", Applicability::MachineApplicable)
            .emit();
    }

    fn parse_path_segments(&mut self, segments: &mut Vec<PathSegment>, style: PathStyle) -> PResult<'a, ()> {
        loop {
            let seg = self.parse_path_segment(style)?;
            segments.push(seg);
            if self.check_path_sep() {
                self.bump();
                continue;
            }
            if self.check_rust_path_sep() {
                let next = self.look_ahead(2);
                if next.is_non_keyword_ident() || next.is_path_segment_keyword() {
                    self.report_rust_path_sep();
                    self.bump();
                    self.bump();
                    continue;
                }
                if next.kind == TokenKind::Lt && matches!(style, PathStyle::Expr | PathStyle::Pat) {
                    // A turbofish: `parse::<int64>()`.
                    let span = self.token.span;
                    self.bump();
                    self.bump();
                    let args = self.parse_angle_args()?;
                    self.dcx()
                        .struct_span_err(span.to(args.span), "Vertex has no turbofish")
                        .with_help("give the type with an annotation instead: `let n: int64 = text.Parse()`")
                        .emit();
                }
            }
            return Ok(());
        }
    }

    fn parse_path_segment(&mut self, style: PathStyle) -> PResult<'a, PathSegment> {
        let ident = self.parse_path_segment_ident()?;
        let mut seg = PathSegment::from_ident(ident);
        if style == PathStyle::Type && !self.token.line_break_before {
            if self.check(TokenKind::Lt) || self.check(TokenKind::Shl) {
                let args = self.parse_angle_args()?;
                seg.args = Some(P::new(GenericArgs::AngleBracketed(args)));
            } else if self.check(TokenKind::OpenParen) && is_fn_trait_sugar_candidate(ident) {
                seg.args = Some(P::new(GenericArgs::Parenthesized(self.parse_parenthesized_args()?)));
            }
        }
        Ok(seg)
    }

    pub fn parse_path_segment_ident(&mut self) -> PResult<'a, Ident> {
        match self.token.ident() {
            Some((ident, IdentIsRaw::No)) if ident.name.is_path_segment_keyword() => {
                self.bump();
                Ok(ident)
            }
            _ => self.parse_ident(),
        }
    }

    pub fn parse_angle_args(&mut self) -> PResult<'a, AngleBracketedArgs> {
        let lo = self.token.span;
        if !self.break_and_eat(TokenKind::Lt) {
            return Err(self.expected_found("`<`"));
        }
        let args = self.with_nl(false, |this| -> PResult<'a, _> {
            let mut args = Vec::new();
            loop {
                if this.check_or_compound(TokenKind::Gt) {
                    break;
                }
                args.push(this.parse_angle_arg()?);
                if !this.eat(TokenKind::Comma) {
                    break;
                }
            }
            this.expect_gt()?;
            Ok(args)
        })?;
        Ok(AngleBracketedArgs { span: lo.to(self.prev_token.span), args })
    }

    fn parse_angle_arg(&mut self) -> PResult<'a, AngleBracketedArg> {
        // `Item = T` and `Item: Bound`.
        if self.check_ident() {
            let next = self.look_ahead(1).kind;
            let is_constraint = matches!(next, TokenKind::Eq)
                || (next == TokenKind::Colon && self.look_ahead(2).kind != TokenKind::Colon);
            if is_constraint {
                let lo = self.token.span;
                let ident = self.parse_ident()?;
                let kind = if self.eat(TokenKind::Eq) {
                    AssocItemConstraintKind::Equality { term: self.parse_ty()? }
                } else {
                    self.expect(TokenKind::Colon)?;
                    AssocItemConstraintKind::Bound { bounds: self.parse_generic_bounds()? }
                };
                return Ok(AngleBracketedArg::Constraint(AssocItemConstraint {
                    id: DUMMY_NODE_ID,
                    ident,
                    gen_args: None,
                    kind,
                    span: lo.to(self.prev_token.span),
                }));
            }
        }
        Ok(AngleBracketedArg::Arg(self.parse_generic_arg()?))
    }

    pub fn parse_generic_arg(&mut self) -> PResult<'a, GenericArg> {
        if let Some(label) = self.token.lifetime() {
            self.bump();
            let name = label.name.as_str().trim_start_matches('\'');
            self.dcx()
                .struct_span_err(label.span, "Vertex has no lifetimes")
                .with_span_suggestion(
                    label.span,
                    "name the origin instead",
                    format!("[{name}]"),
                    Applicability::MaybeIncorrect,
                )
                .emit();
            let ident = Ident::new(vsc_span::Symbol::intern(name), label.span);
            return Ok(GenericArg::Origin(vsc_ast::OriginRef { origins: vec![ident], span: label.span }));
        }
        if self.check(TokenKind::OpenBracket) && self.origin_list_is_unambiguous() {
            return Ok(GenericArg::Origin(self.parse_origin_ref()?));
        }
        if self.token.can_begin_literal_maybe_minus() || self.check(TokenKind::OpenBrace) {
            let value = if self.check(TokenKind::OpenBrace) {
                let block = self.parse_block()?;
                let span = block.span;
                P::new(Expr { id: DUMMY_NODE_ID, kind: ExprKind::Block(block, None), span, attrs: vec![] })
            } else {
                self.parse_literal_maybe_minus()?
            };
            return Ok(GenericArg::Const(AnonConst { id: DUMMY_NODE_ID, value }));
        }
        Ok(GenericArg::Type(self.parse_ty()?))
    }

    /// Is the `[…]` at the current token certainly an origin list? It is
    /// when it holds `static` or several names. A single name (`[doc]`) could
    /// be a slice type, so it parses as one and name resolution decides.
    fn origin_list_is_unambiguous(&self) -> bool {
        let Some(close) = self.group_close() else { return false };
        let inner = &self.buf.tokens[self.pos + 1..close];
        let mut names = 0;
        let mut has_static = false;
        for (i, tok) in inner.iter().enumerate() {
            if i % 2 == 0 {
                if tok.is_keyword(kw::Static) {
                    has_static = true;
                } else if !tok.is_non_keyword_ident() {
                    return false;
                }
                names += 1;
            } else if tok.kind != TokenKind::Comma {
                return false;
            }
        }
        names > 0 && (has_static || names > 1)
    }

    /// `(A, B) -> C` after a path segment such as `Fn`.
    fn parse_parenthesized_args(&mut self) -> PResult<'a, ParenthesizedArgs> {
        let lo = self.token.span;
        let (inputs, _) = self.parse_delim_comma_seq(vsc_ast::token::Delimiter::Parenthesis, |p| p.parse_ty())?;
        let inputs_span = lo.to(self.prev_token.span);
        let output = if self.eat(TokenKind::RArrow) {
            FnRetTy::Ty(self.parse_ty_no_plus()?)
        } else {
            FnRetTy::Default(self.prev_token.span.shrink_to_hi())
        };
        Ok(ParenthesizedArgs { span: lo.to(self.prev_token.span), inputs, inputs_span, output })
    }
}

/// `Fn(T) -> U` sugar applies to Rust's function traits; other `Name(`s in a
/// type position are parse errors anyway, so treat any capitalized name as a
/// candidate and let later passes reject what isn't a function trait.
fn is_fn_trait_sugar_candidate(ident: Ident) -> bool {
    ident.name.as_str().starts_with(|c: char| c.is_ascii_uppercase())
}
