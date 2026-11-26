//! Pretty-printing Vertex AST nodes back to canonical Vertex source text.

mod expr;
mod item;
mod ty_pat;

use vsc_ast::token::{AttrStyle, CommentKind, TokenKind};
use vsc_ast::tokenstream::TokenTree;
use vsc_ast::*;
use vsc_span::{Ident, kw};

pub fn source_file_to_string(file: &SourceFile) -> String {
    let mut p = Printer::new();
    p.source_file(file);
    p.finish()
}

pub fn item_to_string(item: &Item) -> String {
    let mut p = Printer::new();
    p.item(item);
    p.finish()
}

pub fn expr_to_string(e: &Expr) -> String {
    let mut p = Printer::new();
    p.expr(e);
    p.out
}

pub fn ty_to_string(t: &Ty) -> String {
    let mut p = Printer::new();
    p.ty(t);
    p.out
}

pub fn pat_to_string(pat: &Pat) -> String {
    let mut p = Printer::new();
    p.pat(pat);
    p.out
}

/// One attribute, as written: `@inline`, `/// text`.
pub fn attribute_to_string(a: &Attribute) -> String {
    let mut p = Printer::new();
    p.attr(a);
    p.out
}

pub fn path_to_string(path: &Path) -> String {
    let mut p = Printer::new();
    p.path(path, true);
    p.out
}

struct Printer {
    out: String,
    indent: usize,
}

impl Printer {
    fn new() -> Printer {
        Printer { out: String::new(), indent: 0 }
    }

    fn finish(mut self) -> String {
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
        if !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        self.out
    }

    fn w(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn nl(&mut self) {
        // No trailing whitespace on the line we're ending.
        while self.out.ends_with(' ') {
            self.out.pop();
        }
        self.out.push('\n');
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
    }

    fn ident(&mut self, i: Ident) {
        if i.name.is_used_or_reserved_keyword() && !i.name.is_path_segment_keyword() && i.name != kw::Underscore {
            self.w("r#");
        }
        self.w(i.name.as_str());
    }

    fn comma_sep<T>(&mut self, items: &[T], mut f: impl FnMut(&mut Self, &T)) {
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.w(", ");
            }
            f(self, item);
        }
    }

    // -----------------------------------------------------------------------
    // Files and attributes
    // -----------------------------------------------------------------------

    fn source_file(&mut self, file: &SourceFile) {
        for a in &file.inner_attrs {
            self.attr(a);
            self.nl();
        }
        if let Some(pkg) = &file.package {
            for a in &pkg.attrs {
                self.attr(a);
                self.nl();
            }
            self.w("package ");
            self.ident(pkg.name);
            self.nl();
        }
        if !file.imports.is_empty() {
            if file.package.is_some() {
                self.nl();
            }
            for imp in &file.imports {
                self.w("import ");
                self.import(imp);
                self.nl();
            }
        }
        for (i, item) in file.items.iter().enumerate() {
            if i > 0 || file.package.is_some() || !file.imports.is_empty() {
                self.nl();
            }
            self.item(item);
            self.nl();
        }
    }

    fn import(&mut self, imp: &Import) {
        if let Some(name) = imp.rename {
            self.ident(name);
            if imp.items.is_some() {
                self.w(", ");
            }
        }
        if let Some(items) = &imp.items {
            if items.is_empty() {
                self.w("{}");
            } else {
                self.w("{ ");
                self.comma_sep(items, |p, item| {
                    p.ident(item.ident);
                    if let Some(rename) = item.rename {
                        p.w(" as ");
                        p.ident(rename);
                    }
                });
                self.w(" }");
            }
        }
        if imp.rename.is_some() || imp.items.is_some() {
            self.w(" from ");
        }
        self.w("\"");
        self.w(imp.path.as_str());
        self.w("\"");
    }

    fn attrs(&mut self, attrs: &[Attribute]) {
        for a in attrs {
            self.attr(a);
            self.nl();
        }
    }

    fn attr(&mut self, a: &Attribute) {
        match &a.kind {
            AttrKind::DocComment(CommentKind::Line, text) => {
                self.w(if a.style == AttrStyle::Inner { "//!" } else { "///" });
                self.w(text.as_str());
            }
            AttrKind::DocComment(CommentKind::Block, text) => {
                self.w(if a.style == AttrStyle::Inner { "/*!" } else { "/**" });
                self.w(text.as_str());
                self.w("*/");
            }
            AttrKind::Normal(n) => {
                self.w("@");
                let unsafe_ = matches!(n.unsafety, Safety::Unsafe(_));
                if unsafe_ {
                    self.w("unsafe(");
                }
                self.path(&n.path, false);
                match &n.args {
                    AttrArgs::Empty => {}
                    AttrArgs::Delimited(d) => {
                        self.out.push(d.delim.open());
                        self.w(&tts_to_string(&d.tokens));
                        self.out.push(d.delim.close());
                    }
                    AttrArgs::Eq { expr, .. } => {
                        self.w(" = ");
                        self.expr(expr);
                    }
                }
                if unsafe_ {
                    self.w(")");
                }
            }
        }
    }

}

/// Token trees as source text, spaced the way people write attribute
/// arguments: `Debug, Clone`, `target_os = "macos"`, `not(windows)`.
pub fn tts_to_string(tts: &[TokenTree]) -> String {
    let mut out = String::new();
    let mut prev: Option<TokenKind> = None;
    for tt in tts {
        let (text, first) = match tt {
            TokenTree::Token(t) => (t.kind.as_string(), t.kind),
            TokenTree::Delimited(_, delim, inner) => {
                let s = format!("{}{}{}", delim.open(), tts_to_string(inner), delim.close());
                (s, TokenKind::open_for(*delim))
            }
        };
        if let Some(p) = prev {
            let no_space = matches!(first, TokenKind::Comma | TokenKind::Dot | TokenKind::CloseParen | TokenKind::CloseBracket)
                || matches!(p, TokenKind::Dot | TokenKind::At)
                || (matches!(first, TokenKind::OpenParen | TokenKind::OpenBracket) && matches!(p, TokenKind::Ident(..)));
            if !no_space {
                out.push(' ');
            }
        }
        out.push_str(&text);
        prev = Some(match tt {
            TokenTree::Token(t) => t.kind,
            TokenTree::Delimited(_, d, _) => TokenKind::close_for(*d),
        });
    }
    out
}
