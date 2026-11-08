use crate::unescape::{EscapeError, Mode, MixedUnit, unescape_byte, unescape_char, unescape_mixed, unescape_unicode};
use crate::*;

/// Renders tokens as `Kind "text"`, skipping whitespace.
fn lex(src: &str) -> Vec<String> {
    let mut pos = 0;
    let mut out = vec![];
    for tok in tokenize(src) {
        let text = &src[pos..pos + tok.len as usize];
        pos += tok.len as usize;
        if tok.kind != TokenKind::Whitespace {
            out.push(format!("{:?} {text:?}", tok.kind));
        }
    }
    out
}

#[test]
fn punctuation_is_single_chars() {
    assert_eq!(lex("a::b @x #"), vec![
        "Ident \"a\"",
        "Colon \":\"",
        "Colon \":\"",
        "Ident \"b\"",
        "At \"@\"",
        "Ident \"x\"",
        "Pound \"#\"",
    ]);
}

#[test]
fn comments_and_doc_comments() {
    let toks = lex("// plain\n/// outer\n//! inner\n//// plain\n/* a /* nested */ b */ /** doc */ /**/");
    assert_eq!(toks, vec![
        "LineComment { doc_style: None } \"// plain\"",
        "LineComment { doc_style: Some(Outer) } \"/// outer\"",
        "LineComment { doc_style: Some(Inner) } \"//! inner\"",
        "LineComment { doc_style: None } \"//// plain\"",
        "BlockComment { doc_style: None, terminated: true } \"/* a /* nested */ b */\"",
        "BlockComment { doc_style: Some(Outer), terminated: true } \"/** doc */\"",
        "BlockComment { doc_style: None, terminated: true } \"/**/\"",
    ]);
    assert_eq!(lex("/* open"), vec!["BlockComment { doc_style: None, terminated: false } \"/* open\""]);
}

#[test]
fn numbers() {
    let toks = lex("12 0x1F 0b101 0o17 1.5 2e10 1.5e-3 1uint8 2.5float32 0x 1..2 1.Max() x.0.1");
    assert_eq!(toks, vec![
        "Literal { kind: Int { base: Decimal, empty_int: false }, suffix_start: 2 } \"12\"",
        "Literal { kind: Int { base: Hexadecimal, empty_int: false }, suffix_start: 4 } \"0x1F\"",
        "Literal { kind: Int { base: Binary, empty_int: false }, suffix_start: 5 } \"0b101\"",
        "Literal { kind: Int { base: Octal, empty_int: false }, suffix_start: 4 } \"0o17\"",
        "Literal { kind: Float { base: Decimal, empty_exponent: false }, suffix_start: 3 } \"1.5\"",
        "Literal { kind: Float { base: Decimal, empty_exponent: false }, suffix_start: 4 } \"2e10\"",
        "Literal { kind: Float { base: Decimal, empty_exponent: false }, suffix_start: 6 } \"1.5e-3\"",
        "Literal { kind: Int { base: Decimal, empty_int: false }, suffix_start: 1 } \"1uint8\"",
        "Literal { kind: Float { base: Decimal, empty_exponent: false }, suffix_start: 3 } \"2.5float32\"",
        "Literal { kind: Int { base: Hexadecimal, empty_int: true }, suffix_start: 2 } \"0x\"",
        "Literal { kind: Int { base: Decimal, empty_int: false }, suffix_start: 1 } \"1\"",
        "Dot \".\"",
        "Dot \".\"",
        "Literal { kind: Int { base: Decimal, empty_int: false }, suffix_start: 1 } \"2\"",
        "Literal { kind: Int { base: Decimal, empty_int: false }, suffix_start: 1 } \"1\"",
        "Dot \".\"",
        "Ident \"Max\"",
        "OpenParen \"(\"",
        "CloseParen \")\"",
        "Ident \"x\"",
        "Dot \".\"",
        "Literal { kind: Float { base: Decimal, empty_exponent: false }, suffix_start: 3 } \"0.1\"",
    ]);
}

