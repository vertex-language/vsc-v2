//! Escape sequences in char, byte and string literals.
//!
//! The rules are Rust's: `\n \r \t \\ \0 \' \"`, `\xNN` (ASCII only outside
//! byte literals), `\u{…}` (not in byte literals), and `\` at the end of a line
//! to skip the line feed and the next line's leading whitespace.

use std::ops::Range;
use std::str::Chars;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeError {
    /// `''`.
    ZeroChars,
    /// `'ab'`.
    MoreThanOneChar,
    /// A `\` at the end of a char literal.
    LoneSlash,
    /// `\q`.
    InvalidEscape,
    /// A raw `\r`.
    BareCarriageReturn,
    /// A raw `\r` in a raw string.
    BareCarriageReturnInRawString,
    /// A char that must be escaped: `'''`, or a tab or line feed in a char.
    EscapeOnlyChar,
    /// `\x1`.
    TooShortHexEscape,
    /// `\xz1`.
    InvalidCharInHexEscape,
    /// `\xFF` outside a byte literal.
    OutOfRangeHexEscape,
    /// `ሴ`.
    NoBraceInUnicodeEscape,
    /// `\u{1z}`.
    InvalidCharInUnicodeEscape,
    /// `\u{}`.
    EmptyUnicodeEscape,
    /// `\u{1234`.
    UnclosedUnicodeEscape,
    /// `\u{_12}`.
    LeadingUnderscoreUnicodeEscape,
    /// More than six hex digits: `\u{1234567}`.
    OverlongUnicodeEscape,
    /// `\u{D800}`.
    LoneSurrogateUnicodeEscape,
    /// `\u{FFFFFF}`.
    OutOfRangeUnicodeEscape,
    /// `\u{…}` in a byte literal.
    UnicodeEscapeInByte,
    /// A non-ASCII char in a byte literal.
    NonAsciiCharInByte,
    /// A NUL in a C string.
    NulInCStr,
    /// Whitespace after a line continuation that isn't skipped.
    UnskippedWhitespaceWarning,
    /// A line continuation that skips more than one line.
    MultipleSkippedLinesWarning,
}

impl EscapeError {
    pub fn is_fatal(self) -> bool {
        !matches!(self, EscapeError::UnskippedWhitespaceWarning | EscapeError::MultipleSkippedLinesWarning)
    }
}

/// Which kind of literal is being unescaped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Char,
    Byte,
    Str,
    RawStr,
    ByteStr,
    RawByteStr,
    CStr,
    RawCStr,
}

impl Mode {
    pub fn in_double_quotes(self) -> bool {
        !matches!(self, Mode::Char | Mode::Byte)
    }

    /// `\x80`–`\xFF` allowed.
    fn allow_high_bytes(self) -> bool {
        matches!(self, Mode::Byte | Mode::ByteStr | Mode::RawByteStr | Mode::CStr | Mode::RawCStr)
    }

    /// Non-ASCII chars allowed unescaped.
    fn allow_unicode_chars(self) -> bool {
        !matches!(self, Mode::Byte | Mode::ByteStr | Mode::RawByteStr)
    }

    /// `\u{…}` allowed.
    fn allow_unicode_escapes(self) -> bool {
        !matches!(self, Mode::Byte | Mode::ByteStr | Mode::RawByteStr)
    }
}

/// One unit of a C string: a char, or a byte above 0x7F written as `\xNN`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MixedUnit {
    Char(char),
    HighByte(u8),
}

/// Unescapes a char, byte, string or byte string literal's contents. In byte
/// modes each byte comes back as the char with that value. `callback` gets
/// the byte range of each unit in `src`.
pub fn unescape_unicode(src: &str, mode: Mode, callback: &mut impl FnMut(Range<usize>, Result<char, EscapeError>)) {
    match mode {
        Mode::Char | Mode::Byte => {
            let mut chars = src.chars();
            let res = unescape_char_or_byte(&mut chars, mode);
            callback(0..(src.len() - chars.as_str().len()), res);
        }
        Mode::Str | Mode::ByteStr => unescape_non_raw_common(src, mode, &mut |range, res| {
            callback(range, res.map(|unit| match unit {
                MixedUnit::Char(c) => c,
                MixedUnit::HighByte(b) => char::from(b),
            }))
        }),
        Mode::RawStr | Mode::RawByteStr => check_raw_common(src, mode, callback),
        Mode::CStr | Mode::RawCStr => panic!("C strings are unescaped with unescape_mixed"),
    }
}

