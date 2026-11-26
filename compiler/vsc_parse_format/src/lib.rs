//! Format strings (rustc's `rustc_parse_format`).
//!
//! The syntax is Rust's `std::fmt`, unchanged:
//!
//! ```text
//! format_spec := [[fill]align][sign]['#']['0'][width]['.' precision]type
//! fill        := character
//! align       := '<' | '^' | '>'
//! sign        := '+' | '-'
//! width       := count
//! precision   := count | '*'
//! type        := '' | '?' | 'x?' | 'X?' | identifier
//! count       := parameter | integer
//! parameter   := argument '$'
//! argument    := integer | identifier
//! ```
//!
//! [`parse`] works on the literal's contents as written, so byte offsets map
//! straight to spans. Escapes inside the literal (`\n`) are skipped over as
//! text; `{{` and `}}` are literal braces.

use std::ops::Range;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq)]
pub enum Piece<'a> {
    /// Text, with `{{`/`}}` already reduced to one brace.
    Lit(String),
    /// `{…}`
    NextArgument(Box<Argument<'a>>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Argument<'a> {
    pub position: Position<'a>,
    /// Where the position was written (empty for `{}`).
    pub position_span: Range<usize>,
    pub format: FormatSpec<'a>,
    /// The whole `{…}`.
    pub span: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position<'a> {
    /// `{}`: the next argument, counting from 0.
    ArgumentImplicitlyIs(usize),
    /// `{2}`
    ArgumentIs(usize),
    /// `{name}`
    ArgumentNamed(&'a str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Left,
    Right,
    Center,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    Plus,
    Minus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugHex {
    Lower,
    Upper,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Count<'a> {
    /// `5`
    CountIs(usize),
    /// `name$`
    CountIsName(&'a str, Range<usize>),
    /// `1$`
    CountIsParam(usize),
    /// `.*`: the precision comes from the next argument, which has this index.
    CountIsStar(usize),
    CountImplied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatSpec<'a> {
    pub fill: Option<char>,
    pub align: Alignment,
    pub sign: Option<Sign>,
    /// `#`
    pub alternate: bool,
    /// `0`
    pub zero_pad: bool,
    /// `x?` / `X?`
    pub debug_hex: Option<DebugHex>,
    pub precision: Count<'a>,
    pub width: Count<'a>,
    /// The trait: `""` (Display), `"?"` (Debug), `"x"`, `"X"`, `"o"`, `"b"`,
    /// `"e"`, `"E"`, `"p"`.
    pub ty: &'a str,
    pub ty_span: Option<Range<usize>>,
}

impl Default for FormatSpec<'_> {
    fn default() -> Self {
        FormatSpec {
            fill: None,
            align: Alignment::Unknown,
            sign: None,
            alternate: false,
            zero_pad: false,
            debug_hex: None,
            precision: Count::CountImplied,
            width: Count::CountImplied,
            ty: "",
            ty_span: None,
        }
    }
}

/// A format string error, with where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub description: String,
    pub label: String,
    pub span: Range<usize>,
    pub note: Option<String>,
}

/// Parses `s` into pieces. Stops at the first error.
pub fn parse(s: &str) -> Result<Vec<Piece<'_>>, ParseError> {
    let mut p = Parser { s, pos: 0, next_implicit: 0 };
    let mut pieces = Vec::new();
    let mut lit = String::new();
    while p.pos < s.len() {
        let c = p.peek().unwrap();
        match c {
            '{' if p.peek_at(1) == Some('{') => {
                lit.push('{');
                p.pos += 2;
            }
            '{' => {
                if !lit.is_empty() {
                    pieces.push(Piece::Lit(std::mem::take(&mut lit)));
                }
                pieces.push(Piece::NextArgument(Box::new(p.argument()?)));
            }
            '}' if p.peek_at(1) == Some('}') => {
                lit.push('}');
                p.pos += 2;
            }
            '}' => {
                return Err(ParseError {
                    description: "unmatched `}` found".to_string(),
                    label: "unmatched `}`".to_string(),
                    span: p.pos..p.pos + 1,
                    note: Some("if you intended to print `}`, you can escape it using `}}`".to_string()),
                });
            }
            // An escape in the source literal: keep it as text.
            '\\' => {
                let len = 1 + p.s[p.pos + 1..].chars().next().map_or(0, char::len_utf8);
                lit.push_str(&s[p.pos..p.pos + len]);
                p.pos += len;
            }
            c => {
                lit.push(c);
                p.pos += c.len_utf8();
            }
        }
    }
    if !lit.is_empty() {
        pieces.push(Piece::Lit(lit));
    }
    Ok(pieces)
}

struct Parser<'a> {
    s: &'a str,
    pos: usize,
    next_implicit: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<char> {
        self.s[self.pos..].chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.s[self.pos..].chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.bump();
        }
    }

    /// `{position:spec}`, at its `{`.
    fn argument(&mut self) -> Result<Argument<'a>, ParseError> {
        let start = self.pos;
        self.bump(); // `{`
        self.skip_ws();
        let pos_start = self.pos;
        let explicit = self.position();
        let position_span = pos_start..self.pos;
        self.skip_ws();
        let mut format = FormatSpec::default();
        if self.eat(':') {
            format = self.spec()?;
        }
        self.skip_ws();
        if !self.eat('}') {
            return Err(self.expected_close(start));
        }
        let position = match explicit {
            Some(p) => p,
            None => {
                let i = self.next_implicit;
                self.next_implicit += 1;
                Position::ArgumentImplicitlyIs(i)
            }
        };
        // `.*` takes the next implicit argument for the precision, before
        // the value.
        if let Count::CountIsStar(_) = format.precision {
            let i = self.next_implicit;
            if let Position::ArgumentImplicitlyIs(v) = position {
                // The precision comes first: renumber.
                format.precision = Count::CountIsStar(v);
                self.next_implicit += 1;
                return Ok(Argument {
                    position: Position::ArgumentImplicitlyIs(v + 1),
                    position_span,
                    format,
                    span: start..self.pos,
                });
            }
            format.precision = Count::CountIsStar(i);
            self.next_implicit += 1;
        }
        Ok(Argument { position, position_span, format, span: start..self.pos })
    }

    fn expected_close(&self, start: usize) -> ParseError {
        match self.peek() {
            None => ParseError {
                description: "expected `}` but string was terminated".to_string(),
                label: "expected `}` in format string".to_string(),
                span: start..start + 1,
                note: Some("if you intended to print `{`, you can escape it using `{{`".to_string()),
            },
            Some(c) => ParseError {
                description: format!("expected `}}`, found `{c}`"),
                label: "expected `}` in format string".to_string(),
                span: self.pos..self.pos + c.len_utf8(),
                note: Some("if you intended to print `{`, you can escape it using `{{`".to_string()),
            },
        }
    }

    fn position(&mut self) -> Option<Position<'a>> {
        if let Some(n) = self.integer() {
            return Some(Position::ArgumentIs(n));
        }
        let ident = self.identifier()?;
        Some(Position::ArgumentNamed(ident))
    }

    fn integer(&mut self) -> Option<usize> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        if self.pos == start { None } else { self.s[start..self.pos].parse().ok() }
    }

    fn identifier(&mut self) -> Option<&'a str> {
        let start = self.pos;
        let first = self.peek()?;
        if !(first == '_' || first.is_alphabetic()) {
            return None;
        }
        while self.peek().is_some_and(|c| c == '_' || c.is_alphanumeric()) {
            self.bump();
        }
        Some(&self.s[start..self.pos])
    }

    /// After `:`.
    fn spec(&mut self) -> Result<FormatSpec<'a>, ParseError> {
        let mut spec = FormatSpec::default();
        // `[[fill]align]`
        let align_of = |c| match c {
            Some('<') => Some(Alignment::Left),
            Some('>') => Some(Alignment::Right),
            Some('^') => Some(Alignment::Center),
            _ => None,
        };
        if let (Some(fill), Some(a)) = (self.peek(), align_of(self.peek_at(1))) {
            if fill != '}' {
                spec.fill = Some(fill);
                spec.align = a;
                self.bump();
                self.bump();
            }
        }
        if spec.fill.is_none() {
            if let Some(a) = align_of(self.peek()) {
                spec.align = a;
                self.bump();
            }
        }
        if self.eat('+') {
            spec.sign = Some(Sign::Plus);
        } else if self.eat('-') {
            spec.sign = Some(Sign::Minus);
        }
        if self.eat('#') {
            spec.alternate = true;
        }
        // `0` is zero-padding unless it starts `0$`.
        if self.peek() == Some('0') && self.peek_at(1) != Some('$') {
            spec.zero_pad = true;
            self.bump();
        }
        spec.width = self.count();
        if self.eat('.') {
            if self.eat('*') {
                spec.precision = Count::CountIsStar(0);
            } else {
                spec.precision = self.count();
                if spec.precision == Count::CountImplied {
                    return Err(ParseError {
                        description: "expected a precision after `.`".to_string(),
                        label: "expected a number, `name$` or `*`".to_string(),
                        span: self.pos - 1..self.pos,
                        note: None,
                    });
                }
            }
        }
        // The trait.
        let ty_start = self.pos;
        if self.eat('x') {
            if self.eat('?') {
                spec.debug_hex = Some(DebugHex::Lower);
                spec.ty = "?";
            } else {
                spec.ty = "x";
            }
        } else if self.eat('X') {
            if self.eat('?') {
                spec.debug_hex = Some(DebugHex::Upper);
                spec.ty = "?";
            } else {
                spec.ty = "X";
            }
        } else if self.eat('?') {
            spec.ty = "?";
        } else if let Some(word) = self.identifier() {
            spec.ty = word;
        }
        if self.pos > ty_start {
            spec.ty_span = Some(ty_start..self.pos);
        }
        Ok(spec)
    }

    /// `5`, `name$` or `1$`, or nothing.
    fn count(&mut self) -> Count<'a> {
        let start = self.pos;
        if let Some(n) = self.integer() {
            if self.eat('$') {
                return Count::CountIsParam(n);
            }
            return Count::CountIs(n);
        }
        if let Some(name) = self.identifier() {
            if self.eat('$') {
                return Count::CountIsName(name, start..self.pos - 1);
            }
            // Not a count: it was the trait name.
            self.pos = start;
        }
        Count::CountImplied
    }
}
