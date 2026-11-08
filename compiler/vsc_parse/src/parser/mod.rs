//! The recursive-descent parser (rustc's `rustc_parse::parser`).
//!
//! The parser walks a flat, balanced token buffer. Each token records whether
//! a line feed came before it; `nl` says whether line feeds end things in the
//! current context (exception 10, rule 1): yes in blocks, item lists, field
//! lists and match arms, no inside `( )`, `[ ]`, `< >` and comma lists.

mod asm;
mod attr;
mod diagnostics;
mod expr;
mod generics;
mod item;
mod package;
mod pat;
mod path;
mod stmt;
mod ty;

pub use path::PathStyle;

use vsc_ast::token::{Delimiter, IdentIsRaw, Lit, LitKind, Token, TokenKind};
use vsc_ast::tokenstream::{DelimSpan, TokenTree};
use vsc_ast::{DelimArgs, StrLit};
use vsc_errors::{Applicability, Diag};
use vsc_session::ParseSess;
use vsc_span::{BytePos, Ident, Span, Symbol, kw};

use crate::lexer::{TokenBuffer, continues_line};

pub type PResult<'a, T> = Result<T, Diag<'a>>;

/// What the expression parser may and may not do here.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Restrictions(u8);

impl Restrictions {
    pub const NONE: Restrictions = Restrictions(0);
    /// At the start of a statement: a block-like expression ends there.
    pub const STMT_EXPR: Restrictions = Restrictions(1);
    /// In a condition or scrutinee: `x {` starts the body, not a struct literal.
    pub const NO_STRUCT_LITERAL: Restrictions = Restrictions(2);
    /// In an `if`/`while` condition or a match guard: `let` is allowed.
    pub const ALLOW_LET: Restrictions = Restrictions(4);

    pub fn contains(self, other: Restrictions) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn union(self, other: Restrictions) -> Restrictions {
        Restrictions(self.0 | other.0)
    }

    pub fn without(self, other: Restrictions) -> Restrictions {
        Restrictions(self.0 & !other.0)
    }
}

pub struct Parser<'a> {
    pub psess: &'a ParseSess,
    buf: TokenBuffer,
    /// Index of `token` in `buf`.
    pos: usize,
    /// The current token. It differs from `buf.tokens[pos]` after a compound
    /// token has been split (`>>` read as `>` `>`).
    pub token: Token,
    pub prev_token: Token,
    /// Whether line feeds end things here (exception 10, rule 1).
    nl: bool,
    restrictions: Restrictions,
    /// Labels declared so far in the file, for disambiguating `break label`.
    pub(super) labels: Vec<Symbol>,
    /// Whether the parser is inside a `for` or `while` loop body.
    pub(super) in_for_or_while: bool,
}

impl<'a> Parser<'a> {
    pub(crate) fn new(psess: &'a ParseSess, buf: TokenBuffer) -> Parser<'a> {
        let token = buf.tokens[0];
        Parser {
            psess,
            buf,
            pos: 0,
            token,
            prev_token: Token::dummy(),
            nl: true,
            restrictions: Restrictions::NONE,
            labels: Vec::new(),
            in_for_or_while: false,
        }
    }

    pub fn dcx(&self) -> &'a vsc_errors::DiagCtxt {
        &self.psess.dcx
    }

    // -----------------------------------------------------------------------
    // Moving through tokens
    // -----------------------------------------------------------------------

    pub fn bump(&mut self) {
        self.prev_token = self.token;
        if self.pos + 1 < self.buf.tokens.len() {
            self.pos += 1;
        }
        self.token = self.buf.tokens[self.pos];
    }

    /// The token `dist` positions ahead (0 is the current one, unsplit).
    pub fn look_ahead(&self, dist: usize) -> &Token {
        if dist == 0 {
            return &self.token;
        }
        let i = (self.pos + dist).min(self.buf.tokens.len() - 1);
        &self.buf.tokens[i]
    }

