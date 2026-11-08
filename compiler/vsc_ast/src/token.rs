//! The parser's tokens: raw tokens from `vsc_lexer`, cooked.

use std::fmt;

use vsc_span::{Ident, Span, Symbol, kw, sym};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CommentKind {
    Line,
    Block,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AttrStyle {
    Outer,
    Inner,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Delimiter {
    /// `( … )`
    Parenthesis,
    /// `{ … }`
    Brace,
    /// `[ … ]`
    Bracket,
}

impl Delimiter {
    pub fn open(self) -> char {
        match self {
            Delimiter::Parenthesis => '(',
            Delimiter::Brace => '{',
            Delimiter::Bracket => '[',
        }
    }

    pub fn close(self) -> char {
        match self {
            Delimiter::Parenthesis => ')',
            Delimiter::Brace => '}',
            Delimiter::Bracket => ']',
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LitKind {
    Bool,
    Byte,
    Char,
    Integer,
    Float,
    Str,
    StrRaw(u8),
    ByteStr,
    ByteStrRaw(u8),
    CStr,
    CStrRaw(u8),
    /// A literal the lexer already reported an error for.
    Err,
}

/// A literal token: its kind, its text between the delimiters, and its suffix.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Lit {
    pub kind: LitKind,
    pub symbol: Symbol,
    pub suffix: Option<Symbol>,
}

impl Lit {
    pub fn new(kind: LitKind, symbol: Symbol, suffix: Option<Symbol>) -> Lit {
        Lit { kind, symbol, suffix }
    }
}

impl fmt::Display for Lit {
    /// The literal as it was written.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Lit { kind, symbol, suffix } = *self;
        match kind {
            LitKind::Byte => write!(f, "b'{symbol}'")?,
            LitKind::Char => write!(f, "'{symbol}'")?,
            LitKind::Str => write!(f, "\"{symbol}\"")?,
            LitKind::StrRaw(n) => write!(f, "r{0}\"{1}\"{0}", "#".repeat(n as usize), symbol)?,
            LitKind::ByteStr => write!(f, "b\"{symbol}\"")?,
            LitKind::ByteStrRaw(n) => write!(f, "br{0}\"{1}\"{0}", "#".repeat(n as usize), symbol)?,
            LitKind::CStr => write!(f, "c\"{symbol}\"")?,
            LitKind::CStrRaw(n) => write!(f, "cr{0}\"{1}\"{0}", "#".repeat(n as usize), symbol)?,
            LitKind::Integer | LitKind::Float | LitKind::Bool | LitKind::Err => write!(f, "{symbol}")?,
        }
        if let Some(suffix) = suffix {
            write!(f, "{suffix}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum IdentIsRaw {
    No,
    Yes,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TokenKind {
    /// `=`
    Eq,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `==`
    EqEq,
    /// `!=`
    Ne,
    /// `>=`
    Ge,
    /// `>`
    Gt,
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `!`
    Bang,
    /// `~`
    Tilde,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    And,
    Or,
    Shl,
    Shr,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    CaretEq,
    AndEq,
    OrEq,
    ShlEq,
    ShrEq,
    /// `@`
    At,
    /// `.`
    Dot,
    /// `..`
    DotDot,
    /// `...`
    DotDotDot,
    /// `..=`
    DotDotEq,
    Comma,
    Semi,
    Colon,
    /// `->`
    RArrow,
    /// `<-`
    LArrow,
    /// `=>`
    FatArrow,
    /// `#`: not Vertex punctuation; kept so the parser can explain.
    Pound,
    /// `$`: not Vertex punctuation; kept so the parser can explain.
    Dollar,
    Question,
    OpenParen,
    CloseParen,
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Literal(Lit),
    /// An identifier or keyword.
    Ident(Symbol, IdentIsRaw),
    /// `'label`
    Lifetime(Symbol, IdentIsRaw),
    DocComment(CommentKind, AttrStyle, Symbol),
    Eof,
}

impl TokenKind {
    pub fn open_delim(self) -> Option<Delimiter> {
        match self {
            TokenKind::OpenParen => Some(Delimiter::Parenthesis),
            TokenKind::OpenBrace => Some(Delimiter::Brace),
            TokenKind::OpenBracket => Some(Delimiter::Bracket),
            _ => None,
        }
    }

    pub fn close_delim(self) -> Option<Delimiter> {
        match self {
            TokenKind::CloseParen => Some(Delimiter::Parenthesis),
            TokenKind::CloseBrace => Some(Delimiter::Brace),
            TokenKind::CloseBracket => Some(Delimiter::Bracket),
            _ => None,
        }
    }

    pub fn open_for(delim: Delimiter) -> TokenKind {
        match delim {
            Delimiter::Parenthesis => TokenKind::OpenParen,
            Delimiter::Brace => TokenKind::OpenBrace,
            Delimiter::Bracket => TokenKind::OpenBracket,
        }
    }

    pub fn close_for(delim: Delimiter) -> TokenKind {
        match delim {
            Delimiter::Parenthesis => TokenKind::CloseParen,
            Delimiter::Brace => TokenKind::CloseBrace,
            Delimiter::Bracket => TokenKind::CloseBracket,
        }
    }

    /// The token's text, for diagnostics and printing.
    pub fn as_string(self) -> String {
        use TokenKind::*;
        let s = match self {
            Eq => "=",
            Lt => "<",
            Le => "<=",
            EqEq => "==",
            Ne => "!=",
            Ge => ">=",
            Gt => ">",
            AndAnd => "&&",
            OrOr => "||",
            Bang => "!",
            Tilde => "~",
            Plus => "+",
            Minus => "-",
            Star => "*",
            Slash => "/",
            Percent => "%",
            Caret => "^",
            And => "&",
            Or => "|",
            Shl => "<<",
            Shr => ">>",
            PlusEq => "+=",
            MinusEq => "-=",
            StarEq => "*=",
            SlashEq => "/=",
            PercentEq => "%=",
            CaretEq => "^=",
            AndEq => "&=",
            OrEq => "|=",
            ShlEq => "<<=",
            ShrEq => ">>=",
            At => "@",
            Dot => ".",
            DotDot => "..",
            DotDotDot => "...",
            DotDotEq => "..=",
            Comma => ",",
            Semi => ";",
            Colon => ":",
            RArrow => "->",
            LArrow => "<-",
            FatArrow => "=>",
            Pound => "#",
            Dollar => "$",
            Question => "?",
            OpenParen => "(",
            CloseParen => ")",
            OpenBrace => "{",
            CloseBrace => "}",
            OpenBracket => "[",
            CloseBracket => "]",
            Literal(lit) => return lit.to_string(),
            Ident(name, IdentIsRaw::No) => return name.to_string(),
            Ident(name, IdentIsRaw::Yes) => return format!("r#{name}"),
            Lifetime(name, IdentIsRaw::No) => return name.to_string(),
            Lifetime(name, IdentIsRaw::Yes) => return format!("'r#{}", &name.as_str()[1..]),
            DocComment(CommentKind::Line, AttrStyle::Outer, s) => return format!("///{s}"),
            DocComment(CommentKind::Line, AttrStyle::Inner, s) => return format!("//!{s}"),
            DocComment(CommentKind::Block, AttrStyle::Outer, s) => return format!("/**{s}*/"),
            DocComment(CommentKind::Block, AttrStyle::Inner, s) => return format!("/*!{s}*/"),
            Eof => "<eof>",
        };
        s.to_string()
    }
}

/// A cooked token.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// Whether a line feed came between the previous token and this one
    /// (exception 10). Comments in between don't hide it, and a block comment
    /// that contains a line feed counts as one (Go's rule).
    pub line_break_before: bool,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Token {
        Token { kind, span, line_break_before: false }
    }

    pub fn dummy() -> Token {
        Token::new(TokenKind::Eof, vsc_span::DUMMY_SP)
    }

    pub fn ident(&self) -> Option<(Ident, IdentIsRaw)> {
        match self.kind {
            TokenKind::Ident(name, is_raw) => Some((Ident::new(name, self.span), is_raw)),
            _ => None,
        }
    }

    pub fn lifetime(&self) -> Option<Ident> {
        match self.kind {
            TokenKind::Lifetime(name, _) => Some(Ident::new(name, self.span)),
            _ => None,
        }
    }

    pub fn is_ident(&self) -> bool {
        matches!(self.kind, TokenKind::Ident(..))
    }

    /// An identifier that isn't a keyword (raw identifiers included).
    pub fn is_non_keyword_ident(&self) -> bool {
        match self.kind {
            TokenKind::Ident(name, IdentIsRaw::Yes) => !name.is_empty(),
            TokenKind::Ident(name, IdentIsRaw::No) => !name.is_used_or_reserved_keyword(),
            _ => false,
        }
    }

    /// Is this the keyword `kw` (not written as a raw identifier)?
    pub fn is_keyword(&self, kw: Symbol) -> bool {
        matches!(self.kind, TokenKind::Ident(name, IdentIsRaw::No) if name == kw)
    }

    /// A strict or reserved keyword, not raw.
    pub fn is_used_or_reserved_keyword(&self) -> bool {
        matches!(self.kind, TokenKind::Ident(name, IdentIsRaw::No) if name.is_used_or_reserved_keyword())
    }

    pub fn is_path_segment_keyword(&self) -> bool {
        matches!(self.kind, TokenKind::Ident(name, IdentIsRaw::No) if name.is_path_segment_keyword())
    }

    /// A token that can start a path: an identifier, `self` or `Self`.
    pub fn is_path_start(&self) -> bool {
        self.is_non_keyword_ident() || self.is_path_segment_keyword() || self.kind == TokenKind::Lt
    }

    pub fn is_lit(&self) -> bool {
        matches!(self.kind, TokenKind::Literal(..))
    }

    /// A literal, `true`, `false`, or `-` (before a negative literal).
    pub fn can_begin_literal_maybe_minus(&self) -> bool {
        match self.kind {
            TokenKind::Literal(..) | TokenKind::Minus => true,
            TokenKind::Ident(name, IdentIsRaw::No) => name == kw::True || name == kw::False,
            _ => false,
        }
    }

    pub fn is_bool_lit(&self) -> bool {
        self.is_keyword(kw::True) || self.is_keyword(kw::False)
    }

    /// Could an expression start with this token?
    pub fn can_begin_expr(&self) -> bool {
        use TokenKind::*;
        match self.kind {
            Ident(name, is_raw) => ident_can_begin_expr(name, is_raw),
            OpenParen | OpenBrace | OpenBracket | Literal(..) | Bang | Minus | Star | And | AndAnd
            | DotDot | DotDotDot | DotDotEq | Lt | Shl | Lifetime(..) | At => true,
            _ => false,
        }
    }

    /// Could a type start with this token?
    pub fn can_begin_type(&self) -> bool {
        use TokenKind::*;
        match self.kind {
            Ident(name, is_raw) => ident_can_begin_type(name, is_raw),
            OpenParen | OpenBracket | Bang | Star | And | AndAnd | Question | Lt | Shl => true,
            _ => false,
        }
    }

    /// The token's text in backticks, for "expected …, found `x`" messages.
    pub fn to_descr(&self) -> String {
        match self.kind {
            TokenKind::Eof => "end of file".to_string(),
            TokenKind::Ident(name, IdentIsRaw::No) if name.is_strict_keyword() => format!("keyword `{name}`"),
            TokenKind::Ident(name, IdentIsRaw::No) if name.is_reserved_keyword() => {
                format!("reserved keyword `{name}`")
            }
            TokenKind::DocComment(..) => "doc comment".to_string(),
            kind => format!("`{}`", kind.as_string()),
        }
    }
}

fn ident_can_begin_expr(name: Symbol, is_raw: IdentIsRaw) -> bool {
    is_raw == IdentIsRaw::Yes
        || !name.is_used_or_reserved_keyword()
        || [
            kw::Async,
            kw::Break,
            kw::Const,
            kw::Continue,
            kw::False,
            kw::For,
            kw::Func,
            kw::If,
            kw::Let,
            kw::Loop,
            kw::Match,
            kw::Return,
            kw::SelfLower,
            kw::SelfUpper,
            kw::True,
            kw::Unsafe,
            kw::While,
            kw::Underscore,
        ]
        .contains(&name)
}

fn ident_can_begin_type(name: Symbol, is_raw: IdentIsRaw) -> bool {
    is_raw == IdentIsRaw::Yes
        || !name.is_used_or_reserved_keyword()
        || [
            kw::Underscore,
            kw::For,
            kw::Impl,
            kw::Func,
            kw::Unsafe,
            kw::Extern,
            kw::Typeof,
            kw::Dyn,
            kw::SelfUpper,
            kw::SelfLower,
        ]
        .contains(&name)
}

/// Whether the name is one the parser treats specially in expression
/// position (`vec[…]`, `asm(…)`).
pub fn is_builtin_with_own_syntax(name: Symbol) -> bool {
    name == sym::vec || name == sym::asm || name == sym::nakedAsm || name == sym::globalAsm
}
