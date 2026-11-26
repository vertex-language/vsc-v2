//! Matching delimiters (rustc's `lexer/tokentrees.rs`).
//!
//! rustc builds token trees here. vsc keeps the tokens flat and records where
//! each delimiter's partner is, which gives the parser the same structure:
//! it can jump over a group, or look at the token after a group's close.
//! Unbalanced delimiters are reported here, and the buffer is repaired so
//! the parser always sees balanced groups.

use vsc_ast::token::{Delimiter, Token, TokenKind};
use vsc_session::ParseSess;

/// Every token of a file, ending with `Eof`, with matching delimiters linked.
pub(crate) struct TokenBuffer {
    pub tokens: Vec<Token>,
    /// For an opening delimiter, the index of its closer, and the other way
    /// round. `u32::MAX` for other tokens.
    pub matching: Vec<u32>,
}

impl TokenBuffer {
    pub fn partner(&self, index: usize) -> Option<usize> {
        let m = *self.matching.get(index)?;
        (m != u32::MAX).then_some(m as usize)
    }
}

pub(crate) fn balance(psess: &ParseSess, input: Vec<Token>) -> TokenBuffer {
    let mut tokens: Vec<Token> = Vec::with_capacity(input.len());
    // Open delimiters waiting for a closer: (index in `tokens`, delimiter).
    let mut stack: Vec<(usize, Delimiter)> = Vec::new();

    for tok in input {
        if let Some(delim) = tok.kind.open_delim() {
            stack.push((tokens.len(), delim));
            tokens.push(tok);
        } else if let Some(delim) = tok.kind.close_delim() {
            match stack.last() {
                Some(&(_, open)) if open == delim => {
                    stack.pop();
                    tokens.push(tok);
                }
                _ if stack.iter().any(|&(_, d)| d == delim) => {
                    // Some groups inside this one were never closed. Report
                    // the innermost, and close them all here.
                    let &(open_index, _) = stack.last().unwrap();
                    let open_span = tokens[open_index].span;
                    psess
                        .dcx
                        .struct_span_err(tok.span, format!("mismatched closing delimiter: `{}`", delim.close()))
                        .with_span_label(tok.span, "mismatched closing delimiter")
                        .with_span_label(open_span, "unclosed delimiter")
                        .emit();
                    while let Some((_, d)) = stack.pop() {
                        if d == delim {
                            break;
                        }
                        tokens.push(Token::new(TokenKind::close_for(d), tok.span.shrink_to_lo()));
                    }
                    tokens.push(tok);
                }
                _ => {
                    psess
                        .dcx
                        .struct_span_err(tok.span, format!("unexpected closing delimiter: `{}`", delim.close()))
                        .with_span_label(tok.span, "unexpected closing delimiter")
                        .emit();
                }
            }
        } else if tok.kind == TokenKind::Eof {
            if !stack.is_empty() {
                let mut err = psess.dcx.struct_span_err(tok.span, "this file contains an unclosed delimiter");
                for &(index, _) in &stack {
                    err.span_label(tokens[index].span, "unclosed delimiter");
                }
                err.emit();
                while let Some((_, d)) = stack.pop() {
                    tokens.push(Token::new(TokenKind::close_for(d), tok.span));
                }
            }
            tokens.push(tok);
        } else {
            tokens.push(tok);
        }
    }

    let mut matching = vec![u32::MAX; tokens.len()];
    let mut opens = Vec::new();
    for (i, tok) in tokens.iter().enumerate() {
        if tok.kind.open_delim().is_some() {
            opens.push(i);
        } else if tok.kind.close_delim().is_some() {
            let open = opens.pop().expect("token buffer is balanced");
            matching[open] = i as u32;
            matching[i] = open as u32;
        }
    }
    TokenBuffer { tokens, matching }
}