#[test]
fn strings_and_prefixes() {
    let toks = lex(r####"'a' '\n' b'x' "hi\"" b"by" c"cs" r"raw" r#"h"a"# br"rb" cr"rc" 'outer: r#func foo"x" 'ab'"####);
    assert_eq!(toks, vec![
        "Literal { kind: Char { terminated: true }, suffix_start: 3 } \"'a'\"",
        "Literal { kind: Char { terminated: true }, suffix_start: 4 } \"'\\\\n'\"",
        "Literal { kind: Byte { terminated: true }, suffix_start: 4 } \"b'x'\"",
        "Literal { kind: Str { terminated: true }, suffix_start: 6 } \"\\\"hi\\\\\\\"\\\"\"",
        "Literal { kind: ByteStr { terminated: true }, suffix_start: 5 } \"b\\\"by\\\"\"",
        "Literal { kind: CStr { terminated: true }, suffix_start: 5 } \"c\\\"cs\\\"\"",
        "Literal { kind: RawStr { n_hashes: Some(0) }, suffix_start: 6 } \"r\\\"raw\\\"\"",
        "Literal { kind: RawStr { n_hashes: Some(1) }, suffix_start: 8 } \"r#\\\"h\\\"a\\\"#\"",
        "Literal { kind: RawByteStr { n_hashes: Some(0) }, suffix_start: 6 } \"br\\\"rb\\\"\"",
        "Literal { kind: RawCStr { n_hashes: Some(0) }, suffix_start: 6 } \"cr\\\"rc\\\"\"",
        "Lifetime { starts_with_number: false } \"'outer\"",
        "Colon \":\"",
        "RawIdent \"r#func\"",
        "UnknownPrefix \"foo\"",
        "Literal { kind: Str { terminated: true }, suffix_start: 3 } \"\\\"x\\\"\"",
        "Literal { kind: Char { terminated: true }, suffix_start: 4 } \"'ab'\"",
    ]);
}

#[test]
fn unterminated_and_bad_raw_strings() {
    assert_eq!(lex("\"abc"), vec!["Literal { kind: Str { terminated: false }, suffix_start: 4 } \"\\\"abc\""]);
    assert_eq!(validate_raw_str("r##\"abc\"#", 1), Err(RawStrError::NoTerminator {
        expected: 2,
        found: 1,
        possible_terminator_offset: Some(7),
    }));
    assert_eq!(validate_raw_str("r#~", 1), Err(RawStrError::InvalidStarter { bad_char: '~' }));
    assert_eq!(validate_raw_str("r#\"ok\"#", 1), Ok(()));
}

#[test]
fn identifiers() {
    assert!(is_ident("camelCase"));
    assert!(is_ident("_x1"));
    assert!(is_ident("héllo"));
    assert!(is_ident("名前"));
    assert!(!is_ident("1x"));
    assert!(!is_ident(""));
    assert_eq!(lex("🦀"), vec!["InvalidIdent \"🦀\""]);
}

#[test]
fn shebang() {
    assert_eq!(strip_shebang("#!/usr/bin/env vsc\nfunc main() {}"), Some(18));
    assert_eq!(strip_shebang("#![allow]"), None);
    assert_eq!(strip_shebang("func main() {}"), None);
}

#[test]
fn line_feeds_are_whitespace_tokens() {
    let kinds: Vec<_> = tokenize("a\n  b").map(|t| (t.kind, t.len)).collect();
    assert_eq!(kinds, vec![(TokenKind::Ident, 1), (TokenKind::Whitespace, 3), (TokenKind::Ident, 1)]);
}

#[test]
fn escapes() {
    assert_eq!(unescape_char("a"), Ok('a'));
    assert_eq!(unescape_char("\\n"), Ok('\n'));
    assert_eq!(unescape_char("\\u{1F600}"), Ok('😀'));
    assert_eq!(unescape_char("\\x41"), Ok('A'));
    assert_eq!(unescape_char("\\xFF"), Err(EscapeError::OutOfRangeHexEscape));
    assert_eq!(unescape_char(""), Err(EscapeError::ZeroChars));
    assert_eq!(unescape_char("ab"), Err(EscapeError::MoreThanOneChar));
    assert_eq!(unescape_char("\\q"), Err(EscapeError::InvalidEscape));
    assert_eq!(unescape_char("\\u{D800}"), Err(EscapeError::LoneSurrogateUnicodeEscape));
    assert_eq!(unescape_char("\\u{1234567}"), Err(EscapeError::OverlongUnicodeEscape));
    assert_eq!(unescape_byte("\\xFF"), Ok(0xFF));
    assert_eq!(unescape_byte("é"), Err(EscapeError::NonAsciiCharInByte));
    assert_eq!(unescape_byte("\\u{41}"), Err(EscapeError::UnicodeEscapeInByte));
}

#[test]
fn string_escapes_and_continuations() {
    let mut out = String::new();
    let mut errs = vec![];
    unescape_unicode("a\\tb\\\n    c", Mode::Str, &mut |range, res| match res {
        Ok(c) => out.push(c),
        Err(e) => errs.push((range, e)),
    });
    assert_eq!(out, "a\tbc");
    assert!(errs.is_empty());

    let mut units = vec![];
    unescape_mixed("a\\xFF\\0", Mode::CStr, &mut |_, res| units.push(res));
    assert_eq!(units, vec![Ok(MixedUnit::Char('a')), Ok(MixedUnit::HighByte(0xFF)), Err(EscapeError::NulInCStr)]);

    let mut errs = vec![];
    unescape_unicode("a\rb", Mode::RawStr, &mut |range, res| {
        if let Err(e) = res {
            errs.push((range, e))
        }
    });
    assert_eq!(errs, vec![(1..2, EscapeError::BareCarriageReturnInRawString)]);
}