    /// If the current token opens a group, the index of its closer.
    fn group_close(&self) -> Option<usize> {
        if self.token.kind.open_delim().is_some() && self.token == self.buf.tokens[self.pos] {
            self.buf.partner(self.pos)
        } else {
            None
        }
    }

    /// The token after the group the current token opens.
    pub fn token_after_group(&self) -> Option<&Token> {
        let close = self.group_close()?;
        Some(&self.buf.tokens[(close + 1).min(self.buf.tokens.len() - 1)])
    }

    /// The closing token of the group the current token opens.
    pub fn group_close_token(&self) -> Option<&Token> {
        self.group_close().map(|i| &self.buf.tokens[i])
    }

    /// Steps over a whole group, or one token.
    pub fn skip_group_or_token(&mut self) {
        if let Some(close) = self.group_close() {
            self.pos = close;
            self.token = self.buf.tokens[close];
        }
        self.bump();
    }

    /// Moves to the closer of the innermost group that contains the current
    /// token, if it is the group opened at `open_index`.
    fn jump_to_close_of(&mut self, open_index: usize) {
        if let Some(close) = self.buf.partner(open_index) {
            if close >= self.pos {
                self.pos = close;
                self.token = self.buf.tokens[close];
            }
        }
    }

    pub fn check(&self, kind: TokenKind) -> bool {
        self.token.kind == kind
    }

    pub fn eat(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub fn check_keyword(&self, kw: Symbol) -> bool {
        self.token.is_keyword(kw)
    }

    pub fn eat_keyword(&mut self, kw: Symbol) -> bool {
        if self.check_keyword(kw) {
            self.bump();
            true
        } else {
            false
        }
    }

    pub fn expect(&mut self, kind: TokenKind) -> PResult<'a, ()> {
        if self.eat(kind) {
            Ok(())
        } else {
            Err(self.expected_found(&format!("`{}`", kind.as_string())))
        }
    }

