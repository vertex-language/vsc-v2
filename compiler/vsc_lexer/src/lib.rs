//! The raw lexer (rustc's `rustc_lexer`).
//!
//! Turns source text into a stream of raw tokens: kinds and lengths, nothing
//! else. Punctuation comes out one char at a time, and whitespace and comments
//! are tokens too. `vsc_parse` "cooks" these into the parser's tokens: it glues
//! punctuation (`==`, `->`), drops trivia, interns names and decides which line
//! feeds matter (exception 10).
//!
//! Differences from Rust's lexer follow the grammar: `@` is an attribute
//! sigil, and `#`, `$` and `::` are not Vertex punctuation. The raw lexer still
//! produces `#` and `$` (raw strings need `#`), and the parser reports them.

mod cursor;
pub mod unescape;

#[cfg(test)]
mod tests;

pub use cursor::Cursor;
use cursor::EOF_CHAR;

/// A raw token: a kind and a length in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub len: u32,
}

impl Token {
    fn new(kind: TokenKind, len: u32) -> Token {
        Token { kind, len }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    /// `// comment`, `/// outer doc`, `//! inner doc`.
    LineComment { doc_style: Option<DocStyle> },
    /// `/* comment */`, `/** outer doc */`, `/*! inner doc */`.
    BlockComment { doc_style: Option<DocStyle>, terminated: bool },
    /// Any run of whitespace, line feeds included.
    Whitespace,
    /// An identifier or keyword: `func`, `x`.
    Ident,
    /// An identifier with chars that can't be in one, such as emoji.
    InvalidIdent,
    /// `r#ident`.
    RawIdent,
    /// An identifier immediately followed by a quote or `#` that isn't a
    /// literal prefix (`foo"…"`, `foo#`): reserved.
    UnknownPrefix,
    /// `'ident#`: reserved.
    UnknownPrefixLifetime,
    /// `'r#ident`.
    RawLifetime,
    /// A literal, with its suffix (`1uint8`) starting at `suffix_start`.
    Literal { kind: LiteralKind, suffix_start: u32 },
    /// `'ident`: a loop label. (Vertex has no lifetimes; exception 13.)
    Lifetime { starts_with_number: bool },
    Semi,
    Comma,
    Dot,
    OpenParen,
    CloseParen,
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    At,
    Pound,
    Tilde,
    Question,
    Colon,
    Dollar,
    Eq,
    Bang,
    Lt,
    Gt,
    Minus,
    And,
    Or,
    Plus,
    Star,
    Slash,
    Caret,
    Percent,
    /// A char that can't start any token.
    Unknown,
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocStyle {
    Outer,
    Inner,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LiteralKind {
    /// `12`, `0x1f`, `0b1010`, `0o17`.
    Int { base: Base, empty_int: bool },
    /// `1.5`, `2e10`.
    Float { base: Base, empty_exponent: bool },
    /// `'a'`.
    Char { terminated: bool },
    /// `b'a'`.
    Byte { terminated: bool },
    /// `"abc"`.
    Str { terminated: bool },
    /// `b"abc"`.
    ByteStr { terminated: bool },
    /// `c"abc"`.
    CStr { terminated: bool },
    /// `r"abc"`, `r#"abc"#`. `None` means the delimiters were invalid.
    RawStr { n_hashes: Option<u8> },
    /// `br"abc"`.
    RawByteStr { n_hashes: Option<u8> },
    /// `cr"abc"`.
    RawCStr { n_hashes: Option<u8> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Base {
    Binary = 2,
    Octal = 8,
    Decimal = 10,
    Hexadecimal = 16,
}

/// What went wrong with a raw string's delimiters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawStrError {
    /// Something other than `"` after the `#`s: `r#~"`.
    InvalidStarter { bad_char: char },
    /// The string never ends. `possible_terminator_offset` is where the
    /// longest run of closing `#`s started, if any.
    NoTerminator { expected: u32, found: u32, possible_terminator_offset: Option<u32> },
    /// More than 255 `#`s.
    TooManyDelimiters { found: u32 },
}

/// The length of a `#!` line at the start of the input, if there is one.
///
/// `#![` starts an attribute in Rust rather than a shebang. Vertex has no inner
/// attributes, but keeps the rule so the same files lex the same way.
pub fn strip_shebang(input: &str) -> Option<usize> {
    let tail = input.strip_prefix("#!")?;
    let next = tokenize(tail).map(|t| t.kind).find(|kind| {
        !matches!(
            kind,
            TokenKind::Whitespace
                | TokenKind::LineComment { doc_style: None }
                | TokenKind::BlockComment { doc_style: None, .. }
        )
    });
    if next != Some(TokenKind::OpenBracket) {
        return Some(2 + tail.lines().next().unwrap_or_default().len());
    }
    None
}

/// Checks a raw string literal's delimiters; `prefix_len` is the length of
/// `r`, `br` or `cr`.
pub fn validate_raw_str(input: &str, prefix_len: u32) -> Result<(), RawStrError> {
    debug_assert!(!input.is_empty());
    let mut cursor = Cursor::new(input);
    for _ in 0..prefix_len {
        cursor.bump().unwrap();
    }
    cursor.raw_double_quoted_string().map(|_| ())
}

/// The raw tokens of `input`, ending before `Eof`.
pub fn tokenize(input: &str) -> impl Iterator<Item = Token> + '_ {
    let mut cursor = Cursor::new(input);
    std::iter::from_fn(move || {
        let token = cursor.advance_token();
        (token.kind != TokenKind::Eof).then_some(token)
    })
}

/// Whitespace as the grammar defines it (Unicode `Pattern_White_Space`).
pub fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{0085}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

/// Can `c` start an identifier? (`_` or Unicode `XID_Start`.)
pub fn is_id_start(c: char) -> bool {
    c == '_' || c.is_ascii_alphabetic() || (!c.is_ascii() && unicode::is_xid_start(c))
}

/// Can `c` continue an identifier? (Unicode `XID_Continue`.)
pub fn is_id_continue(c: char) -> bool {
    c == '_' || c.is_ascii_alphanumeric() || (!c.is_ascii() && unicode::is_xid_continue(c))
}

/// Is `s` a single identifier?
pub fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(start) => is_id_start(start) && chars.all(is_id_continue),
        None => false,
    }
}

mod unicode {
    //! `XID_Start` / `XID_Continue` for non-ASCII chars.
    //!
    //! This approximates the Unicode properties with std's `Alphabetic` and
    //! `Numeric` plus the combining-mark blocks. Exact tables come from
    //! `src/tools/unicode-table-generator`, which isn't written yet.

