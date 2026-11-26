//! Messages for bad escapes in literals.

use std::ops::Range;

use vsc_errors::Applicability;
use vsc_lexer::unescape::{EscapeError, Mode};
use vsc_session::ParseSess;
use vsc_span::Span;

pub(crate) fn emit_unescape_error(
    psess: &ParseSess,
    lit: &str,
    full_lit_span: Span,
    err_span: Span,
    mode: Mode,
    range: Range<usize>,
    error: EscapeError,
) {
    let dcx = &psess.dcx;
    let escaped = &lit[range.clone()];
    let last_char = escaped.chars().last().unwrap_or(' ');
    let what = match mode {
        Mode::Char => "character literal",
        Mode::Byte => "byte literal",
        Mode::ByteStr | Mode::RawByteStr => "byte string literal",
        Mode::CStr | Mode::RawCStr => "C string literal",
        Mode::Str | Mode::RawStr => "string literal",
    };
    match error {
        EscapeError::LoneSlash => {
            dcx.struct_span_err(err_span, "invalid trailing slash in literal")
                .with_span_label(err_span, "invalid trailing slash in literal")
                .emit();
        }
        EscapeError::MoreThanOneChar => {
            let mut err = dcx.struct_span_err(full_lit_span, format!("{what} may only contain one codepoint"));
            if mode == Mode::Char {
                err.span_suggestion(
                    full_lit_span,
                    "if you meant to write a string literal, use double quotes",
                    format!("\"{lit}\""),
                    Applicability::MachineApplicable,
                );
            }
            err.emit();
        }
        EscapeError::EscapeOnlyChar => {
            let c = lit[range].chars().next().unwrap();
            let shown = match c {
                '\n' => "\\n".to_string(),
                '\t' => "\\t".to_string(),
                c => c.to_string(),
            };
            let escaped_form = match c {
                '\n' => "\\n".to_string(),
                '\t' => "\\t".to_string(),
                c => format!("\\{c}"),
            };
            dcx.struct_span_err(err_span, format!("character constant must be escaped: `{shown}`"))
                .with_span_suggestion(
                    err_span,
                    "escape the character",
                    escaped_form,
                    Applicability::MachineApplicable,
                )
                .emit();
        }
        EscapeError::BareCarriageReturn => {
            dcx.struct_span_err(err_span, format!("bare CR not allowed in {what}"))
                .with_help("escape it as `\\r`")
                .emit();
        }
        EscapeError::BareCarriageReturnInRawString => {
            dcx.struct_span_err(err_span, "bare CR not allowed in raw string").emit();
        }
        EscapeError::InvalidEscape => {
            let label = if matches!(mode, Mode::Byte | Mode::ByteStr) {
                "unknown byte escape"
            } else {
                "unknown character escape"
            };
            dcx.struct_span_err(err_span, format!("{label}: `{}`", last_char.escape_default()))
                .with_span_label(err_span, label)
                .with_help("valid escapes are `\\n`, `\\r`, `\\t`, `\\\\`, `\\0`, `\\'`, `\\\"`, `\\xNN` and `\\u{…}`")
                .emit();
        }
        EscapeError::TooShortHexEscape => {
            dcx.span_err(err_span, "numeric character escape is too short");
        }
        EscapeError::InvalidCharInHexEscape | EscapeError::InvalidCharInUnicodeEscape => {
            let kind = if error == EscapeError::InvalidCharInHexEscape { "numeric" } else { "unicode" };
            dcx.struct_span_err(err_span, format!("invalid character in {kind} character escape: `{}`", last_char.escape_default()))
                .emit();
        }
        EscapeError::NonAsciiCharInByte => {
            dcx.struct_span_err(err_span, format!("non-ASCII character in {what}"))
                .with_span_label(err_span, format!("must be ASCII"))
                .with_help("use a `\\xNN` escape for the byte you want")
                .emit();
        }
        EscapeError::OutOfRangeHexEscape => {
            dcx.struct_span_err(err_span, "out of range hex escape")
                .with_span_label(err_span, "must be a character in the range [\\x00-\\x7f]")
                .emit();
        }
        EscapeError::LeadingUnderscoreUnicodeEscape => {
            dcx.span_err(err_span, "invalid start of unicode escape: `_`");
        }
        EscapeError::OverlongUnicodeEscape => {
            dcx.struct_span_err(err_span, "overlong unicode escape")
                .with_span_label(err_span, "must have at most 6 hex digits")
                .emit();
        }
        EscapeError::UnclosedUnicodeEscape => {
            dcx.struct_span_err(err_span, "unterminated unicode escape")
                .with_span_label(err_span, "missing a closing `}`")
                .emit();
        }
        EscapeError::NoBraceInUnicodeEscape => {
            dcx.struct_span_err(err_span, "incorrect unicode escape sequence")
                .with_help("format of unicode escape sequences is `\\u{…}`")
                .emit();
        }
        EscapeError::UnicodeEscapeInByte => {
            dcx.struct_span_err(err_span, format!("unicode escape in {what}"))
                .with_span_label(err_span, format!("unicode escape in {what}"))
                .with_help(format!("unicode escape sequences cannot be used as a byte or in a byte string"))
                .emit();
        }
        EscapeError::EmptyUnicodeEscape => {
            dcx.struct_span_err(err_span, "empty unicode escape")
                .with_span_label(err_span, "this escape must have at least 1 hex digit")
                .emit();
        }
        EscapeError::ZeroChars => {
            dcx.span_err(err_span, "empty character literal");
        }
        EscapeError::LoneSurrogateUnicodeEscape => {
            dcx.struct_span_err(err_span, "invalid unicode character escape")
                .with_span_label(err_span, "invalid escape")
                .with_help("unicode escape must not be a surrogate")
                .emit();
        }
        EscapeError::OutOfRangeUnicodeEscape => {
            dcx.struct_span_err(err_span, "invalid unicode character escape")
                .with_span_label(err_span, "invalid escape")
                .with_help("unicode escape must be at most 10FFFF")
                .emit();
        }
        EscapeError::NulInCStr => {
            dcx.span_err(err_span, "null characters in C string literals are not supported");
        }
        EscapeError::UnskippedWhitespaceWarning => {
            dcx.struct_span_warn(err_span, "whitespace symbol is not skipped")
                .with_span_label(err_span, "whitespace symbol is not skipped")
                .emit();
        }
        EscapeError::MultipleSkippedLinesWarning => {
            dcx.struct_span_warn(err_span, "multiple lines skipped by escaped newline")
                .with_span_label(err_span, "skipping everything up to and including this point")
                .emit();
        }
    }
}