    pub fn expect_keyword(&mut self, kw: Symbol) -> PResult<'a, ()> {
        if self.eat_keyword(kw) {
            Ok(())
        } else {
            Err(self.expected_found(&format!("`{kw}`")))
        }
    }

    /// Eats `expected`, splitting a compound token that starts with it:
    /// `>>` as `>` `>`, `&&` as `&` `&`.
    pub fn break_and_eat(&mut self, expected: TokenKind) -> bool {
        if self.check(expected) {
            self.bump();
            return true;
        }
        use TokenKind::*;
        let rest = match (self.token.kind, expected) {
            (Shr, Gt) => Gt,
            (Ge, Gt) => Eq,
            (ShrEq, Gt) => Ge,
            (AndAnd, And) => And,
            (OrOr, Or) => Or,
            (Shl, Lt) => Lt,
            (Le, Lt) => Eq,
            (DotDot, Dot) => Dot,
            _ => return false,
        };
        let lo = self.token.span.lo();
        let mid = BytePos(lo.0 + 1);
        self.prev_token = Token { kind: expected, span: Span::new(lo, mid), line_break_before: self.token.line_break_before };
        self.token = Token { kind: rest, span: Span::new(mid, self.token.span.hi()), line_break_before: false };
        true
    }

    /// Is the current token `expected`, or a compound token starting with it?
    pub fn check_or_compound(&self, expected: TokenKind) -> bool {
        use TokenKind::*;
        self.check(expected)
            || matches!(
                (self.token.kind, expected),
                (Shr | Ge | ShrEq, Gt) | (AndAnd, And) | (OrOr, Or) | (Shl | Le, Lt)
            )
    }

    pub fn expect_gt(&mut self) -> PResult<'a, ()> {
        if self.break_and_eat(TokenKind::Gt) { Ok(()) } else { Err(self.expected_found("`>`")) }
    }

    // -----------------------------------------------------------------------
    // Line breaks (exception 10)
    // -----------------------------------------------------------------------

    /// Is there a line feed before the current token, in a context where line
    /// feeds end things?
    pub fn at_line_break(&self) -> bool {
        self.nl && self.token.line_break_before
    }

    /// Has the previous line ended here? True at a line break unless the
    /// current token continues the line (rule 2).
    pub fn line_ended(&self) -> bool {
        self.at_line_break() && !continues_line(&self.token, self.look_ahead(1))
    }

    /// Runs `f` with line feeds significant (`true`) or not (`false`).
    pub fn with_nl<T>(&mut self, nl: bool, f: impl FnOnce(&mut Self) -> T) -> T {
        let old = std::mem::replace(&mut self.nl, nl);
        let r = f(self);
        self.nl = old;
        r
    }

    pub fn with_res<T>(&mut self, r: Restrictions, f: impl FnOnce(&mut Self) -> T) -> T {
        let old = std::mem::replace(&mut self.restrictions, r);
        let res = f(self);
        self.restrictions = old;
        res
    }

    /// Is the current token the end of a statement, declaration, field,
    /// variant or arm? `;`, a line break, a closing `}`, or the end of input.
    /// After something that ends with `}`, nothing is needed (`Separation`).
    pub fn at_terminator(&self) -> bool {
        self.check(TokenKind::Semi)
            || self.check(TokenKind::CloseBrace)
            || self.check(TokenKind::Eof)
            || self.token.line_break_before
            || self.prev_token.kind == TokenKind::CloseBrace
    }

    /// Requires a terminator after a statement, field, variant or arm, and
    /// eats any `;`s.
    pub fn expect_terminator(&mut self, what: &str) -> PResult<'a, ()> {
        if self.at_terminator() {
            while self.eat(TokenKind::Semi) {}
            return Ok(());
        }
        let prev_end = self.prev_token.span.shrink_to_hi();
        let mut err = self.dcx().struct_span_err(
            self.token.span,
            format!("expected `;` or a line break, found {}", self.token.to_descr()),
        );
        err.span_label(self.token.span, "unexpected token");
        err.note(format!("{what} are separated by line breaks, or by `;` on one line"));
        err.span_suggestion(prev_end, "add `;` here", ";", Applicability::MaybeIncorrect);
        Err(err)
    }

    // -----------------------------------------------------------------------
    // Identifiers and literals
    // -----------------------------------------------------------------------

    pub fn parse_ident(&mut self) -> PResult<'a, Ident> {
        match self.token.ident() {
            Some((ident, IdentIsRaw::Yes)) => {
                self.bump();
                Ok(ident)
            }
            Some((ident, IdentIsRaw::No)) if !ident.is_reserved() => {
                self.bump();
                Ok(ident)
            }
            Some((ident, IdentIsRaw::No)) => {
                let mut err = self.expected_found("identifier");
                if let Some(hint) = diagnostics::rust_keyword_hint(ident.name) {
                    err.help(hint);
                } else {
                    err.span_suggestion(
                        ident.span,
                        "escape the keyword to use it as an identifier",
                        format!("r#{}", ident.name),
                        Applicability::MaybeIncorrect,
                    );
                }
                Err(err)
            }
            None => Err(self.expected_found("identifier")),
        }
    }

    /// An identifier, or `_` when `allow_underscore`.
    pub fn parse_ident_or_underscore(&mut self) -> PResult<'a, Ident> {
        if self.check_keyword(kw::Underscore) {
            let span = self.token.span;
            self.bump();
            return Ok(Ident::new(kw::Underscore, span));
        }
        self.parse_ident()
    }

    pub fn check_ident(&self) -> bool {
        self.token.is_non_keyword_ident()
    }

    /// A string literal, such as an ABI name or import path.
    pub fn parse_str_lit(&mut self) -> Option<StrLit> {
        match self.token.kind {
            TokenKind::Literal(Lit { kind: LitKind::Str | LitKind::StrRaw(_), symbol, suffix: None }) => {
                let span = self.token.span;
                self.bump();
                Some(StrLit { symbol, span })
            }
            _ => None,
        }
    }

    pub fn check_str_lit(&self) -> bool {
        matches!(self.token.kind, TokenKind::Literal(Lit { kind: LitKind::Str | LitKind::StrRaw(_), .. }))
    }

    // -----------------------------------------------------------------------
    // Lists and token trees
    // -----------------------------------------------------------------------

    /// `open item, item, … close`, with line feeds as whitespace. An error in
    /// an element is reported, and parsing resumes after the group.
    pub fn parse_delim_comma_seq<T>(
        &mut self,
        delim: Delimiter,
        mut f: impl FnMut(&mut Self) -> PResult<'a, T>,
    ) -> PResult<'a, (Vec<T>, bool)> {
        let open = TokenKind::open_for(delim);
        let close = TokenKind::close_for(delim);
        if !self.check(open) {
            return Err(self.expected_found(&format!("`{}`", open.as_string())));
        }
        let open_index = self.pos;
        self.bump();
        self.with_nl(false, |this| {
            let mut items = Vec::new();
            let mut trailing = false;
            loop {
                if this.check(close) {
                    break;
                }
                match f(this) {
                    Ok(item) => items.push(item),
                    Err(err) => {
                        err.emit();
                        this.jump_to_close_of(open_index);
                        break;
                    }
                }
                if this.eat(TokenKind::Comma) {
                    trailing = true;
                    continue;
                }
                trailing = false;
                if this.check(close) {
                    break;
                }
                let err = this.expected_found(&format!("`,` or `{}`", close.as_string()));
                err.emit();
                this.jump_to_close_of(open_index);
                break;
            }
            this.expect(close)?;
            Ok((items, trailing))
        })
    }

    /// A delimited group as token trees: `(…)`, `[…]` or `{…}`.
    pub fn parse_delim_args(&mut self) -> PResult<'a, DelimArgs> {
        let Some(delim) = self.token.kind.open_delim() else {
            return Err(self.expected_found("`(`, `[` or `{`"));
        };
        match self.parse_token_tree() {
            TokenTree::Delimited(dspan, _, tokens) => Ok(DelimArgs { dspan, delim, tokens }),
            TokenTree::Token(_) => unreachable!(),
        }
    }

    pub fn parse_token_tree(&mut self) -> TokenTree {
        if let Some(delim) = self.token.kind.open_delim() {
            let open_span = self.token.span;
            let close = self.group_close().expect("balanced buffer");
            self.bump();
            let mut inner = Vec::new();
            while self.pos < close {
                inner.push(self.parse_token_tree());
            }
            let close_span = self.token.span;
            self.bump();
            TokenTree::Delimited(DelimSpan { open: open_span, close: close_span }, delim, inner)
        } else {
            let tok = self.token;
            self.bump();
            TokenTree::Token(tok)
        }
    }

    // -----------------------------------------------------------------------
    // Recovery
    // -----------------------------------------------------------------------

    /// After an error in a statement: skips to the next line, `;` or `}`.
    pub fn recover_stmt(&mut self) {
        let start = self.pos;
        loop {
            match self.token.kind {
                TokenKind::Eof | TokenKind::CloseBrace => break,
                TokenKind::Semi => {
                    self.bump();
                    break;
                }
                _ => {}
            }
            if self.pos != start && self.token.line_break_before {
                break;
            }
            self.skip_group_or_token();
        }
    }

    /// After an error in an item: skips to a line that starts an item.
    pub fn recover_item(&mut self) {
        let start = self.pos;
        loop {
            match self.token.kind {
                TokenKind::Eof | TokenKind::CloseBrace => break,
                _ => {}
            }
            if self.pos != start && self.token.line_break_before && self.is_item_start_token() {
                break;
            }
            self.skip_group_or_token();
        }
    }

    fn is_item_start_token(&self) -> bool {
        self.check(TokenKind::At)
            || matches!(self.token.kind, TokenKind::DocComment(..))
            || [
                kw::Func,
                kw::Struct,
                kw::Class,
                kw::Enum,
                kw::Trait,
                kw::Implement,
                kw::Type,
                kw::Const,
                kw::Static,
                kw::Extern,
                kw::Unsafe,
                kw::Public,
                kw::Package,
                kw::Async,
                kw::Import,
                kw::Union,
            ]
            .iter()
            .any(|&k| self.check_keyword(k))
    }
}