/// Unescapes a C string literal's contents.
pub fn unescape_mixed(src: &str, mode: Mode, callback: &mut impl FnMut(Range<usize>, Result<MixedUnit, EscapeError>)) {
    match mode {
        Mode::CStr => unescape_non_raw_common(src, mode, &mut |range, res| {
            let res = res.and_then(|unit| match unit {
                MixedUnit::Char('\0') => Err(EscapeError::NulInCStr),
                unit => Ok(unit),
            });
            callback(range, res)
        }),
        Mode::RawCStr => check_raw_common(src, mode, &mut |range, res| {
            let res = res.and_then(|c| if c == '\0' { Err(EscapeError::NulInCStr) } else { Ok(MixedUnit::Char(c)) });
            callback(range, res)
        }),
        _ => panic!("unescape_mixed is for C strings"),
    }
}

/// The value of a char literal's contents (`a`, `\n`, `\u{1F600}`).
pub fn unescape_char(src: &str) -> Result<char, EscapeError> {
    unescape_char_or_byte(&mut src.chars(), Mode::Char)
}

/// The value of a byte literal's contents.
pub fn unescape_byte(src: &str) -> Result<u8, EscapeError> {
    unescape_char_or_byte(&mut src.chars(), Mode::Byte).map(|c| c as u8)
}

fn unescape_char_or_byte(chars: &mut Chars<'_>, mode: Mode) -> Result<char, EscapeError> {
    let c = chars.next().ok_or(EscapeError::ZeroChars)?;
    let res = match c {
        '\\' => scan_escape(chars, mode).map(|unit| match unit {
            MixedUnit::Char(c) => c,
            MixedUnit::HighByte(b) => char::from(b),
        }),
        '\n' | '\t' | '\'' => Err(EscapeError::EscapeOnlyChar),
        '\r' => Err(EscapeError::BareCarriageReturn),
        _ => ascii_check(c, !mode.allow_unicode_chars()),
    }?;
    if chars.next().is_some() {
        return Err(EscapeError::MoreThanOneChar);
    }
    Ok(res)
}

/// After a `\`.
fn scan_escape(chars: &mut Chars<'_>, mode: Mode) -> Result<MixedUnit, EscapeError> {
    let c = match chars.next().ok_or(EscapeError::LoneSlash)? {
        '"' => '"',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        '\\' => '\\',
        '\'' => '\'',
        '0' => '\0',
        'x' => {
            let hi = chars.next().ok_or(EscapeError::TooShortHexEscape)?;
            let hi = hi.to_digit(16).ok_or(EscapeError::InvalidCharInHexEscape)?;
            let lo = chars.next().ok_or(EscapeError::TooShortHexEscape)?;
            let lo = lo.to_digit(16).ok_or(EscapeError::InvalidCharInHexEscape)?;
            let value = (hi * 16 + lo) as u8;
            return if value.is_ascii() {
                Ok(MixedUnit::Char(char::from(value)))
            } else if mode.allow_high_bytes() {
                Ok(MixedUnit::HighByte(value))
            } else {
                Err(EscapeError::OutOfRangeHexEscape)
            };
        }
        'u' => return scan_unicode(chars, mode.allow_unicode_escapes()).map(MixedUnit::Char),
        _ => return Err(EscapeError::InvalidEscape),
    };
    Ok(MixedUnit::Char(c))
}

