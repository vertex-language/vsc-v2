//! Cooking raw tokens (rustc's `rustc_parse::lexer`).
//!
//! `vsc_lexer` gives kinds and lengths. This turns them into the parser's
//! tokens: punctuation glued into operators, names interned, literals checked,
//! trivia dropped, and each token marked with whether a line feed came before
//! it (exception 10). Errors in the text are reported here, and lexing goes on
//! so the parser sees as much of the file as possible.

mod line_breaks;
mod tokentrees;
mod unescape_error_reporting;
mod unicode_chars;

pub(crate) use line_breaks::continues_line;
pub(crate) use tokentrees::TokenBuffer;

/// Balances an already-cooked token list (for re-parsing token trees).
pub(crate) fn balance_tokens(psess: &ParseSess, tokens: Vec<Token>) -> TokenBuffer {
    tokentrees::balance(psess, tokens)
}

use vsc_ast::token::{AttrStyle, CommentKind, IdentIsRaw, Lit, LitKind, Token, TokenKind};
use vsc_errors::Applicability;
use vsc_lexer::unescape::{self, Mode};
use vsc_lexer::{Base, Cursor, DocStyle, LiteralKind, RawStrError};
use vsc_session::ParseSess;
use vsc_span::{BytePos, Span, Symbol};

/// Lexes `src`, which starts at `start_pos` in the source map, into a balanced
/// token buffer ending with `Eof`.
pub(crate) fn lex_token_trees(psess: &ParseSess, src: &str, start_pos: BytePos) -> TokenBuffer {
    let mut reader = StringReader::new(psess, src, start_pos);
    let mut tokens = Vec::new();
    loop {
        let tok = reader.next_token();
        let eof = tok.kind == TokenKind::Eof;
        tokens.push(tok);
        if eof {
            break;
        }
    }
    tokentrees::balance(psess, tokens)
}

struct StringReader<'psess, 'src> {
    psess: &'psess ParseSess,
    /// Where `src` starts in the source map.
    start_pos: BytePos,
    /// Where the next raw token starts.
    pos: BytePos,
    src: &'src str,
    cursor: Cursor<'src>,
    /// A raw token read ahead while gluing punctuation.
    peeked: Option<(vsc_lexer::Token, BytePos)>,
}

impl<'psess, 'src> StringReader<'psess, 'src> {
    fn new(psess: &'psess ParseSess, src: &'src str, start_pos: BytePos) -> Self {
        let shebang_len = vsc_lexer::strip_shebang(src).unwrap_or(0);
        let rest = &src[shebang_len..];
        StringReader {
            psess,
            start_pos,
            pos: BytePos(start_pos.0 + shebang_len as u32),
            src,
            cursor: Cursor::new(rest),
            peeked: None,
        }
    }

