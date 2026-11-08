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

    // -----------------------------------------------------------------------
    // Items
    // -----------------------------------------------------------------------

    fn vis(&mut self, v: &Visibility) {
        match v.kind {
            VisibilityKind::Public => self.w("public "),
            VisibilityKind::Package => self.w("package "),
            VisibilityKind::Inherited => {}
        }
    }

    fn item(&mut self, item: &Item) {
        self.attrs(&item.attrs);
        // A flat receiver is printed back as one.
        if let ItemKind::Impl(i) = &item.kind {
            if let ImplOrigin::FlatReceiver { .. } = i.origin {
                if let [method] = &i.items[..] {
                    if let AssocItemKind::Fn(f) = &method.kind {
                        self.attrs(&method.attrs);
                        self.vis(&method.vis);
                        self.func(f);
                        return;
                    }
                }
            }
        }
        self.vis(&item.vis);
        match &item.kind {
            ItemKind::Fn(f) => self.func(f),
            ItemKind::TyAlias(t) => self.ty_alias(t),
            ItemKind::Struct(ident, generics, data) => self.struct_like("struct", *ident, generics, data),
            ItemKind::Class(ident, generics, data) => self.struct_like("class", *ident, generics, data),
            ItemKind::Union(ident, generics, data) => self.struct_like("union", *ident, generics, data),
            ItemKind::Enum(ident, generics, def) => {
                self.w("enum ");
                self.ident(*ident);
                self.generic_params(&generics.params);
                self.where_clause(&generics.where_clause);
                if def.variants.is_empty() {
                    self.w(" {}");
                    return;
                }
                self.w(" {");
                self.indent += 1;
                for v in &def.variants {
                    self.nl();
                    self.attrs(&v.attrs);
                    self.vis(&v.vis);
                    self.ident(v.ident);
                    match &v.data {
                        VariantData::Unit(_) => {}
                        VariantData::Tuple(fields, _) => self.tuple_fields(fields),
                        VariantData::Struct { fields } => {
                            self.w(" ");
                            self.record_fields_inline(fields);
                        }
                    }
                    if let Some(d) = &v.disr_expr {
                        self.w(" = ");
                        self.expr(&d.value);
                    }
                }
                self.indent -= 1;
                self.nl();
                self.w("}");
            }
            ItemKind::Const(c) => self.const_item(c),
            ItemKind::Static(s) => self.static_item(s),
            ItemKind::Trait(t) => {
                if matches!(t.safety, Safety::Unsafe(_)) {
                    self.w("unsafe ");
                }
                self.w("trait ");
                self.ident(t.ident);
                self.generic_params(&t.generics.params);
                if !t.bounds.is_empty() {
                    self.w(": ");
                    self.bounds(&t.bounds);
                }
                self.where_clause(&t.generics.where_clause);
                self.assoc_items(&t.items);
            }
            ItemKind::Impl(i) => {
                if matches!(i.safety, Safety::Unsafe(_)) {
                    self.w("unsafe ");
                }
                self.w("implement");
                self.generic_params(&i.generics.params);
                self.w(" ");
                if i.negative {
                    self.w("!");
                }
                if let Some(t) = &i.of_trait {
                    self.path(&t.path, true);
                    self.w(" for ");
                }
                self.ty(&i.self_ty);
                self.where_clause(&i.generics.where_clause);
                self.assoc_items(&i.items);
            }
            ItemKind::ForeignMod(m) => {
                if matches!(m.safety, Safety::Unsafe(_)) {
                    self.w("unsafe ");
                }
                self.w("extern ");
                if let Some(abi) = m.abi {
                    self.w(&format!("\"{}\" ", abi.symbol));
                }
                if m.items.is_empty() {
                    self.w("{}");
                    return;
                }
                self.w("{");
                self.indent += 1;
                for fi in &m.items {
                    self.nl();
                    self.attrs(&fi.attrs);
                    self.vis(&fi.vis);
                    match &fi.kind {
                        ForeignItemKind::Fn(f) => self.func(f),
                        ForeignItemKind::Static(s) => self.static_item(s),
                        ForeignItemKind::TyAlias(t) => {
                            self.w("type ");
                            self.ident(t.ident);
                        }
                    }
                }
                self.indent -= 1;
                self.nl();
                self.w("}");
            }
            ItemKind::GlobalAsm(asm) => self.inline_asm(asm),
        }
    }

    fn assoc_items(&mut self, items: &[P<AssocItem>]) {
        if items.is_empty() {
            self.w(" {}");
            return;
        }
        self.w(" {");
        self.indent += 1;
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.nl();
            }
            self.nl();
            self.attrs(&item.attrs);
            self.vis(&item.vis);
            match &item.kind {
                AssocItemKind::Fn(f) => self.func(f),
                AssocItemKind::Const(c) => self.const_item(c),
                AssocItemKind::Type(t) => self.ty_alias(t),
            }
        }
        self.indent -= 1;
        self.nl();
        self.w("}");
    }

    fn func(&mut self, f: &Fn) {
        let h = &f.sig.header;
        if let Const::Yes(_) = h.constness {
            self.w("const ");
        }
        if h.asyncness.is_some() {
            self.w("async ");
        }
        match h.safety {
            Safety::Unsafe(_) => self.w("unsafe "),
            Safety::Safe(_) => self.w("safe "),
            Safety::Default => {}
        }
        self.extern_(h.ext);
        self.w("func ");
        let inputs = &f.sig.decl.inputs;
        let (receiver, rest) = match inputs.first() {
            Some(p) if p.flat_receiver => (Some(p), &inputs[1..]),
            _ => (None, &inputs[..]),
        };
        if let Some(r) = receiver {
            self.w("(");
            self.pat(&r.pat);
            self.w(": ");
            self.ty(&r.ty);
            self.w(") ");
        }
        self.ident(f.ident);
        self.generic_params(&f.generics.params);
        self.w("(");
        self.comma_sep(rest, |p, param| p.param(param));
        self.w(")");
        self.ret_ty(&f.sig.decl.output);
        self.where_clause(&f.generics.where_clause);
        if let Some(body) = &f.body {
            self.w(" ");
            self.block(body, false);
        }
    }

    fn extern_(&mut self, ext: Extern) {
        match ext {
            Extern::None => {}
            Extern::Implicit(_) => self.w("extern "),
            Extern::Explicit(abi, _) => self.w(&format!("extern \"{}\" ", abi.symbol)),
        }
    }

    fn param(&mut self, p: &Param) {
        for a in &p.attrs {
            self.attr(a);
            self.w(" ");
        }
        if let TyKind::CVarArgs = p.ty.kind {
            if matches!(p.pat.kind, PatKind::Wild) {
                self.w("...");
                return;
            }
        }
        if p.is_self() {
            let PatKind::Ident(mode, ..) = &p.pat.kind else { unreachable!() };
            match &p.ty.kind {
                TyKind::ImplicitSelf => {
                    if mode.1.is_mut() {
                        self.w("var ");
                    }
                    self.w("self");
                }
                TyKind::Ref(origin, mt) if matches!(mt.ty.kind, TyKind::ImplicitSelf) => {
                    self.w("&");
                    if let Some(o) = origin {
                        self.origin_ref(o);
                        self.w(" ");
                    }
                    if mt.mutbl.is_mut() {
                        self.w("mut ");
                    }
                    self.w("self");
                }
                _ => {
                    if mode.1.is_mut() {
                        self.w("var ");
                    }
                    self.w("self: ");
                    self.ty(&p.ty);
                }
            }
            return;
        }
        self.pat(&p.pat);
        if !matches!(p.ty.kind, TyKind::Infer) || !p.ty.span.is_empty() {
            if !matches!(p.ty.kind, TyKind::Infer) {
                self.w(": ");
                self.ty(&p.ty);
            }
        }
    }

    fn ret_ty(&mut self, r: &FnRetTy) {
        if let FnRetTy::Ty(t) = r {
            self.w(" -> ");
            self.ty(t);
        }
    }

    fn struct_like(&mut self, keyword: &str, ident: Ident, generics: &Generics, data: &VariantData) {
        self.w(keyword);
        self.w(" ");
        self.ident(ident);
        self.generic_params(&generics.params);
        match data {
            VariantData::Unit(_) => self.where_clause(&generics.where_clause),
            VariantData::Tuple(fields, _) => {
                self.tuple_fields(fields);
                self.where_clause(&generics.where_clause);
            }
            VariantData::Struct { fields } => {
                self.where_clause(&generics.where_clause);
                self.w(" ");
                self.record_fields_block(fields);
            }
        }
    }

    fn tuple_fields(&mut self, fields: &[FieldDef]) {
        self.w("(");
        self.comma_sep(fields, |p, f| {
            for a in &f.attrs {
                p.attr(a);
                p.w(" ");
            }
            p.vis(&f.vis);
            p.ty(&f.ty);
        });
        self.w(")");
    }

    fn field(&mut self, f: &FieldDef) {
        self.vis(&f.vis);
        if let Some(i) = f.ident {
            self.ident(i);
            self.w(": ");
        }
        self.ty(&f.ty);
    }

    /// `{ a: T; b: U }` on one line, as enum variants are usually written.
    fn record_fields_inline(&mut self, fields: &[FieldDef]) {
        if fields.is_empty() || fields.iter().any(|f| !f.attrs.is_empty()) {
            self.record_fields_block(fields);
            return;
        }
        self.w("{ ");
        for (i, f) in fields.iter().enumerate() {
            if i > 0 {
                self.w("; ");
            }
            self.field(f);
        }
        self.w(" }");
    }

    fn record_fields_block(&mut self, fields: &[FieldDef]) {
        if fields.is_empty() {
            self.w("{}");
            return;
        }
        self.w("{");
        self.indent += 1;
        for f in fields {
            self.nl();
            self.attrs(&f.attrs);
            self.field(f);
        }
        self.indent -= 1;
        self.nl();
        self.w("}");
    }

    fn ty_alias(&mut self, t: &TyAlias) {
        self.w("type ");
        self.ident(t.ident);
        self.generic_params(&t.generics.params);
        if !t.bounds.is_empty() {
            self.w(": ");
            self.bounds(&t.bounds);
        }
        self.where_clause(&t.generics.where_clause);
        if let Some(ty) = &t.ty {
            self.w(" = ");
            self.ty(ty);
        }
    }

    fn const_item(&mut self, c: &ConstItem) {
        self.w("const ");
        self.ident(c.ident);
        self.w(": ");
        self.ty(&c.ty);
        if let Some(e) = &c.expr {
            self.w(" = ");
            self.expr(e);
        }
    }

    fn static_item(&mut self, s: &StaticItem) {
        match s.safety {
            Safety::Unsafe(_) => self.w("unsafe "),
            Safety::Safe(_) => self.w("safe "),
            Safety::Default => {}
        }
        self.w("static ");
        if s.mutability.is_mut() {
            self.w("var ");
        }
        self.ident(s.ident);
        self.w(": ");
        self.ty(&s.ty);
        if let Some(e) = &s.expr {
            self.w(" = ");
            self.expr(e);
        }
    }

    // -----------------------------------------------------------------------
    // Generics
    // -----------------------------------------------------------------------

    fn generic_params(&mut self, params: &[GenericParam]) {
        if params.is_empty() {
            return;
        }
        self.w("<");
        self.comma_sep(params, |p, param| p.generic_param(param));
        self.w(">");
    }

    fn generic_param(&mut self, param: &GenericParam) {
        for a in &param.attrs {
            self.attr(a);
            self.w(" ");
        }
        match &param.kind {
            GenericParamKind::Origin => {
                self.ident(param.ident);
                if !param.bounds.is_empty() {
                    self.w(": ");
                    self.bounds(&param.bounds);
                }
            }
            GenericParamKind::Type { default } => {
                self.ident(param.ident);
                if !param.bounds.is_empty() {
                    self.w(": ");
                    self.bounds(&param.bounds);
                }
                if let Some(d) = default {
                    self.w(" = ");
                    self.ty(d);
                }
            }
            GenericParamKind::Const { ty, default, .. } => {
                self.w("const ");
                self.ident(param.ident);
                self.w(": ");
                self.ty(ty);
                if let Some(d) = default {
                    self.w(" = ");
                    self.expr(&d.value);
                }
            }
        }
    }

    fn where_clause(&mut self, wc: &WhereClause) {
        if wc.predicates.is_empty() {
            return;
        }
        self.w(" where ");
        self.comma_sep(&wc.predicates, |p, pred| match &pred.kind {
            WherePredicateKind::BoundPredicate { bound_generic_params, bounded_ty, bounds } => {
                if !bound_generic_params.is_empty() {
                    p.w("for");
                    p.generic_params(bound_generic_params);
                    p.w(" ");
                }
                p.ty(bounded_ty);
                p.w(": ");
                p.bounds(bounds);
            }
        });
    }

    fn bounds(&mut self, bounds: &[GenericBound]) {
        for (i, b) in bounds.iter().enumerate() {
            if i > 0 {
                self.w(" + ");
            }
            self.bound(b);
        }
    }

    fn bound(&mut self, b: &GenericBound) {
        match b {
            GenericBound::Trait(t) => {
                if t.parens {
                    self.w("(");
                }
                if t.modifiers.maybe {
                    self.w("?");
                }
                if !t.bound_generic_params.is_empty() {
                    self.w("for");
                    self.generic_params(&t.bound_generic_params);
                    self.w(" ");
                }
                self.path(&t.trait_ref.path, true);
                if t.parens {
                    self.w(")");
                }
            }
            GenericBound::Outlives(o) => self.origin_ref(o),
            GenericBound::Use(idents, _) => {
                self.w("use<");
                self.comma_sep(idents, |p, i| p.ident(*i));
                self.w(">");
            }
        }
    }

    fn origin_ref(&mut self, o: &OriginRef) {
        if o.origins.len() == 1 {
            self.w(o.origins[0].name.as_str());
        } else {
            self.w("[");
            self.comma_sep(&o.origins, |p, i| p.w(i.name.as_str()));
            self.w("]");
        }
    }

    // -----------------------------------------------------------------------
    // Paths and types
    // -----------------------------------------------------------------------

    fn path(&mut self, path: &Path, with_args: bool) {
        for (i, seg) in path.segments.iter().enumerate() {
            if i > 0 {
                self.w(".");
            }
            self.ident(seg.ident);
            if with_args {
                if let Some(args) = &seg.args {
                    self.generic_args(args);
                }
            }
        }
    }

    fn qpath(&mut self, qself: &QSelf, path: &Path) {
        self.w("<");
        self.ty(&qself.ty);
        if qself.position > 0 {
            self.w(" as ");
            let trait_path = Path { span: path.span, segments: path.segments[..qself.position].to_vec() };
            self.path(&trait_path, true);
        }
        self.w(">");
        for seg in &path.segments[qself.position..] {
            self.w(".");
            self.ident(seg.ident);
            if let Some(args) = &seg.args {
                self.generic_args(args);
            }
        }
    }

    fn generic_args(&mut self, args: &GenericArgs) {
        match args {
            GenericArgs::AngleBracketed(a) => {
                self.w("<");
                self.comma_sep(&a.args, |p, arg| match arg {
                    AngleBracketedArg::Arg(GenericArg::Type(t)) => p.ty(t),
                    AngleBracketedArg::Arg(GenericArg::Origin(o)) => p.origin_ref(o),
                    AngleBracketedArg::Arg(GenericArg::Const(c)) => p.expr(&c.value),
                    AngleBracketedArg::Constraint(c) => {
                        p.ident(c.ident);
                        match &c.kind {
                            AssocItemConstraintKind::Equality { term } => {
                                p.w(" = ");
                                p.ty(term);
                            }
                            AssocItemConstraintKind::Bound { bounds } => {
                                p.w(": ");
                                p.bounds(bounds);
                            }
                        }
                    }
                });
                self.w(">");
            }
            GenericArgs::Parenthesized(a) => {
                self.w("(");
                self.comma_sep(&a.inputs, |p, t| p.ty(t));
                self.w(")");
                self.ret_ty(&a.output);
            }
        }
    }

    fn ty(&mut self, t: &Ty) {
        match &t.kind {
            TyKind::Slice(inner) => {
                self.w("[");
                self.ty(inner);
                self.w("]");
            }
            TyKind::Array(inner, len) => {
                self.w("[");
                self.ty(inner);
                self.w("; ");
                self.expr(&len.value);
                self.w("]");
            }
            TyKind::Ptr(mt) => {
                self.w(if mt.mutbl.is_mut() { "*mut " } else { "*const " });
                self.ty(&mt.ty);
            }
            TyKind::Ref(origin, mt) => {
                self.w("&");
                if let Some(o) = origin {
                    self.origin_ref(o);
                    self.w(" ");
                }
                if mt.mutbl.is_mut() {
                    self.w("mut ");
                }
                self.ty(&mt.ty);
            }
            TyKind::FnPtr(f) => {
                if !f.generic_params.is_empty() {
                    self.w("for");
                    self.generic_params(&f.generic_params);
                    self.w(" ");
                }
                if matches!(f.safety, Safety::Unsafe(_)) {
                    self.w("unsafe ");
                }
                self.extern_(f.ext);
                self.w("func");
                self.fn_ty_decl(&f.decl);
            }
            TyKind::FuncValue(f) => {
                if !f.generic_params.is_empty() {
                    self.w("for");
                    self.generic_params(&f.generic_params);
                    self.w(" ");
                }
                self.w("func");
                self.fn_ty_decl(&f.decl);
            }
            TyKind::Never => self.w("never"),
            TyKind::Tup(tys) => {
                self.w("(");
                self.comma_sep(tys, |p, t| p.ty(t));
                if tys.len() == 1 {
                    self.w(",");
                }
                self.w(")");
            }
            TyKind::Path(None, path) => self.path(path, true),
            TyKind::Path(Some(q), path) => self.qpath(q, path),
            TyKind::TraitObject(bounds) => {
                self.w("dyn ");
                self.bounds(bounds);
            }
            TyKind::ImplTrait(_, bounds) => {
                self.w("impl ");
                self.bounds(bounds);
            }
            TyKind::Paren(inner) => {
                self.w("(");
                self.ty(inner);
                self.w(")");
            }
            TyKind::Infer => self.w("_"),
            TyKind::ImplicitSelf => self.w("Self"),
            TyKind::CVarArgs => self.w("..."),
            TyKind::Err => self.w("/*error*/"),
        }
    }

    fn fn_ty_decl(&mut self, decl: &FnDecl) {
        self.w("(");
        self.comma_sep(&decl.inputs, |p, param| {
            if let TyKind::CVarArgs = param.ty.kind {
                p.w("...");
                return;
            }
            match &param.pat.kind {
                PatKind::Ident(_, ident, _) => {
                    p.ident(*ident);
                    p.w(": ");
                }
                PatKind::Wild if !param.pat.span.is_empty() => p.w("_: "),
                _ => {}
            }
            p.ty(&param.ty);
        });
        self.w(")");
        self.ret_ty(&decl.output);
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    fn pat(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Wild => self.w("_"),
            PatKind::Ident(mode, ident, sub) => {
                self.binding_mode(*mode);
                self.ident(*ident);
                if let Some(sub) = sub {
                    self.w(" @ ");
                    self.pat(sub);
                }
            }
            PatKind::Struct(qself, path, fields, rest) => {
                self.maybe_qpath(qself.as_deref(), path);
                self.w(" {");
                if fields.is_empty() && *rest == PatFieldsRest::None {
                    self.w("}");
                    return;
                }
                self.w(" ");
                self.comma_sep(fields, |p, f| {
                    if f.is_shorthand {
                        p.pat(&f.pat);
                    } else {
                        p.ident(f.ident);
                        p.w(": ");
                        p.pat(&f.pat);
                    }
                });
                if *rest == PatFieldsRest::Rest {
                    if !fields.is_empty() {
                        self.w(", ");
                    }
                    self.w("..");
                }
                self.w(" }");
            }
            PatKind::TupleStruct(qself, path, pats) => {
                self.maybe_qpath(qself.as_deref(), path);
                self.w("(");
                self.comma_sep(pats, |p, pat| p.pat(pat));
                self.w(")");
            }
            PatKind::Or(pats) => {
                for (i, p) in pats.iter().enumerate() {
                    if i > 0 {
                        self.w(" | ");
                    }
                    self.pat(p);
                }
            }
            PatKind::Path(qself, path) => self.maybe_qpath(qself.as_deref(), path),
            PatKind::Tuple(pats) => {
                self.w("(");
                self.comma_sep(pats, |p, pat| p.pat(pat));
                if pats.len() == 1 && !matches!(pats[0].kind, PatKind::Rest) {
                    self.w(",");
                }
                self.w(")");
            }
            PatKind::Ref(inner, m) => {
                self.w("&");
                if m.is_mut() {
                    self.w("mut ");
                }
                self.pat(inner);
            }
            PatKind::Expr(e) => self.expr(e),
            PatKind::Range(lo, hi, end) => {
                if let Some(lo) = lo {
                    self.expr(lo);
                }
                self.w(match end {
                    RangeEnd::Included => "..=",
                    RangeEnd::Excluded => "..",
                });
                if let Some(hi) = hi {
                    self.expr(hi);
                }
            }
            PatKind::Slice(pats) => {
                self.w("[");
                self.comma_sep(pats, |p, pat| p.pat(pat));
                self.w("]");
            }
            PatKind::Rest => self.w(".."),
            PatKind::Paren(inner) => {
                self.w("(");
                self.pat(inner);
                self.w(")");
            }
            PatKind::Err => self.w("/*error*/"),
        }
    }

    fn binding_mode(&mut self, mode: BindingMode) {
        match mode.0 {
            ByRef::Yes(m) => self.w(if m.is_mut() { "ref mut " } else { "ref " }),
            ByRef::No if mode.1.is_mut() => self.w("var "),
            ByRef::No => {}
        }
    }

    fn maybe_qpath(&mut self, qself: Option<&QSelf>, path: &Path) {
        match qself {
            Some(q) => self.qpath(q, path),
            None => self.path(path, true),
        }
    }

    // -----------------------------------------------------------------------
    // Statements and blocks
    // -----------------------------------------------------------------------

    /// A block on its own lines, or `{ e }` on one line when `inline` is
    /// allowed and the block is a single short expression.
    fn block(&mut self, b: &Block, inline: bool) {
        if b.rules == BlockCheckMode::Unsafe {
            self.w("unsafe ");
        }
        if b.stmts.is_empty() {
            self.w("{}");
            return;
        }
        if inline && b.stmts.len() == 1 {
            if let StmtKind::Expr(e) = &b.stmts[0].kind {
                if is_simple(e) {
                    self.w("{ ");
                    self.expr(e);
                    self.w(" }");
                    return;
                }
            }
        }
        self.w("{");
        self.indent += 1;
        for s in &b.stmts {
            self.nl();
            self.stmt(s);
        }
        self.indent -= 1;
        self.nl();
        self.w("}");
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let(l) => {
                self.attrs(&l.attrs);
                self.w(if l.is_var { "var " } else { "let " });
                self.pat(&l.pat);
                if let Some(t) = &l.ty {
                    self.w(": ");
                    self.ty(t);
                }
                match &l.kind {
                    LocalKind::Decl => {}
                    LocalKind::Init(e) => {
                        self.w(" = ");
                        self.expr(e);
                    }
                    LocalKind::InitElse(e, b) => {
                        self.w(" = ");
                        self.expr(e);
                        self.w(" else ");
                        self.block(b, false);
                    }
                }
            }
            StmtKind::Item(i) => self.item(i),
            StmtKind::Expr(e) => self.expr(e),
        }
    }

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------

    fn label(&mut self, label: &Option<Label>) {
        if let Some(l) = label {
            self.w(l.ident.name.as_str());
            self.w(": ");
        }
    }

    fn expr(&mut self, e: &Expr) {
        for a in &e.attrs {
            self.attr(a);
            self.w(" ");
        }
        match &e.kind {
            ExprKind::Array(es) => {
                self.w("[");
                self.comma_sep(es, |p, e| p.expr(e));
                self.w("]");
            }
            ExprKind::Repeat(e, n) => {
                self.w("[");
                self.expr(e);
                self.w("; ");
                self.expr(&n.value);
                self.w("]");
            }
            ExprKind::Vec(inner) => {
                self.w("vec");
                self.expr(inner);
            }
            ExprKind::ConstBlock(c) => {
                self.w("const ");
                self.expr(&c.value);
            }
            ExprKind::Call(f, args) => {
                self.expr(f);
                self.w("(");
                self.comma_sep(args, |p, a| p.expr(a));
                self.w(")");
            }
            ExprKind::MethodCall(m) => {
                self.expr(&m.receiver);
                self.w(".");
                self.ident(m.seg.ident);
                self.w("(");
                self.comma_sep(&m.args, |p, a| p.expr(a));
                self.w(")");
            }
            ExprKind::Tup(es) => {
                self.w("(");
                self.comma_sep(es, |p, e| p.expr(e));
                if es.len() == 1 {
                    self.w(",");
                }
                self.w(")");
            }
            ExprKind::Binary(op, l, r) => {
                self.expr(l);
                self.w(" ");
                self.w(op.node.as_str());
                self.w(" ");
                self.expr(r);
            }
            ExprKind::Unary(op, e) => {
                self.w(op.as_str());
                self.expr(e);
            }
            ExprKind::Lit(lit) => self.w(&lit.to_string()),
            ExprKind::Cast(e, t) => {
                self.expr(e);
                self.w(" as ");
                self.ty(t);
            }
            ExprKind::Let(pat, e, _) => {
                self.w("let ");
                self.pat(pat);
                self.w(" = ");
                self.expr(e);
            }
            ExprKind::If(cond, then, els) => {
                self.w("if ");
                self.expr(cond);
                self.w(" ");
                self.block(then, false);
                if let Some(els) = els {
                    self.w(" else ");
                    match &els.kind {
                        ExprKind::Block(b, None) => self.block(b, false),
                        _ => self.expr(els),
                    }
                }
            }
            ExprKind::While(cond, body, label) => {
                self.label(label);
                self.w("while ");
                self.expr(cond);
                self.w(" ");
                self.block(body, false);
            }
            ExprKind::ForLoop { pat, iter, body, label } => {
                self.label(label);
                self.w("for ");
                self.pat(pat);
                self.w(" in ");
                self.expr(iter);
                self.w(" ");
                self.block(body, false);
            }
            ExprKind::Loop(body, label, _) => {
                self.label(label);
                self.w("loop ");
                self.block(body, false);
            }
            ExprKind::Match(scrutinee, arms) => {
                self.w("match ");
                self.expr(scrutinee);
                if arms.is_empty() {
                    self.w(" {}");
                    return;
                }
                self.w(" {");
                self.indent += 1;
                for arm in arms {
                    self.nl();
                    self.attrs(&arm.attrs);
                    self.pat(&arm.pat);
                    if let Some(g) = &arm.guard {
                        self.w(" if ");
                        self.expr(g);
                    }
                    self.w(" => ");
                    match &arm.body.kind {
                        ExprKind::Block(b, None) if arm.body.attrs.is_empty() => self.block(b, true),
                        _ => self.expr(&arm.body),
                    }
                }
                self.indent -= 1;
                self.nl();
                self.w("}");
            }
            ExprKind::Closure(c) => {
                if c.asyncness.is_some() {
                    self.w("async ");
                }
                self.w("func(");
                self.comma_sep(&c.fn_decl.inputs, |p, param| p.param(param));
                self.w(")");
                self.ret_ty(&c.fn_decl.output);
                self.w(" ");
                self.block(&c.body, true);
            }
            ExprKind::Block(b, label) => {
                self.label(label);
                self.block(b, false);
            }
            ExprKind::Async(b) => {
                self.w("async ");
                self.block(b, true);
            }
            ExprKind::Await(e, _) => {
                self.expr(e);
                self.w(".await");
            }
            ExprKind::Assign(l, r, _) => {
                self.expr(l);
                self.w(" = ");
                self.expr(r);
            }
            ExprKind::AssignOp(op, l, r) => {
                self.expr(l);
                self.w(" ");
                self.w(op.node.as_str());
                self.w("= ");
                self.expr(r);
            }
            ExprKind::Field(e, ident) => {
                self.expr(e);
                self.w(".");
                self.w(ident.name.as_str());
            }
            ExprKind::Index(e, idx, _) => {
                self.expr(e);
                self.w("[");
                self.expr(idx);
                self.w("]");
            }
            ExprKind::Range(lo, hi, limits) => {
                if let Some(lo) = lo {
                    self.expr(lo);
                }
                self.w(match limits {
                    RangeLimits::HalfOpen => "..",
                    RangeLimits::Closed => "..=",
                });
                if let Some(hi) = hi {
                    self.expr(hi);
                }
            }
            ExprKind::Underscore => self.w("_"),
            ExprKind::Path(None, path) => self.path(path, false),
            ExprKind::Path(Some(q), path) => self.qpath(q, path),
            ExprKind::AddrOf(kind, m, e) => {
                self.w("&");
                if *kind == BorrowKind::Raw {
                    self.w(if m.is_mut() { "raw mut " } else { "raw const " });
                } else if m.is_mut() {
                    self.w("mut ");
                }
                self.expr(e);
            }
            ExprKind::Break(label, value) => {
                self.w("break");
                if let Some(l) = label {
                    self.w(" ");
                    self.w(l.ident.name.as_str());
                }
                if let Some(v) = value {
                    self.w(" ");
                    self.expr(v);
                }
            }
            ExprKind::Continue(label) => {
                self.w("continue");
                if let Some(l) = label {
                    self.w(" ");
                    self.w(l.ident.name.as_str());
                }
            }
            ExprKind::Ret(value) => {
                self.w("return");
                if let Some(v) = value {
                    self.w(" ");
                    self.expr(v);
                }
            }
            ExprKind::InlineAsm(asm) => self.inline_asm(asm),
            ExprKind::Builtin(b) => self.builtin(b),
            ExprKind::Struct(s) => {
                self.maybe_qpath(s.qself.as_deref(), &s.path);
                if s.fields.is_empty() && matches!(s.rest, StructRest::None) {
                    self.w(" {}");
                    return;
                }
                self.w(" { ");
                self.comma_sep(&s.fields, |p, f| {
                    if f.is_shorthand {
                        p.ident(f.ident);
                    } else {
                        p.w(f.ident.name.as_str());
                        p.w(": ");
                        p.expr(&f.expr);
                    }
                });
                if let StructRest::Base(base) = &s.rest {
                    if !s.fields.is_empty() {
                        self.w(", ");
                    }
                    self.w("..");
                    self.expr(base);
                }
                self.w(" }");
            }
            ExprKind::Paren(e) => {
                self.w("(");
                self.expr(e);
                self.w(")");
            }
            ExprKind::Try(e) => {
                self.expr(e);
                self.w("?");
            }
            ExprKind::Err => self.w("/*error*/"),
        }
    }

    /// A built-in call, printed as it was written.
    fn builtin(&mut self, b: &BuiltinCall) {
        self.w(b.kind.name());
        self.w("(");
        let mut first = true;
        let mut sep = |p: &mut Self| {
            if !std::mem::replace(&mut first, false) {
                p.w(", ");
            }
        };
        let fmt = |p: &mut Self, f: &FormatArgs, sep: &mut dyn FnMut(&mut Self)| {
            sep(p);
            p.w(&f.lit.to_string());
            for arg in &f.arguments {
                match arg.kind {
                    FormatArgumentKind::Normal => {
                        sep(p);
                        p.expr(&arg.expr);
                    }
                    FormatArgumentKind::Named(name) => {
                        sep(p);
                        p.ident(name);
                        p.w(" = ");
                        p.expr(&arg.expr);
                    }
                    FormatArgumentKind::Captured(_) => {}
                }
            }
        };
        match &b.kind {
            BuiltinKind::Print { args, .. } | BuiltinKind::Format(args) => {
                if !(args.lit.symbol.is_empty() && matches!(b.kind, BuiltinKind::Print { newline: true, .. }) && args.lit_span.is_empty()) {
                    fmt(self, args, &mut sep);
                }
            }
            BuiltinKind::Panic(f) | BuiltinKind::Todo(f) | BuiltinKind::Unreachable(f) => {
                if let Some(f) = f {
                    fmt(self, f, &mut sep);
                }
            }
            BuiltinKind::Assert { cond, msg } => {
                sep(self);
                self.expr(cond);
                if let Some(f) = msg {
                    fmt(self, f, &mut sep);
                }
            }
            BuiltinKind::AssertEq { left, right, msg } => {
                sep(self);
                self.expr(left);
                sep(self);
                self.expr(right);
                if let Some(f) = msg {
                    fmt(self, f, &mut sep);
                }
            }
        }
        self.w(")");
    }

    fn inline_asm(&mut self, asm: &InlineAsm) {
        self.w(match asm.asm_macro {
            AsmMacro::Asm => "asm(",
            AsmMacro::NakedAsm => "nakedAsm(",
            AsmMacro::GlobalAsm => "globalAsm(",
        });
        let mut first = true;
        let mut sep = |p: &mut Self| {
            if !std::mem::replace(&mut first, false) {
                p.w(", ");
            }
        };
        for (lit, _) in &asm.templates {
            sep(self);
            self.w(&lit.to_string());
        }
        for (op, _) in &asm.operands {
            sep(self);
            let reg = |r: &InlineAsmRegOrRegClass| match r {
                InlineAsmRegOrRegClass::Reg(s) => format!("(\"{s}\")"),
                InlineAsmRegOrRegClass::RegClass(s) => format!("({s})"),
            };
            let name = |p: &mut Self, n: &Option<Ident>| {
                if let Some(n) = n {
                    p.ident(*n);
                    p.w(" = ");
                }
            };
            match op {
                InlineAsmOperand::In { name: n, reg: r, expr } => {
                    name(self, n);
                    self.w(&format!("in{} ", reg(r)));
                    self.expr(expr);
                }
                InlineAsmOperand::Out { name: n, reg: r, late, expr } => {
                    name(self, n);
                    self.w(&format!("{}{} ", if *late { "lateout" } else { "out" }, reg(r)));
                    match expr {
                        Some(e) => self.expr(e),
                        None => self.w("_"),
                    }
                }
                InlineAsmOperand::InOut { name: n, reg: r, late, expr } => {
                    name(self, n);
                    self.w(&format!("{}{} ", if *late { "inlateout" } else { "inout" }, reg(r)));
                    self.expr(expr);
                }
                InlineAsmOperand::SplitInOut { name: n, reg: r, late, in_expr, out_expr } => {
                    name(self, n);
                    self.w(&format!("{}{} ", if *late { "inlateout" } else { "inout" }, reg(r)));
                    self.expr(in_expr);
                    self.w(" => ");
                    match out_expr {
                        Some(e) => self.expr(e),
                        None => self.w("_"),
                    }
                }
                InlineAsmOperand::Const { name: n, anon_const } => {
                    name(self, n);
                    self.w("const ");
                    self.expr(&anon_const.value);
                }
                InlineAsmOperand::Sym { name: n, qself, path, .. } => {
                    name(self, n);
                    self.w("sym ");
                    self.maybe_qpath(qself.as_deref(), path);
                }
                InlineAsmOperand::Label { name: n, block } => {
                    name(self, n);
                    self.w("label ");
                    self.block(block, true);
                }
            }
        }
        if !asm.clobber_abis.is_empty() {
            sep(self);
            self.w("clobberAbi(");
            let abis: Vec<String> = asm.clobber_abis.iter().map(|(s, _)| format!("\"{s}\"")).collect();
            self.w(&abis.join(", "));
            self.w(")");
        }
        if !asm.options.is_empty() {
            sep(self);
            self.w("options(");
            let opts: Vec<&str> = asm.options.iter().map(|(s, _)| s.as_str()).collect();
            self.w(&opts.join(", "));
            self.w(")");
        }
        self.w(")");
    }
}

/// Short enough for `{ e }` on one line: no nested blocks.
fn is_simple(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::If(..)
        | ExprKind::Match(..)
        | ExprKind::Loop(..)
        | ExprKind::While(..)
        | ExprKind::ForLoop { .. }
        | ExprKind::Block(..)
        | ExprKind::Async(..) => false,
        ExprKind::Closure(c) => c.body.stmts.len() <= 1,
        ExprKind::Call(f, args) => is_simple(f) && args.iter().all(|a| is_simple(a)),
        ExprKind::MethodCall(m) => is_simple(&m.receiver) && m.args.iter().all(|a| is_simple(a)),
        ExprKind::Binary(_, l, r) | ExprKind::Assign(l, r, _) | ExprKind::AssignOp(_, l, r) => {
            is_simple(l) && is_simple(r)
        }
        ExprKind::Unary(_, e) | ExprKind::Paren(e) | ExprKind::Try(e) | ExprKind::Field(e, _) | ExprKind::Await(e, _) => {
            is_simple(e)
        }
        _ => true,
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