/// After `\u`: `{` hex digits (with `_`) `}`.
fn scan_unicode(chars: &mut Chars<'_>, allow_unicode_escapes: bool) -> Result<char, EscapeError> {
    if chars.next() != Some('{') {
        return Err(EscapeError::NoBraceInUnicodeEscape);
    }
    let mut n_digits = 1;
    let mut value: u32 = match chars.next().ok_or(EscapeError::UnclosedUnicodeEscape)? {
        '_' => return Err(EscapeError::LeadingUnderscoreUnicodeEscape),
        '}' => return Err(EscapeError::EmptyUnicodeEscape),
        c => c.to_digit(16).ok_or(EscapeError::InvalidCharInUnicodeEscape)?,
    };
    loop {
        match chars.next() {
            None => return Err(EscapeError::UnclosedUnicodeEscape),
            Some('_') => continue,
            Some('}') => {
                if n_digits > 6 {
                    return Err(EscapeError::OverlongUnicodeEscape);
                }
                if !allow_unicode_escapes {
                    return Err(EscapeError::UnicodeEscapeInByte);
                }
                return char::from_u32(value).ok_or(if value > 0x10FFFF {
                    EscapeError::OutOfRangeUnicodeEscape
                } else {
                    EscapeError::LoneSurrogateUnicodeEscape
                });
            }
            Some(c) => {
                let digit = c.to_digit(16).ok_or(EscapeError::InvalidCharInUnicodeEscape)?;
                n_digits += 1;
                if n_digits > 6 {
                    // Keep going to report the right error at the `}`.
                    continue;
                }
                value = value * 16 + digit;
            }
        }
    }
}

fn ascii_check(c: char, characters_should_be_ascii: bool) -> Result<char, EscapeError> {
    if characters_should_be_ascii && !c.is_ascii() { Err(EscapeError::NonAsciiCharInByte) } else { Ok(c) }
}

fn unescape_non_raw_common(
    src: &str,
    mode: Mode,
    callback: &mut impl FnMut(Range<usize>, Result<MixedUnit, EscapeError>),
) {
    let mut chars = src.chars();
    let allow_unicode_chars = mode.allow_unicode_chars();
    while let Some(c) = chars.next() {
        let start = src.len() - chars.as_str().len() - c.len_utf8();
        let res = match c {
            '\\' => match chars.clone().next() {
                Some('\n') => {
                    skip_ascii_whitespace(&mut chars, start, &mut |range, err| callback(range, Err(err)));
                    continue;
                }
                _ => scan_escape(&mut chars, mode),
            },
            '"' => Err(EscapeError::EscapeOnlyChar),
            '\r' => Err(EscapeError::BareCarriageReturn),
            _ => ascii_check(c, !allow_unicode_chars).map(MixedUnit::Char),
        };
        let end = src.len() - chars.as_str().len();
        callback(start..end, res);
    }
}

/// After a `\` at the end of a line: skips the line feed and the next line's
/// leading ASCII whitespace.
fn skip_ascii_whitespace(chars: &mut Chars<'_>, start: usize, callback: &mut impl FnMut(Range<usize>, EscapeError)) {
    let tail = chars.as_str();
    let first_non_space =
        tail.bytes().position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r')).unwrap_or(tail.len());
    if tail[1..first_non_space].contains('\n') {
        let end = start + first_non_space + 1;
        callback(start..end, EscapeError::MultipleSkippedLinesWarning);
    }
    let tail = &tail[first_non_space..];
    if let Some(c) = tail.chars().next() {
        if c.is_whitespace() {
            let end = start + first_non_space + c.len_utf8() + 1;
            callback(start..end, EscapeError::UnskippedWhitespaceWarning);
        }
    }
    *chars = tail.chars();
}

/// Raw strings have no escapes; only bare `\r` and, in byte strings,
/// non-ASCII chars are errors.
fn check_raw_common(src: &str, mode: Mode, callback: &mut impl FnMut(Range<usize>, Result<char, EscapeError>)) {
    let mut chars = src.chars();
    let allow_unicode_chars = mode.allow_unicode_chars();
    while let Some(c) = chars.next() {
        let start = src.len() - chars.as_str().len() - c.len_utf8();
        let res = match c {
            '\r' => Err(EscapeError::BareCarriageReturnInRawString),
            _ => ascii_check(c, !allow_unicode_chars),
        };
        let end = src.len() - chars.as_str().len();
        callback(start..end, res);
    }
}
