//! Error messages shared by the parser.

use vsc_errors::Diag;
use vsc_span::{Symbol, kw};

use super::Parser;

impl<'a> Parser<'a> {
    /// `expected {what}, found {token}`, labeled at the token.
    pub fn expected_found(&self, what: &str) -> Diag<'a> {
        let found = self.token.to_descr();
        let mut err = self.dcx().struct_span_err(self.token.span, format!("expected {what}, found {found}"));
        err.span_label(self.token.span, format!("expected {what}"));
        if let Some(name) = self.token.ident().map(|(i, _)| i.name) {
            if let Some(hint) = rust_keyword_hint(name) {
                err.help(hint);
            }
        }
        if self.token.kind == vsc_ast::token::TokenKind::Pound {
            err.help("Vertex attributes are written `@name`, not `#[name]`");
        }
        err
    }

    pub fn struct_span_err(&self, span: vsc_span::Span, msg: impl Into<String>) -> Diag<'a> {
        self.dcx().struct_span_err(span, msg)
    }
}

/// Help for Rust spellings Vertex replaced.
pub(crate) fn rust_keyword_hint(name: Symbol) -> Option<&'static str> {
    Some(match name {
        n if n == kw::Fn => "Vertex declares functions with `func` (exception 4)",
        n if n == kw::Pub => "Vertex writes `public` for `pub`, and `package` for `pub(crate)` (exception 6)",
        n if n == kw::Mod => "a Vertex package is a folder; there is no `mod` (exception 1)",
        n if n == kw::Crate || n == kw::Super => {
            "Vertex imports packages by path; there is no `crate` or `super` path (exception 1)"
        }
        n if n == kw::Move => "Vertex has no `move`: a function literal that escapes takes its captures (exception 14)",
        n if n == kw::Use => "Vertex imports with `import \"path\"` at the top of the file (exception 1)",
        _ => return None,
    })
}
