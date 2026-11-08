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
    /// In expressions: generic arguments without `::` where they can't be
    /// a comparison (`make<float64>()`, `Vec<int32>.new()`).
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

    /// Rust's `::<…>` at the `::`: reported with the fix (drop the `::`),
    /// and the arguments kept.
    pub(super) fn parse_turbofish(&mut self) -> PResult<'a, AngleBracketedArgs> {
        let colons = self.token.span.to(self.look_ahead(1).span);
        self.bump();
        self.bump();
        let args = self.parse_angle_args()?;
        self.dcx()
            .struct_span_err(colons, "generic arguments in an expression have no `::` in Vertex")
            .with_span_suggestion(colons, "remove the `::`", "", Applicability::MachineApplicable)
            .emit();
        Ok(args)
    }

    /// At `<` after a name in an expression: are these generic arguments
    /// (`make<float64>()`, `Vec<int32>.new()`, `xs.sum<uint64>()`) rather
    /// than less-than? They are when everything up to the matching `>` is
    /// what types are made of, each argument starts the way a type does (a
    /// `PascalCase` name, a built-in type, a package path, `&`, `*`, `[`,
    /// `(`, `func`, `dyn`, `impl`, `_`, a literal or a `{ … }` constant), and
    /// the `>` is followed by `(`, `.`, `{`, `?`, a closing bracket, `,`, `;`
    /// or a line break. This is C#'s rule (Kotlin's and TypeScript's are
    /// alike), narrowed by Vertex's casing: values are `camelCase` and
    /// built-in types are never values, so `a < b` stays a comparison.
    pub(super) fn is_expr_generic_args(&self) -> bool {
        let toks = &self.buf.tokens;
        let mut i = self.pos + 1;
        let mut depth = 1usize;
        let mut nest: Vec<TokenKind> = Vec::new();
        let mut arg_start = true;
        let mut steps = 0;
        let close;
        loop {
            steps += 1;
            if steps > 128 || i >= toks.len() {
                return false;
            }
            let t = &toks[i];
            if arg_start && depth == 1 && nest.is_empty() {
                if !starts_type(t, toks.get(i + 1)) {
                    return false;
                }
                arg_start = false;
                // A constant in braces: skip to its closing brace.
                if t.kind == TokenKind::OpenBrace {
                    let mut braces = 0usize;
                    loop {
                        match toks.get(i).map(|t| &t.kind) {
                            Some(TokenKind::OpenBrace) => braces += 1,
                            Some(TokenKind::CloseBrace) => {
                                braces -= 1;
                                if braces == 0 {
                                    break;
                                }
                            }
                            Some(TokenKind::Eof) | None => return false,
                            _ => {}
                        }
                        i += 1;
                    }
                    i += 1;
                    continue;
                }
            }
            match &t.kind {
                TokenKind::Lt => depth += 1,
                TokenKind::Shl => depth += 2,
                TokenKind::Gt => {
                    depth -= 1;
                    if depth == 0 {
                        close = i;
                        break;
                    }
                }
                TokenKind::Shr => {
                    if depth < 2 {
                        return false;
                    }
                    depth -= 2;
                    if depth == 0 {
                        close = i;
                        break;
                    }
                }
                TokenKind::Comma => {
                    if depth == 1 && nest.is_empty() {
                        arg_start = true;
                    }
                }
                TokenKind::OpenParen | TokenKind::OpenBracket => nest.push(t.kind),
                TokenKind::CloseParen => {
                    if nest.pop() != Some(TokenKind::OpenParen) {
                        return false;
                    }
                }
                TokenKind::CloseBracket => {
                    if nest.pop() != Some(TokenKind::OpenBracket) {
                        return false;
                    }
                }
                TokenKind::Ident(name, _) => {
                    let ok_keyword = [kw::Mut, kw::Const, kw::Dyn, kw::Impl, kw::Func, kw::As, kw::SelfUpper, kw::Underscore, kw::For, kw::Unsafe, kw::Extern];
                    if !t.is_non_keyword_ident() && !ok_keyword.contains(name) {
                        return false;
                    }
                }
                TokenKind::Literal(_)
                | TokenKind::And
                | TokenKind::AndAnd
                | TokenKind::Star
                | TokenKind::Dot
                | TokenKind::RArrow
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Eq => {}
                TokenKind::Semi if nest.last() == Some(&TokenKind::OpenBracket) => {}
                _ => return false,
            }
            i += 1;
        }
        let next = toks.get(close + 1);
        match next {
            None => true,
            Some(n) => {
                n.line_break_before
                    || matches!(
                        n.kind,
                        TokenKind::OpenParen
                            | TokenKind::Dot
                            | TokenKind::OpenBrace
                            | TokenKind::Question
                            | TokenKind::CloseParen
                            | TokenKind::CloseBracket
                            | TokenKind::CloseBrace
                            | TokenKind::Comma
                            | TokenKind::Semi
                            | TokenKind::Eof
                    )
            }
        }
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
                    // Rust's turbofish: `parse::<int64>()`.
                    let args = self.parse_turbofish()?;
                    if let Some(last) = segments.last_mut() {
                        last.args = Some(P::new(GenericArgs::AngleBracketed(args)));
                    }
                }
            }
            return Ok(());
        }
    }

    fn parse_path_segment(&mut self, style: PathStyle) -> PResult<'a, PathSegment> {
        let ident = self.parse_path_segment_ident()?;
        let mut seg = PathSegment::from_ident(ident);
        if style == PathStyle::Expr && !self.token.line_break_before && self.check(TokenKind::Lt) && self.is_expr_generic_args() {
            let args = self.parse_angle_args()?;
            seg.args = Some(P::new(GenericArgs::AngleBracketed(args)));
        } else if style == PathStyle::Type && !self.token.line_break_before {
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
                .struct_span_err(label.span, "lifetimes have no `'` in Vertex")
                .with_span_suggestion(
                    label.span,
                    "remove the leading `'`",
                    name.to_string(),
                    Applicability::MachineApplicable,
                )
                .emit();
            let ident = Ident::new(vsc_span::Symbol::intern(name), label.span);
            return Ok(GenericArg::Lifetime(vsc_ast::Lifetime { ident: ident }));
        }
        if self.eat_keyword(kw::Static) {
            let span = self.prev_token.span;
            return Ok(GenericArg::Lifetime(vsc_ast::Lifetime { ident: Ident::new(kw::Static, span) }));
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

/// Can `t` (followed by `next`) start a generic argument in an expression?
fn starts_type(t: &vsc_ast::token::Token, next: Option<&vsc_ast::token::Token>) -> bool {
    use vsc_span::sym;
    match &t.kind {
        TokenKind::Ident(name, _) => {
            let s = name.as_str();
            if [kw::SelfUpper, kw::Func, kw::Dyn, kw::Impl, kw::Underscore, kw::For, kw::Unsafe, kw::Extern].contains(name) {
                return true;
            }
            if !t.is_non_keyword_ident() {
                return false;
            }
            let builtin_ty = [
                sym::int, sym::int8, sym::int16, sym::int32, sym::int64, sym::int128, sym::uint, sym::uint8, sym::uint16,
                sym::uint32, sym::uint64, sym::uint128, sym::float32, sym::float64, sym::bool, sym::char, sym::string,
                sym::never,
            ];
            s.starts_with(|c: char| c.is_ascii_uppercase())
                || builtin_ty.contains(name)
                || name.is_builtin_name()
                // A type in a package: `collections.HashMap<K, V>`.
                || next.is_some_and(|n| n.kind == TokenKind::Dot && n.span.lo() == t.span.hi())
        }
        TokenKind::Literal(_)
        | TokenKind::And
        | TokenKind::AndAnd
        | TokenKind::Star
        | TokenKind::OpenBracket
        | TokenKind::OpenParen
        | TokenKind::OpenBrace
        | TokenKind::Minus
        | TokenKind::Lt => true,
        _ => false,
    }
}