    fn dcx(&self) -> &'psess vsc_errors::DiagCtxt {
        &self.psess.dcx
    }

    fn mk_sp(&self, lo: BytePos, hi: BytePos) -> Span {
        Span::new(lo, hi)
    }

    fn str_from_to(&self, lo: BytePos, hi: BytePos) -> &'src str {
        &self.src[(lo.0 - self.start_pos.0) as usize..(hi.0 - self.start_pos.0) as usize]
    }

    fn str_from(&self, lo: BytePos) -> &'src str {
        self.str_from_to(lo, self.pos)
    }

    /// The next raw token and where it starts.
    fn next_raw(&mut self) -> (vsc_lexer::Token, BytePos) {
        if let Some(peeked) = self.peeked.take() {
            return peeked;
        }
        let start = self.pos;
        let tok = self.cursor.advance_token();
        self.pos = BytePos(self.pos.0 + tok.len);
        (tok, start)
    }

    fn peek_raw(&mut self) -> vsc_lexer::TokenKind {
        if self.peeked.is_none() {
            let start = self.pos;
            let tok = self.cursor.advance_token();
            self.pos = BytePos(self.pos.0 + tok.len);
            self.peeked = Some((tok, start));
        }
        self.peeked.unwrap().0.kind
    }

    /// The next cooked token.
    fn next_token(&mut self) -> Token {
        let mut line_break_before = false;
        loop {
            let (raw, start) = self.next_raw();
            // With a peeked token pending, `self.pos` is past it; the current
            // token ends at `start + len`.
            let end = BytePos(start.0 + raw.len);
            let kind = match raw.kind {
                vsc_lexer::TokenKind::Whitespace => {
                    if self.str_from_to(start, end).contains('\n') {
                        line_break_before = true;
                    }
                    continue;
                }
                vsc_lexer::TokenKind::LineComment { doc_style } => match doc_style {
                    None => continue,
                    Some(style) => {
                        let content = &self.str_from_to(start, end)[3..];
                        self.doc_comment(CommentKind::Line, style, content)
                    }
                },
                vsc_lexer::TokenKind::BlockComment { doc_style, terminated } => {
                    let text = self.str_from_to(start, end);
                    if !terminated {
                        let msg = if doc_style.is_some() {
                            "unterminated block doc-comment"
                        } else {
                            "unterminated block comment"
                        };
                        self.dcx()
                            .struct_span_err(self.mk_sp(start, BytePos(start.0 + 2)), msg)
                            .with_note("block comments nest: every `/*` needs its own `*/`")
                            .emit();
                    }
                    // A block comment with a line feed in it counts as one (Go's rule).
                    if text.contains('\n') {
                        line_break_before = true;
                    }
                    match doc_style {
                        None => continue,
                        Some(style) => {
                            let close = if terminated { 2 } else { 0 };
                            let content = &text[3..text.len() - close];
                            self.doc_comment(CommentKind::Block, style, content)
                        }
                    }
                }
                vsc_lexer::TokenKind::Ident => {
                    let name = self.str_from_to(start, end);
                    TokenKind::Ident(Symbol::intern(name), IdentIsRaw::No)
                }
                vsc_lexer::TokenKind::RawIdent => {
                    let name = &self.str_from_to(start, end)[2..];
                    if matches!(name, "_" | "self" | "Self" | "crate" | "super") {
                        self.dcx().span_err(self.mk_sp(start, end), format!("`{name}` cannot be a raw identifier"));
                    }
                    TokenKind::Ident(Symbol::intern(name), IdentIsRaw::Yes)
                }
                vsc_lexer::TokenKind::InvalidIdent => {
                    let name = self.str_from_to(start, end);
                    self.dcx().span_err(self.mk_sp(start, end), format!("identifiers cannot contain emoji: `{name}`"));
                    TokenKind::Ident(Symbol::intern(name), IdentIsRaw::No)
                }
                vsc_lexer::TokenKind::UnknownPrefix => {
                    let prefix = self.str_from_to(start, end);
                    self.dcx()
                        .struct_span_err(self.mk_sp(start, end), format!("prefix `{prefix}` is unknown"))
                        .with_span_label(self.mk_sp(start, end), "unknown prefix")
                        .with_note("prefixed identifiers and literals are reserved")
                        .with_span_suggestion(
                            self.mk_sp(end, end),
                            "consider inserting whitespace here",
                            " ",
                            Applicability::MaybeIncorrect,
                        )
                        .emit();
                    TokenKind::Ident(Symbol::intern(prefix), IdentIsRaw::No)
                }
                vsc_lexer::TokenKind::UnknownPrefixLifetime => {
                    let text = self.str_from_to(start, end);
                    self.dcx().span_err(self.mk_sp(start, end), format!("prefix `{}` is unknown", &text[1..]));
                    TokenKind::Lifetime(Symbol::intern(text), IdentIsRaw::No)
                }
                vsc_lexer::TokenKind::RawLifetime => {
                    let text = self.str_from_to(start, end);
                    let name = format!("'{}", &text[3..]);
                    TokenKind::Lifetime(Symbol::intern(&name), IdentIsRaw::Yes)
                }
                vsc_lexer::TokenKind::Lifetime { starts_with_number } => {
                    let text = self.str_from_to(start, end);
                    if starts_with_number {
                        self.dcx().span_err(self.mk_sp(start, end), "labels cannot start with a number");
                    }
                    TokenKind::Lifetime(Symbol::intern(text), IdentIsRaw::No)
                }
                vsc_lexer::TokenKind::Literal { kind, suffix_start } => {
                    let suffix_start = BytePos(start.0 + suffix_start);
                    let lit = self.cook_lexer_literal(start, suffix_start, end, kind);
                    let suffix = if suffix_start < end {
                        let s = self.str_from_to(suffix_start, end);
                        Some(Symbol::intern(s))
                    } else {
                        None
                    };
                    TokenKind::Literal(Lit { suffix, ..lit })
                }
                vsc_lexer::TokenKind::Unknown => {
                    let text = self.str_from_to(start, end);
                    let c = text.chars().next().unwrap();
                    let span = self.mk_sp(start, end);
                    if let Some(kind) = unicode_chars::check_for_substitution(self.psess, span, c) {
                        kind
                    } else {
                        let msg = format!("unknown start of token: {}", escaped_char(c));
                        self.dcx().span_err(span, msg);
                        continue;
                    }
                }
                vsc_lexer::TokenKind::Eof => TokenKind::Eof,
                punct => {
                    let (kind, hi) = self.glue_punct(punct, end);
                    return Token { kind, span: self.mk_sp(start, hi), line_break_before };
                }
            };
            return Token { kind, span: self.mk_sp(start, end), line_break_before };
        }
    }

    /// Glues adjacent punctuation into one operator, longest match first.
    /// Returns the operator and where it ends.
    fn glue_punct(&mut self, first: vsc_lexer::TokenKind, mut end: BytePos) -> (TokenKind, BytePos) {
        use vsc_lexer::TokenKind as R;
        let mut kind = single_punct(first);
        if !is_single_punct(first) {
            return (kind, end);
        }
        loop {
            let next = self.peek_raw();
            let glued = match (kind, next) {
                (TokenKind::Eq, R::Eq) => TokenKind::EqEq,
                (TokenKind::Eq, R::Gt) => TokenKind::FatArrow,
                (TokenKind::Lt, R::Eq) => TokenKind::Le,
                (TokenKind::Lt, R::Lt) => TokenKind::Shl,
                (TokenKind::Shl, R::Eq) => TokenKind::ShlEq,
                (TokenKind::Gt, R::Eq) => TokenKind::Ge,
                (TokenKind::Gt, R::Gt) => TokenKind::Shr,
                (TokenKind::Shr, R::Eq) => TokenKind::ShrEq,
                (TokenKind::Bang, R::Eq) => TokenKind::Ne,
                (TokenKind::And, R::And) => TokenKind::AndAnd,
                (TokenKind::And, R::Eq) => TokenKind::AndEq,
                (TokenKind::Or, R::Or) => TokenKind::OrOr,
                (TokenKind::Or, R::Eq) => TokenKind::OrEq,
                (TokenKind::Plus, R::Eq) => TokenKind::PlusEq,
                (TokenKind::Minus, R::Eq) => TokenKind::MinusEq,
                (TokenKind::Minus, R::Gt) => TokenKind::RArrow,
                (TokenKind::Star, R::Eq) => TokenKind::StarEq,
                (TokenKind::Slash, R::Eq) => TokenKind::SlashEq,
                (TokenKind::Percent, R::Eq) => TokenKind::PercentEq,
                (TokenKind::Caret, R::Eq) => TokenKind::CaretEq,
                (TokenKind::Dot, R::Dot) => TokenKind::DotDot,
                (TokenKind::DotDot, R::Dot) => TokenKind::DotDotDot,
                (TokenKind::DotDot, R::Eq) => TokenKind::DotDotEq,
                _ => return (kind, end),
            };
            // Consume the peeked token.
            let (tok, start) = self.peeked.take().unwrap();
            end = BytePos(start.0 + tok.len);
            kind = glued;
        }
    }

    fn doc_comment(&self, kind: CommentKind, style: DocStyle, content: &str) -> TokenKind {
        let style = match style {
            DocStyle::Outer => AttrStyle::Outer,
            DocStyle::Inner => AttrStyle::Inner,
        };
        TokenKind::DocComment(kind, style, Symbol::intern(content))
    }

    fn cook_lexer_literal(&self, start: BytePos, suffix_start: BytePos, end: BytePos, kind: LiteralKind) -> Lit {
        let full = self.mk_sp(start, end);
        match kind {
            LiteralKind::Char { terminated } => {
                if !terminated {
                    self.dcx()
                        .struct_span_err(full, "unterminated character literal")
                        .with_span_label(full, "missing closing `'`")
                        .emit();
                    return self.err_lit(start, suffix_start);
                }
                self.cook_quoted(LitKind::Char, Mode::Char, start, suffix_start, 1, 1)
            }
            LiteralKind::Byte { terminated } => {
                if !terminated {
                    self.dcx()
                        .struct_span_err(full, "unterminated byte constant")
                        .with_span_label(full, "missing closing `'`")
                        .emit();
                    return self.err_lit(start, suffix_start);
                }
                self.cook_quoted(LitKind::Byte, Mode::Byte, start, suffix_start, 2, 1)
            }
            LiteralKind::Str { terminated } => {
                if !terminated {
                    self.unterminated(full, "unterminated double quote string");
                    return self.err_lit(start, suffix_start);
                }
                self.cook_quoted(LitKind::Str, Mode::Str, start, suffix_start, 1, 1)
            }
            LiteralKind::ByteStr { terminated } => {
                if !terminated {
                    self.unterminated(full, "unterminated double quote byte string");
                    return self.err_lit(start, suffix_start);
                }
                self.cook_quoted(LitKind::ByteStr, Mode::ByteStr, start, suffix_start, 2, 1)
            }
            LiteralKind::CStr { terminated } => {
                if !terminated {
                    self.unterminated(full, "unterminated C string");
                    return self.err_lit(start, suffix_start);
                }
                self.cook_quoted(LitKind::CStr, Mode::CStr, start, suffix_start, 2, 1)
            }
            LiteralKind::RawStr { n_hashes } => self.cook_raw(LitKind::StrRaw, Mode::RawStr, n_hashes, start, suffix_start, 1),
            LiteralKind::RawByteStr { n_hashes } => {
                self.cook_raw(LitKind::ByteStrRaw, Mode::RawByteStr, n_hashes, start, suffix_start, 2)
            }
            LiteralKind::RawCStr { n_hashes } => {
                self.cook_raw(LitKind::CStrRaw, Mode::RawCStr, n_hashes, start, suffix_start, 2)
            }
            LiteralKind::Int { base, empty_int } => {
                let text = self.str_from_to(start, suffix_start);
                if empty_int {
                    self.dcx()
                        .struct_span_err(self.mk_sp(start, suffix_start), "no valid digits found for number")
                        .emit();
                    return Lit::new(LitKind::Integer, Symbol::intern("0"), None);
                }
                self.check_int_digits(base, start, text);
                Lit::new(LitKind::Integer, Symbol::intern(text), None)
            }
            LiteralKind::Float { base, empty_exponent } => {
                let text = self.str_from_to(start, suffix_start);
                if empty_exponent {
                    self.dcx().span_err(self.mk_sp(start, suffix_start), "expected at least one digit in exponent");
                }
                let name = match base {
                    Base::Hexadecimal => Some("hexadecimal"),
                    Base::Octal => Some("octal"),
                    Base::Binary => Some("binary"),
                    Base::Decimal => None,
                };
                if let Some(name) = name {
                    self.dcx().span_err(
                        self.mk_sp(start, suffix_start),
                        format!("{name} float literal is not supported"),
                    );
                }
                Lit::new(LitKind::Float, Symbol::intern(text), None)
            }
        }
    }

    fn err_lit(&self, start: BytePos, suffix_start: BytePos) -> Lit {
        Lit::new(LitKind::Err, Symbol::intern(self.str_from_to(start, suffix_start)), None)
    }

    fn unterminated(&self, span: Span, msg: &str) {
        self.dcx()
            .struct_span_err(span.shrink_to_lo().with_hi(BytePos(span.lo().0 + 1)), msg)
            .emit();
    }

    /// Binary and octal literals lex with any decimal digit; reject the ones
    /// that don't fit the base.
    fn check_int_digits(&self, base: Base, start: BytePos, text: &str) {
        let radix = base as u32;
        if radix >= 10 {
            return;
        }
        for (i, c) in text.char_indices().skip(2) {
            if c != '_' && c.to_digit(10).is_some_and(|d| d >= radix) {
                let lo = BytePos(start.0 + i as u32);
                self.dcx().span_err(
                    self.mk_sp(lo, BytePos(lo.0 + 1)),
                    format!("invalid digit for a base {radix} literal"),
                );
            }
        }
    }

    /// A literal between quotes: checks its escapes and returns its contents.
    fn cook_quoted(
        &self,
        kind: LitKind,
        mode: Mode,
        start: BytePos,
        end: BytePos,
        prefix_len: u32,
        postfix_len: u32,
    ) -> Lit {
        let content_start = BytePos(start.0 + prefix_len);
        let content_end = BytePos(end.0 - postfix_len);
        let content = self.str_from_to(content_start, content_end);
        let mut has_fatal_err = false;
        let lit_span = self.mk_sp(start, end);
        let mut report = |range: std::ops::Range<usize>, err: unescape::EscapeError| {
            if err.is_fatal() {
                has_fatal_err = true;
            }
            let span = self.mk_sp(
                BytePos(content_start.0 + range.start as u32),
                BytePos(content_start.0 + range.end as u32),
            );
            unescape_error_reporting::emit_unescape_error(self.psess, content, lit_span, span, mode, range, err);
        };
        match mode {
            Mode::CStr => unescape::unescape_mixed(content, mode, &mut |range, res| {
                if let Err(e) = res {
                    report(range, e)
                }
            }),
            _ => unescape::unescape_unicode(content, mode, &mut |range, res| {
                if let Err(e) = res {
                    report(range, e)
                }
            }),
        }
        let kind = if has_fatal_err { LitKind::Err } else { kind };
        Lit::new(kind, Symbol::intern(content), None)
    }

    fn cook_raw(
        &self,
        kind: fn(u8) -> LitKind,
        mode: Mode,
        n_hashes: Option<u8>,
        start: BytePos,
        end: BytePos,
        prefix_len: u32,
    ) -> Lit {
        let Some(n) = n_hashes else {
            self.report_raw_str_error(start, prefix_len);
            return self.err_lit(start, end);
        };
        let n = n as u32;
        let content_start = BytePos(start.0 + prefix_len + n + 1);
        let content_end = BytePos(end.0 - n - 1);
        let content = self.str_from_to(content_start, content_end);
        let mut has_err = false;
        let lit_span = self.mk_sp(start, end);
        let mut report = |range: std::ops::Range<usize>, err: unescape::EscapeError| {
            has_err = true;
            let span = self.mk_sp(
                BytePos(content_start.0 + range.start as u32),
                BytePos(content_start.0 + range.end as u32),
            );
            unescape_error_reporting::emit_unescape_error(self.psess, content, lit_span, span, mode, range, err);
        };
        match mode {
            Mode::RawCStr => unescape::unescape_mixed(content, mode, &mut |range, res| {
                if let Err(e) = res {
                    report(range, e)
                }
            }),
            _ => unescape::unescape_unicode(content, mode, &mut |range, res| {
                if let Err(e) = res {
                    report(range, e)
                }
            }),
        }
        let kind = if has_err { LitKind::Err } else { kind(n as u8) };
        Lit::new(kind, Symbol::intern(content), None)
    }

    fn report_raw_str_error(&self, start: BytePos, prefix_len: u32) {
        let text = self.str_from(start);
        match vsc_lexer::validate_raw_str(text, prefix_len) {
            Err(RawStrError::InvalidStarter { bad_char }) => {
                let lo = BytePos(start.0 + prefix_len + text[prefix_len as usize..].bytes().take_while(|&b| b == b'#').count() as u32);
                self.dcx()
                    .struct_span_err(
                        self.mk_sp(lo, BytePos(lo.0 + bad_char.len_utf8() as u32)),
                        format!("found invalid character; only `#` is allowed in raw string delimitation: {}", escaped_char(bad_char)),
                    )
                    .emit();
            }
            Err(RawStrError::NoTerminator { expected, found, possible_terminator_offset }) => {
                let span = self.mk_sp(start, BytePos(start.0 + prefix_len));
                let mut err = self.dcx().struct_span_err(span, "unterminated raw string");
                err.span_label(span, "unterminated raw string");
                if expected > 0 {
                    err.note(format!(
                        "this raw string should be terminated with `\"{}`",
                        "#".repeat(expected as usize)
                    ));
                }
                if let Some(offset) = possible_terminator_offset {
                    let lo = BytePos(start.0 + offset);
                    let sp = self.mk_sp(lo, BytePos(lo.0 + 1 + found));
                    err.span_label(sp, format!("help: consider terminating the string here: `\"{}`", "#".repeat(expected as usize)));
                }
                err.emit();
            }
            Err(RawStrError::TooManyDelimiters { found }) => {
                self.dcx().span_err(
                    self.mk_sp(start, BytePos(start.0 + prefix_len + found)),
                    format!("too many `#` symbols: raw strings may be delimited by up to 255 `#` symbols, but found {found}"),
                );
            }
            Ok(()) => {}
        }
    }
}

