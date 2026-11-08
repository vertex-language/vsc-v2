//! Source files (exception 1): `package name`, then imports, then items.

use vsc_ast::token::{IdentIsRaw, TokenKind};
use vsc_ast::{Import, PackageDecl, SourceFile};
use vsc_span::{Ident, kw};

use super::{PResult, Parser};

impl<'a> Parser<'a> {
    pub fn parse_source_file(&mut self) -> SourceFile {
        let lo = self.token.span;
        while self.eat(TokenKind::Semi) {}
        let inner_attrs = self.parse_inner_doc_comments();

        // Attributes before `package` belong to the package; otherwise they
        // belong to the first item, which `parse_item_list` will parse.
        let checkpoint = (self.pos, self.token, self.prev_token);
        let package = match self.parse_outer_attributes() {
            Ok(attrs) if self.is_package_decl() => match self.parse_package_decl(attrs) {
                Ok(decl) => Some(decl),
                Err(err) => {
                    err.emit();
                    self.recover_stmt();
                    None
                }
            },
            Ok(_) => {
                (self.pos, self.token, self.prev_token) = checkpoint;
                None
            }
            Err(err) => {
                err.cancel();
                (self.pos, self.token, self.prev_token) = checkpoint;
                None
            }
        };

        let mut imports = Vec::new();
        loop {
            while self.eat(TokenKind::Semi) {}
            if !self.check_keyword(kw::Import) {
                break;
            }
            match self.parse_import_decl() {
                Ok(mut specs) => {
                    imports.append(&mut specs);
                    if let Err(err) = self.expect_terminator("imports") {
                        err.emit();
                        self.recover_stmt();
                    }
                }
                Err(err) => {
                    err.emit();
                    self.recover_stmt();
                }
            }
        }

        let items = self.parse_item_list(false);
        SourceFile { inner_attrs, package, imports, items, span: lo.to(self.prev_token.span) }
    }

    /// `package name`, as opposed to an item with `package` visibility: the
    /// name is followed by the end of the line.
    fn is_package_decl(&self) -> bool {
        self.check_keyword(kw::Package)
            && self.look_ahead(1).is_non_keyword_ident()
            && {
                let after = self.look_ahead(2);
                after.line_break_before || matches!(after.kind, TokenKind::Semi | TokenKind::Eof)
            }
    }

    fn parse_package_decl(&mut self, attrs: vsc_ast::AttrVec) -> PResult<'a, PackageDecl> {
        let lo = attrs.first().map(|a| a.span).unwrap_or(self.token.span);
        self.expect_keyword(kw::Package)?;
        let name = self.parse_ident()?;
        let span = lo.to(self.prev_token.span);
        self.expect_terminator("declarations")?;
        Ok(PackageDecl { attrs, name, span })
    }

    /// `import "path"`, `import name "path"`, or `import ( … )`.
    pub fn parse_import_decl(&mut self) -> PResult<'a, Vec<Import>> {
        let lo = self.token.span;
        self.expect_keyword(kw::Import)?;
        if self.check(TokenKind::OpenParen) {
            let open_index = self.pos;
            self.bump();
            let specs = self.with_nl(false, |this| -> PResult<'a, _> {
                let mut specs = Vec::new();
                loop {
                    while this.eat(TokenKind::Semi) || this.eat(TokenKind::Comma) {}
                    if this.check(TokenKind::CloseParen) {
                        break;
                    }
                    match this.parse_import_spec(this.token.span) {
                        Ok(spec) => specs.push(spec),
                        Err(err) => {
                            err.emit();
                            this.jump_to_close_of(open_index);
                            break;
                        }
                    }
                }
                this.expect(TokenKind::CloseParen)?;
                Ok(specs)
            })?;
            return Ok(specs);
        }
        Ok(vec![self.parse_import_spec(lo)?])
    }

    /// `"path"` or `name "path"`.
    fn parse_import_spec(&mut self, lo: vsc_span::Span) -> PResult<'a, Import> {
        let rename = match self.token.ident() {
            Some((ident, IdentIsRaw::Yes)) => {
                self.bump();
                Some(ident)
            }
            Some((ident, IdentIsRaw::No)) if !ident.is_reserved() || ident.name == kw::Underscore => {
                self.bump();
                Some(Ident::new(ident.name, ident.span))
            }
            _ => None,
        };
        let Some(path) = self.parse_str_lit() else {
            let mut err = self.expected_found("an import path string");
            if self.check_path_segment_ident() || self.check(TokenKind::Dot) {
                err.help("import paths are strings: `import \"std/fs\"`");
            }
            return Err(err);
        };
        if path.symbol.as_str().is_empty() {
            self.dcx().span_err(path.span, "import path is empty");
        }
        Ok(Import { rename, path: path.symbol, path_span: path.span, span: lo.to(self.prev_token.span) })
    }
}