    pub(crate) fn is_xid_start(c: char) -> bool {
        c.is_alphabetic() && !is_combining_mark(c)
    }

    pub(crate) fn is_xid_continue(c: char) -> bool {
        c.is_alphanumeric() || is_combining_mark(c) || is_connector(c)
    }

    fn is_combining_mark(c: char) -> bool {
        matches!(c,
            '\u{0300}'..='\u{036F}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{1DC0}'..='\u{1DFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}')
    }

    fn is_connector(c: char) -> bool {
        matches!(c, '\u{203F}' | '\u{2040}' | '\u{2054}' | '\u{FE33}' | '\u{FE34}' | '\u{FE4D}'..='\u{FE4F}' | '\u{FF3F}')
    }
}

impl Cursor<'_> {
    /// Lexes the next token.
    pub fn advance_token(&mut self) -> Token {
        let Some(first_char) = self.bump() else {
            return Token::new(TokenKind::Eof, 0);
        };
        use TokenKind::*;
        let kind = match first_char {
            '/' => match self.first() {
                '/' => self.line_comment(),
                '*' => self.block_comment(),
                _ => Slash,
            },
            c if is_whitespace(c) => {
                self.eat_while(is_whitespace);
                Whitespace
            }
            // Raw identifiers, raw strings, or an identifier starting with `r`.
            'r' => match (self.first(), self.second()) {
                ('#', c1) if is_id_start(c1) => self.raw_ident(),
                ('#', _) | ('"', _) => {
                    let res = self.raw_double_quoted_string();
                    let suffix_start = self.pos_within_token();
                    if res.is_ok() {
                        self.eat_literal_suffix();
                    }
                    Literal { kind: LiteralKind::RawStr { n_hashes: res.ok() }, suffix_start }
                }
                _ => self.ident_or_unknown_prefix(),
            },
            'b' => self.prefixed_string(
                |terminated| LiteralKind::ByteStr { terminated },
                |n_hashes| LiteralKind::RawByteStr { n_hashes },
                Some(|terminated| LiteralKind::Byte { terminated }),
            ),
            'c' => self.prefixed_string(
                |terminated| LiteralKind::CStr { terminated },
                |n_hashes| LiteralKind::RawCStr { n_hashes },
                None,
            ),
            c if is_id_start(c) => self.ident_or_unknown_prefix(),
            c @ '0'..='9' => {
                let kind = self.number(c);
                let suffix_start = self.pos_within_token();
                self.eat_literal_suffix();
                Literal { kind, suffix_start }
            }
            '\'' => self.lifetime_or_char(),
            '"' => {
                let terminated = self.double_quoted_string();
                let suffix_start = self.pos_within_token();
                if terminated {
                    self.eat_literal_suffix();
                }
                Literal { kind: LiteralKind::Str { terminated }, suffix_start }
            }
            ';' => Semi,
            ',' => Comma,
            '.' => Dot,
            '(' => OpenParen,
            ')' => CloseParen,
            '{' => OpenBrace,
            '}' => CloseBrace,
            '[' => OpenBracket,
            ']' => CloseBracket,
            '@' => At,
            '#' => Pound,
            '~' => Tilde,
            '?' => Question,
            ':' => Colon,
            '$' => Dollar,
            '=' => Eq,
            '!' => Bang,
            '<' => Lt,
            '>' => Gt,
            '-' => Minus,
            '&' => And,
            '|' => Or,
            '+' => Plus,
            '*' => Star,
            '^' => Caret,
            '%' => Percent,
            c if !c.is_ascii() && is_emoji(c) => self.invalid_ident(),
            _ => Unknown,
        };
        let token = Token::new(kind, self.pos_within_token());
        self.reset_pos_within_token();
        token
    }