fn is_single_punct(kind: vsc_lexer::TokenKind) -> bool {
    use vsc_lexer::TokenKind as R;
    matches!(
        kind,
        R::Eq | R::Lt | R::Gt | R::Bang | R::And | R::Or | R::Plus | R::Minus | R::Star | R::Slash | R::Percent
            | R::Caret | R::Dot
    )
}

fn single_punct(kind: vsc_lexer::TokenKind) -> TokenKind {
    use vsc_lexer::TokenKind as R;
    match kind {
        R::Semi => TokenKind::Semi,
        R::Comma => TokenKind::Comma,
        R::Dot => TokenKind::Dot,
        R::OpenParen => TokenKind::OpenParen,
        R::CloseParen => TokenKind::CloseParen,
        R::OpenBrace => TokenKind::OpenBrace,
        R::CloseBrace => TokenKind::CloseBrace,
        R::OpenBracket => TokenKind::OpenBracket,
        R::CloseBracket => TokenKind::CloseBracket,
        R::At => TokenKind::At,
        R::Pound => TokenKind::Pound,
        R::Tilde => TokenKind::Tilde,
        R::Question => TokenKind::Question,
        R::Colon => TokenKind::Colon,
        R::Dollar => TokenKind::Dollar,
        R::Eq => TokenKind::Eq,
        R::Bang => TokenKind::Bang,
        R::Lt => TokenKind::Lt,
        R::Gt => TokenKind::Gt,
        R::Minus => TokenKind::Minus,
        R::And => TokenKind::And,
        R::Or => TokenKind::Or,
        R::Plus => TokenKind::Plus,
        R::Star => TokenKind::Star,
        R::Slash => TokenKind::Slash,
        R::Caret => TokenKind::Caret,
        R::Percent => TokenKind::Percent,
        other => unreachable!("not punctuation: {other:?}"),
    }
}

pub(crate) fn escaped_char(c: char) -> String {
    match c {
        '\u{20}'..='\u{7e}' => c.to_string(),
        _ => c.escape_default().to_string(),
    }
}

