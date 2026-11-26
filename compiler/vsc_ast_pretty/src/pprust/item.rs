//! Pretty-printing items, functions, structs, enums, traits, impls, aliases, consts, and statics.

use vsc_ast::*;
use vsc_span::Ident;

use super::Printer;

impl Printer {
    // -----------------------------------------------------------------------
    // Items
    // -----------------------------------------------------------------------

    pub(super) fn vis(&mut self, v: &Visibility) {
        match v.kind {
            VisibilityKind::Public => self.w("public "),
            VisibilityKind::Package => self.w("package "),
            VisibilityKind::Inherited => {}
        }
    }

    pub(super) fn item(&mut self, item: &Item) {
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

    pub(super) fn assoc_items(&mut self, items: &[P<AssocItem>]) {
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

    pub(super) fn func(&mut self, f: &Fn) {
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

    pub(super) fn extern_(&mut self, ext: Extern) {
        match ext {
            Extern::None => {}
            Extern::Implicit(_) => self.w("extern "),
            Extern::Explicit(abi, _) => self.w(&format!("extern \"{}\" ", abi.symbol)),
        }
    }

    pub(super) fn param(&mut self, p: &Param) {
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
                TyKind::Ref(lifetime, mt) if matches!(mt.ty.kind, TyKind::ImplicitSelf) => {
                    self.w("&");
                    if let Some(o) = lifetime {
                        self.lifetime_ref(o);
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

    pub(super) fn ret_ty(&mut self, r: &FnRetTy) {
        if let FnRetTy::Ty(t) = r {
            self.w(" -> ");
            self.ty(t);
        }
    }

    pub(super) fn struct_like(&mut self, keyword: &str, ident: Ident, generics: &Generics, data: &VariantData) {
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

    pub(super) fn tuple_fields(&mut self, fields: &[FieldDef]) {
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

    pub(super) fn field(&mut self, f: &FieldDef) {
        self.vis(&f.vis);
        if let Some(i) = f.ident {
            self.ident(i);
            self.w(": ");
        }
        self.ty(&f.ty);
    }

    /// `{ a: T; b: U }` on one line, as enum variants are usually written.
    pub(super) fn record_fields_inline(&mut self, fields: &[FieldDef]) {
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

    pub(super) fn record_fields_block(&mut self, fields: &[FieldDef]) {
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

    pub(super) fn ty_alias(&mut self, t: &TyAlias) {
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

    pub(super) fn const_item(&mut self, c: &ConstItem) {
        self.w("const ");
        self.ident(c.ident);
        self.w(": ");
        self.ty(&c.ty);
        if let Some(e) = &c.expr {
            self.w(" = ");
            self.expr(e);
        }
    }

    pub(super) fn static_item(&mut self, s: &StaticItem) {
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

}
