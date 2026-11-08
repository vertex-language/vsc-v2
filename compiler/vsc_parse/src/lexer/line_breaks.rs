//! Exception 10: which line feeds end a statement.
//!
//! The grammar's rules, after Swift's:
//!
//! 1. A line feed is a terminator only where the grammar accepts one, that
//!    is, where the statement, declaration, field, variant or arm before it is
//!    complete. Inside `( )`, `[ ]` and comma-separated lists it is always
//!    whitespace. The parser tracks this: see `Parser::at_line_break`.
//! 2. Even there, the next line continues the previous one if it starts with
//!    `.`, with a binary operator followed by whitespace, with `as`, `where` or
//!    `else`, or with `{` right after a declaration header. This module
//!    implements that rule; declaration headers are handled by the item
//!    parser, which never treats a line feed before a body's `{` as an end.
//! 3. Comments don't count, and blank lines are one line feed (the lexer).
//! 4. `;` still separates things on one line (the parser).

use vsc_ast::token::{IdentIsRaw, Token, TokenKind};
use vsc_span::kw;

/// Rule 2: does a line that starts with `tok` (followed by `next`) continue the
/// line before it?
pub(crate) fn continues_line(tok: &Token, next: &Token) -> bool {
    use TokenKind::*;
    match tok.kind {
        // Method chains: `.Iter()` on its own line.
        Dot => true,
        Ident(name, IdentIsRaw::No) => name == kw::As || name == kw::Where || name == kw::Else,
        // A binary operator continues only when followed by whitespace: `- x`
        // continues, while `-x` starts a new statement (Swift's rule).
        Plus | Minus | Star | Slash | Percent | Caret | And | Or | Shl | Shr | EqEq | Ne | Lt | Le | Gt | Ge
        | AndAnd | OrOr | Eq | PlusEq | MinusEq | StarEq | SlashEq | PercentEq | CaretEq | AndEq | OrEq | ShlEq
        | ShrEq | DotDot | DotDotEq => followed_by_whitespace(tok, next),
        _ => false,
    }
}

fn followed_by_whitespace(tok: &Token, next: &Token) -> bool {
    next.line_break_before || next.span.lo() > tok.span.hi() || next.kind == TokenKind::Eof
}
