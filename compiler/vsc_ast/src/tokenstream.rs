//! Token trees: tokens grouped by their delimiters. Attribute arguments
//! (`@derive(Debug, Clone)`) are kept as token trees until
//! `vsc_attr_parsing` reads them.

use vsc_span::Span;

use crate::token::{Delimiter, Token};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DelimSpan {
    pub open: Span,
    pub close: Span,
}

impl DelimSpan {
    pub fn entire(self) -> Span {
        self.open.to(self.close)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum TokenTree {
    Token(Token),
    Delimited(DelimSpan, Delimiter, Vec<TokenTree>),
}

impl TokenTree {
    pub fn span(&self) -> Span {
        match self {
            TokenTree::Token(t) => t.span,
            TokenTree::Delimited(d, ..) => d.entire(),
        }
    }
}
