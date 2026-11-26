//! Unicode chars that look like ASCII punctuation (rustc's
//! `lexer/unicode_chars.rs`, with a shorter table).

use vsc_ast::token::TokenKind;
use vsc_errors::Applicability;
use vsc_session::ParseSess;
use vsc_span::Span;

/// (confusable char, its name, the ASCII char it looks like)
const CONFUSABLES: &[(char, &str, char)] = &[
    ('\u{037E}', "Greek Question Mark", ';'),
    ('\u{FF1B}', "Fullwidth Semicolon", ';'),
    ('\u{FF0C}', "Fullwidth Comma", ','),
    ('\u{3001}', "Ideographic Comma", ','),
    ('\u{FF1A}', "Fullwidth Colon", ':'),
    ('\u{FF08}', "Fullwidth Left Parenthesis", '('),
    ('\u{FF09}', "Fullwidth Right Parenthesis", ')'),
    ('\u{FF5B}', "Fullwidth Left Curly Bracket", '{'),
    ('\u{FF5D}', "Fullwidth Right Curly Bracket", '}'),
    ('\u{FF3B}', "Fullwidth Left Square Bracket", '['),
    ('\u{FF3D}', "Fullwidth Right Square Bracket", ']'),
    ('\u{FF0E}', "Fullwidth Full Stop", '.'),
    ('\u{3002}', "Ideographic Full Stop", '.'),
    ('\u{FF1D}', "Fullwidth Equals Sign", '='),
    ('\u{FF1C}', "Fullwidth Less-Than Sign", '<'),
    ('\u{FF1E}', "Fullwidth Greater-Than Sign", '>'),
    ('\u{2212}', "Minus Sign", '-'),
    ('\u{2010}', "Hyphen", '-'),
    ('\u{2013}', "En Dash", '-'),
    ('\u{2014}', "Em Dash", '-'),
    ('\u{FF0B}', "Fullwidth Plus Sign", '+'),
    ('\u{00D7}', "Multiplication Sign", '*'),
    ('\u{2217}', "Asterisk Operator", '*'),
    ('\u{FF01}', "Fullwidth Exclamation Mark", '!'),
    ('\u{01C3}', "Latin Letter Retroflex Click", '!'),
    ('\u{FF1F}', "Fullwidth Question Mark", '?'),
    ('\u{FF20}', "Fullwidth Commercial At", '@'),
    ('\u{FF06}', "Fullwidth Ampersand", '&'),
    ('\u{FF5C}', "Fullwidth Vertical Line", '|'),
    ('\u{2018}', "Left Single Quotation Mark", '\''),
    ('\u{2019}', "Right Single Quotation Mark", '\''),
    ('\u{201C}', "Left Double Quotation Mark", '"'),
    ('\u{201D}', "Right Double Quotation Mark", '"'),
    ('\u{00A0}', "No-Break Space", ' '),
    ('\u{3000}', "Ideographic Space", ' '),
    ('\u{200B}', "Zero Width Space", ' '),
    ('\u{FEFF}', "Zero Width No-Break Space", ' '),
];

/// Reports a confusable char and returns the token it was probably meant to be.
pub(crate) fn check_for_substitution(psess: &ParseSess, span: Span, c: char) -> Option<TokenKind> {
    let &(_, name, ascii) = CONFUSABLES.iter().find(|(ch, ..)| *ch == c)?;
    let ascii_name = match ascii {
        ' ' => "Space".to_string(),
        '\'' => "Single Quote".to_string(),
        '"' => "Quotation Mark".to_string(),
        other => format!("'{other}'"),
    };
    psess
        .dcx
        .struct_span_err(span, format!("unknown start of token: {}", super::escaped_char(c)))
        .with_span_suggestion(
            span,
            format!(
                "Unicode character '{c}' ({name}) looks like {ascii_name}, but it is not"
            ),
            ascii.to_string(),
            Applicability::MaybeIncorrect,
        )
        .emit();
    let kind = match ascii {
        ';' => TokenKind::Semi,
        ',' => TokenKind::Comma,
        ':' => TokenKind::Colon,
        '(' => TokenKind::OpenParen,
        ')' => TokenKind::CloseParen,
        '{' => TokenKind::OpenBrace,
        '}' => TokenKind::CloseBrace,
        '[' => TokenKind::OpenBracket,
        ']' => TokenKind::CloseBracket,
        '.' => TokenKind::Dot,
        '=' => TokenKind::Eq,
        '<' => TokenKind::Lt,
        '>' => TokenKind::Gt,
        '-' => TokenKind::Minus,
        '+' => TokenKind::Plus,
        '*' => TokenKind::Star,
        '!' => TokenKind::Bang,
        '?' => TokenKind::Question,
        '@' => TokenKind::At,
        '&' => TokenKind::And,
        '|' => TokenKind::Or,
        // Quotes and spaces can't be turned into a token here.
        _ => return None,
    };
    Some(kind)
}