    fn line_comment(&mut self) -> TokenKind {
        debug_assert_eq!(self.first(), '/');
        self.bump();
        let doc_style = match self.first() {
            // `//!` is an inner doc comment.
            '!' => Some(DocStyle::Inner),
            // `////` (four or more slashes) is an ordinary comment.
            '/' if self.second() != '/' => Some(DocStyle::Outer),
            _ => None,
        };
        self.eat_until(b'\n');
        TokenKind::LineComment { doc_style }
    }

    fn block_comment(&mut self) -> TokenKind {
        debug_assert_eq!(self.first(), '*');
        self.bump();
        let doc_style = match self.first() {
            '!' => Some(DocStyle::Inner),
            // `/***` and `/**/` are ordinary comments.
            '*' if !matches!(self.second(), '*' | '/') => Some(DocStyle::Outer),
            _ => None,
        };
        let mut depth = 1usize;
        while let Some(c) = self.bump() {
            match c {
                '/' if self.first() == '*' => {
                    self.bump();
                    depth += 1;
                }
                '*' if self.first() == '/' => {
                    self.bump();
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        TokenKind::BlockComment { doc_style, terminated: depth == 0 }
    }

    fn raw_ident(&mut self) -> TokenKind {
        debug_assert!(self.first() == '#' && is_id_start(self.second()));
        self.bump(); // `#`
        self.eat_identifier();
        TokenKind::RawIdent
    }

    fn ident_or_unknown_prefix(&mut self) -> TokenKind {
        self.eat_while(is_id_continue);
        match self.first() {
            '#' | '"' | '\'' => TokenKind::UnknownPrefix,
            c if !c.is_ascii() && is_emoji(c) => self.invalid_ident(),
            _ => TokenKind::Ident,
        }
    }

    fn invalid_ident(&mut self) -> TokenKind {
        // Take the rest of the would-be identifier, emoji (and ZWJ) included.
        self.eat_while(|c| is_id_continue(c) || (!c.is_ascii() && is_emoji(c)) || c == '\u{200d}');
        TokenKind::InvalidIdent
    }

    /// `b'…'`, `b"…"`, `br"…"`, `c"…"`, `cr"…"`, or an identifier starting
    /// with `b` or `c`.
    fn prefixed_string(
        &mut self,
        mk_kind: fn(bool) -> LiteralKind,
        mk_kind_raw: fn(Option<u8>) -> LiteralKind,
        single_quoted: Option<fn(bool) -> LiteralKind>,
    ) -> TokenKind {
        match (self.first(), self.second(), single_quoted) {
            ('\'', _, Some(single_quoted)) => {
                self.bump();
                let terminated = self.single_quoted_string();
                let suffix_start = self.pos_within_token();
                if terminated {
                    self.eat_literal_suffix();
                }
                TokenKind::Literal { kind: single_quoted(terminated), suffix_start }
            }
            ('"', _, _) => {
                self.bump();
                let terminated = self.double_quoted_string();
                let suffix_start = self.pos_within_token();
                if terminated {
                    self.eat_literal_suffix();
                }
                TokenKind::Literal { kind: mk_kind(terminated), suffix_start }
            }
            ('r', '"', _) | ('r', '#', _) => {
                self.bump();
                let res = self.raw_double_quoted_string();
                let suffix_start = self.pos_within_token();
                if res.is_ok() {
                    self.eat_literal_suffix();
                }
                TokenKind::Literal { kind: mk_kind_raw(res.ok()), suffix_start }
            }
            _ => self.ident_or_unknown_prefix(),
        }
    }

    fn number(&mut self, first_digit: char) -> LiteralKind {
        let mut base = Base::Decimal;
        if first_digit == '0' {
            match self.first() {
                'b' => {
                    base = Base::Binary;
                    self.bump();
                    if !self.eat_decimal_digits() {
                        return LiteralKind::Int { base, empty_int: true };
                    }
                }
                'o' => {
                    base = Base::Octal;
                    self.bump();
                    if !self.eat_decimal_digits() {
                        return LiteralKind::Int { base, empty_int: true };
                    }
                }
                'x' => {
                    base = Base::Hexadecimal;
                    self.bump();
                    if !self.eat_hexadecimal_digits() {
                        return LiteralKind::Int { base, empty_int: true };
                    }
                }
                '0'..='9' | '_' => {
                    self.eat_decimal_digits();
                }
                '.' | 'e' | 'E' => {}
                _ => return LiteralKind::Int { base, empty_int: false },
            }
        } else {
            self.eat_decimal_digits();
        }

        match self.first() {
            // `1.` followed by `.` is a range, and followed by an identifier
            // (`1.Max()`) is a method call: neither is a float.
            '.' if self.second() != '.' && !is_id_start(self.second()) => {
                self.bump();
                let mut empty_exponent = false;
                if self.first().is_ascii_digit() {
                    self.eat_decimal_digits();
                    if matches!(self.first(), 'e' | 'E') {
                        self.bump();
                        empty_exponent = !self.eat_float_exponent();
                    }
                }
                LiteralKind::Float { base, empty_exponent }
            }
            'e' | 'E' => {
                self.bump();
                let empty_exponent = !self.eat_float_exponent();
                LiteralKind::Float { base, empty_exponent }
            }
            _ => LiteralKind::Int { base, empty_int: false },
        }
    }

    fn lifetime_or_char(&mut self) -> TokenKind {
        let can_be_a_lifetime = if self.second() == '\'' {
            // `'a'` is a char literal.
            false
        } else {
            is_id_start(self.first()) || self.first().is_ascii_digit()
        };

        if !can_be_a_lifetime {
            let terminated = self.single_quoted_string();
            let suffix_start = self.pos_within_token();
            if terminated {
                self.eat_literal_suffix();
            }
            return TokenKind::Literal { kind: LiteralKind::Char { terminated }, suffix_start };
        }

        if self.first() == 'r' && self.second() == '#' && is_id_start(self.third()) {
            self.bump();
            self.bump();
            self.eat_while(is_id_continue);
            return TokenKind::RawLifetime;
        }

        let starts_with_number = self.first().is_ascii_digit();
        self.bump();
        self.eat_while(is_id_continue);

        match self.first() {
            // `'ab'` is an (invalid) char literal; the parser reports it.
            '\'' => {
                self.bump();
                TokenKind::Literal { kind: LiteralKind::Char { terminated: true }, suffix_start: self.pos_within_token() }
            }
            '#' if !starts_with_number => TokenKind::UnknownPrefixLifetime,
            _ => TokenKind::Lifetime { starts_with_number },
        }
    }

    /// The body of `'…'` after the opening quote. Returns whether it closed.
    fn single_quoted_string(&mut self) -> bool {
        // A one-char literal.
        if self.second() == '\'' && self.first() != '\\' {
            self.bump();
            self.bump();
            return true;
        }
        loop {
            match self.first() {
                '\'' => {
                    self.bump();
                    return true;
                }
                // Probably the start of a comment, not of the char: stop.
                '/' => break,
                '\n' if self.second() != '\'' => break,
                EOF_CHAR if self.is_eof() => break,
                '\\' => {
                    self.bump();
                    self.bump();
                }
                _ => {
                    self.bump();
                }
            }
        }
        false
    }

    /// The body of `"…"` after the opening quote. Returns whether it closed.
    fn double_quoted_string(&mut self) -> bool {
        while let Some(c) = self.bump() {
            match c {
                '"' => return true,
                '\\' if self.first() == '\\' || self.first() == '"' => {
                    self.bump();
                }
                _ => {}
            }
        }
        false
    }

    /// The rest of `r#"…"#` after the prefix. Returns the number of `#`s.
    fn raw_double_quoted_string(&mut self) -> Result<u8, RawStrError> {
        let n_hashes = self.raw_string_unvalidated()?;
        match u8::try_from(n_hashes) {
            Ok(n) => Ok(n),
            Err(_) => Err(RawStrError::TooManyDelimiters { found: n_hashes }),
        }
    }

    fn raw_string_unvalidated(&mut self) -> Result<u32, RawStrError> {
        let mut possible_terminator_offset = None;
        let mut max_hashes = 0;

        let mut n_start_hashes: u32 = 0;
        while self.first() == '#' {
            n_start_hashes += 1;
            self.bump();
        }

        match self.bump() {
            Some('"') => {}
            c => return Err(RawStrError::InvalidStarter { bad_char: c.unwrap_or(EOF_CHAR) }),
        }

        loop {
            self.eat_until(b'"');
            if self.is_eof() {
                return Err(RawStrError::NoTerminator {
                    expected: n_start_hashes,
                    found: max_hashes,
                    possible_terminator_offset,
                });
            }
            self.bump(); // `"`

            let mut n_end_hashes = 0;
            while self.first() == '#' && n_end_hashes < n_start_hashes {
                n_end_hashes += 1;
                self.bump();
            }
            if n_end_hashes == n_start_hashes {
                return Ok(n_start_hashes);
            } else if n_end_hashes > max_hashes {
                // The offset of the closing quote, from the start of the token.
                possible_terminator_offset = Some(self.pos_within_token() - n_end_hashes - 1);
                max_hashes = n_end_hashes;
            }
        }
    }

    fn eat_decimal_digits(&mut self) -> bool {
        let mut has_digits = false;
        loop {
            match self.first() {
                '_' => {
                    self.bump();
                }
                '0'..='9' => {
                    has_digits = true;
                    self.bump();
                }
                _ => break,
            }
        }
        has_digits
    }

    fn eat_hexadecimal_digits(&mut self) -> bool {
        let mut has_digits = false;
        loop {
            match self.first() {
                '_' => {
                    self.bump();
                }
                '0'..='9' | 'a'..='f' | 'A'..='F' => {
                    has_digits = true;
                    self.bump();
                }
                _ => break,
            }
        }
        has_digits
    }

    fn eat_float_exponent(&mut self) -> bool {
        if self.first() == '-' || self.first() == '+' {
            self.bump();
        }
        self.eat_decimal_digits()
    }

    fn eat_literal_suffix(&mut self) {
        self.eat_identifier();
    }

    fn eat_identifier(&mut self) {
        if !is_id_start(self.first()) {
            return;
        }
        self.bump();
        self.eat_while(is_id_continue);
    }
}

/// Emoji are rejected as identifiers with a dedicated error rather than as
/// unknown chars.
fn is_emoji(c: char) -> bool {
    matches!(c,
        '\u{1F300}'..='\u{1FAFF}'
        | '\u{2600}'..='\u{27BF}'
        | '\u{1F000}'..='\u{1F2FF}'
        | '\u{FE0F}')
}
