//! The Vertex parser (rustc's `rustc_parse`).
//!
//! [`parse_source_file`] lexes and parses one `.vs` file. Errors are reported
//! to the session's diagnostics context as they're found; the parser recovers
//! and returns as much of the file as it could read.

pub mod lexer;
pub mod parser;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;

use vsc_ast::SourceFile as AstSourceFile;
use vsc_ast::token::{Token, TokenKind};
use vsc_ast::tokenstream::TokenTree;
use vsc_session::ParseSess;
use vsc_span::{FileName, SourceFile};

pub use parser::Parser;

/// A parser over a loaded file.
pub fn new_parser_from_file<'a>(psess: &'a ParseSess, file: &Arc<SourceFile>) -> Parser<'a> {
    let buf = lexer::lex_token_trees(psess, &file.src, file.start_pos);
    Parser::new(psess, buf)
}

/// Loads `src` into the source map under `name` and parses it.
pub fn parse_source_str(psess: &ParseSess, name: FileName, src: String) -> AstSourceFile {
    let file = psess.source_map.new_source_file(name, src);
    parse_source_file(psess, &file)
}

/// Parses a file already in the source map.
pub fn parse_source_file(psess: &ParseSess, file: &Arc<SourceFile>) -> AstSourceFile {
    new_parser_from_file(psess, file).parse_source_file()
}

/// Reads and parses a file from disk.
pub fn parse_file(psess: &ParseSess, path: &Path) -> std::io::Result<AstSourceFile> {
    let file = psess.source_map.load_file(path)?;
    Ok(parse_source_file(psess, &file))
}

/// Parses comma-separated attributes (without `@`) from token trees, as
/// `@cfgAttr(predicate, attr, …)` holds them. Errors are reported and the
/// attributes that parsed are returned.
pub fn parse_attrs_from_tts(psess: &ParseSess, tts: &[TokenTree]) -> Vec<vsc_ast::Attribute> {
    fn flatten(tts: &[TokenTree], out: &mut Vec<Token>) {
        for tt in tts {
            match tt {
                TokenTree::Token(t) => out.push(*t),
                TokenTree::Delimited(dspan, delim, inner) => {
                    out.push(Token::new(TokenKind::open_for(*delim), dspan.open));
                    flatten(inner, out);
                    out.push(Token::new(TokenKind::close_for(*delim), dspan.close));
                }
            }
        }
    }
    let mut tokens = Vec::new();
    flatten(tts, &mut tokens);
    let end = tokens.last().map(|t| t.span.shrink_to_hi()).unwrap_or(vsc_span::DUMMY_SP);
    tokens.push(Token::new(TokenKind::Eof, end));
    let buf = lexer::balance_tokens(psess, tokens);
    let mut p = Parser::new(psess, buf);
    let mut attrs = Vec::new();
    while !p.check(TokenKind::Eof) {
        let lo = p.token.span;
        match p.parse_attr_item(lo) {
            Ok(a) => attrs.push(a),
            Err(e) => {
                e.emit();
                break;
            }
        }
        if !p.eat(TokenKind::Comma) {
            if !p.check(TokenKind::Eof) {
                p.expected_found("`,`").emit();
            }
            break;
        }
    }
    attrs
}
