//! Pretty-printing HIR modules, items, associated items, variants, and function signatures.

use std::fmt::Write;

use vsc_ast::Safety;
use vsc_hir::*;

use super::Printer;

impl<'a> Printer<'a> {
    // -----------------------------------------------------------------------
    // Items
    // -----------------------------------------------------------------------

    pub(super) fn mod_items(&mut self, m: &Mod) {
        for (i, &id) in m.item_ids.iter().enumerate() {
            if i > 0 {
                self.nl();
            }
            self.item(id);
            self.nl();
        }
    }

    pub(super) fn safety(&mut self, s: Safety) {
        match s {
            Safety::Unsafe(_) => self.w("unsafe "),
            Safety::Safe(_) => self.w("safe "),
            Safety::Default => {}
        }
    }

    pub(super) fn vis(&mut self, span: vsc_span::Span) {
        // Visibility is checked in `vsc_resolve`; HIR keeps only where it was
        // written. A non-empty span means something was written.
        if span.lo() != span.hi() {
            self.w("pub ");
        }
    }

    pub(super) fn item(&mut self, id: ItemId) {
        let owner = self.krate.owner(id.owner_id);
        self.owner_attrs(owner);
        let item = self.krate.item(id);
        self.vis(item.vis_span);
        let name = item.ident.name.as_str();
        match &item.kind {
            ItemKind::Mod(m) => {
                let _ = write!(self.out, "mod {name} {{");
                self.indent += 1;
                self.nl();
                self.mod_items(m);
                self.indent -= 1;
                self.trim_trailing_indent();
                self.w("}");
            }
            ItemKind::Fn { sig, generics, body } => {
                self.fn_sig(name, sig, generics, Some(*body), None);
                self.w(" ");
                self.body_value(*body);
            }
            ItemKind::Static(ty, m, body) => {
                self.w("static ");
                if m.is_mut() {
                    self.w("mut ");
                }
                let _ = write!(self.out, "{name}: ");
                self.ty(ty);
                self.w(" = ");
                self.body_value(*body);
                self.w(";");
            }
            ItemKind::Const(ty, _, body) => {
                let _ = write!(self.out, "const {name}: ");
                self.ty(ty);
                self.w(" = ");
                self.body_value(*body);
                self.w(";");
            }
            ItemKind::TyAlias(ty, g) => {
                let _ = write!(self.out, "type {name}");
                self.generic_params(&g.params);
                self.where_clause(g);
                self.w(" = ");
                self.ty(ty);
                self.w(";");
            }
            ItemKind::Enum(def, g) => {
                let _ = write!(self.out, "enum {name}");
                self.generic_params(&g.params);
                self.where_clause(g);
                self.w(" {");
                self.indent += 1;
                for v in &def.variants {
                    self.nl();
                    self.attrs(v.hir_id);
                    self.w(v.ident.name.as_str());
                    self.variant_data(&v.data, false);
                    if let Some(d) = &v.disr_expr {
                        self.w(" = ");
                        self.anon_const(d);
                    }
                    self.w(",");
                }
                self.indent -= 1;
                self.nl();
                self.w("}");
            }
            ItemKind::Struct(data, g) | ItemKind::Class(data, g) | ItemKind::Union(data, g) => {
                let kw = match item.kind {
                    ItemKind::Struct(..) => "struct",
                    ItemKind::Class(..) => "class",
                    _ => "union",
                };
                let _ = write!(self.out, "{kw} {name}");
                self.generic_params(&g.params);
                self.where_clause(g);
                self.variant_data(data, true);
            }
            ItemKind::Trait { safety, generics, bounds, items } => {
                self.safety(*safety);
                let _ = write!(self.out, "trait {name}");
                self.generic_params(&generics.params);
                if !bounds.is_empty() {
                    self.w(": ");
                    self.bounds(bounds);
                }
                self.where_clause(generics);
                self.assoc_items(items);
            }
            ItemKind::Impl(i) => {
                if i.flat_receiver {
                    self.w("// flat receiver");
                    self.nl();
                }
                self.safety(i.safety);
                self.w("impl");
                self.generic_params(&i.generics.params);
                self.w(" ");
                if let Some(t) = &i.of_trait {
                    if i.negative {
                        self.w("!");
                    }
                    self.path(&t.path);
                    self.w(" for ");
                }
                self.ty(&i.self_ty);
                self.where_clause(&i.generics);
                self.assoc_items(&i.items);
            }
            ItemKind::ForeignMod { abi, items } => {
                self.w("extern ");
                if let Some(abi) = abi {
                    let _ = write!(self.out, "\"{abi}\" ");
                }
                self.w("{");
                self.indent += 1;
                for &def in items {
                    self.nl();
                    let owner = self.krate.owner(def);
                    self.owner_attrs(owner);
                    let fi = self.krate.foreign_item(def);
                    self.vis(fi.vis_span);
                    let fname = fi.ident.name.as_str();
                    match &fi.kind {
                        ForeignItemKind::Fn(sig, names, g) => {
                            self.fn_sig(fname, sig, g, None, Some(names));
                            self.w(";");
                        }
                        ForeignItemKind::Static(ty, m, safety) => {
                            self.safety(*safety);
                            self.w("static ");
                            if m.is_mut() {
                                self.w("mut ");
                            }
                            let _ = write!(self.out, "{fname}: ");
                            self.ty(ty);
                            self.w(";");
                        }
                        ForeignItemKind::Type => {
                            let _ = write!(self.out, "type {fname};");
                        }
                    }
                }
                self.indent -= 1;
                self.nl();
                self.w("}");
            }
            ItemKind::GlobalAsm(asm) => {
                self.inline_asm(asm);
                self.w(";");
            }
        }
    }

    pub(super) fn trim_trailing_indent(&mut self) {
        while self.out.ends_with(' ') {
            self.out.pop();
        }
    }

    pub(super) fn assoc_items(&mut self, items: &[vsc_span::def_id::LocalDefId]) {
        self.w(" {");
        self.indent += 1;
        for &def in items {
            self.nl();
            let owner = self.krate.owner(def);
            self.owner_attrs(owner);
            let ai = self.krate.assoc_item(def);
            self.vis(ai.vis_span);
            let name = ai.ident.name.as_str();
            match &ai.kind {
                AssocItemKind::Fn(sig, body) => {
                    self.fn_sig(name, sig, &ai.generics, *body, None);
                    match body {
                        Some(b) => {
                            self.w(" ");
                            self.body_value(*b);
                        }
                        None => self.w(";"),
                    }
                }
                AssocItemKind::Const(ty, body) => {
                    let _ = write!(self.out, "const {name}: ");
                    self.ty(ty);
                    if let Some(b) = body {
                        self.w(" = ");
                        self.body_value(*b);
                    }
                    self.w(";");
                }
                AssocItemKind::Type(bounds, ty) => {
                    let _ = write!(self.out, "type {name}");
                    self.generic_params(&ai.generics.params);
                    if !bounds.is_empty() {
                        self.w(": ");
                        self.bounds(bounds);
                    }
                    self.where_clause(&ai.generics);
                    if let Some(t) = ty {
                        self.w(" = ");
                        self.ty(t);
                    }
                    self.w(";");
                }
            }
        }
        self.indent -= 1;
        self.nl();
        self.w("}");
    }

    pub(super) fn variant_data(&mut self, data: &VariantData, is_item: bool) {
        match data {
            VariantData::Struct { fields } => {
                self.w(" {");
                self.indent += 1;
                for f in fields {
                    self.nl();
                    self.attrs(f.hir_id);
                    self.vis(f.vis_span);
                    let _ = write!(self.out, "{}: ", f.ident);
                    self.ty(&f.ty);
                    self.w(",");
                }
                self.indent -= 1;
                if !fields.is_empty() {
                    self.nl();
                }
                self.w("}");
            }
            VariantData::Tuple(fields, ..) => {
                self.w("(");
                self.comma_sep(fields, |p, f| {
                    p.vis(f.vis_span);
                    p.ty(&f.ty)
                });
                self.w(")");
                if is_item {
                    self.w(";");
                }
            }
            VariantData::Unit(..) => {
                if is_item {
                    self.w(";");
                }
            }
        }
    }

    /// `fn name<G>(params) -> R where …`. Parameter names come from the
    /// body's patterns, or a foreign function's names.
    pub(super) fn fn_sig(&mut self, name: &str, sig: &FnSig, g: &Generics, body: Option<BodyId>, names: Option<&[vsc_span::Ident]>) {
        let h = &sig.header;
        if h.is_const {
            self.w("const ");
        }
        if h.is_async {
            self.w("async ");
        }
        self.safety(h.safety);
        if let Some(abi) = h.abi {
            let _ = write!(self.out, "extern \"{abi}\" ");
        }
        let _ = write!(self.out, "fn {name}");
        self.generic_params(&g.params);
        self.w("(");
        let body = body.map(|b| self.krate.body(b));
        for (i, ty) in sig.decl.inputs.iter().enumerate() {
            if i > 0 {
                self.w(", ");
            }
            if i == 0 && self.self_param(sig.decl.implicit_self, ty) {
                continue;
            }
            match (body, names) {
                (Some(b), _) if i < b.params.len() => {
                    self.pat(&b.params[i].pat);
                    self.w(": ");
                }
                (_, Some(n)) if i < n.len() => {
                    let _ = write!(self.out, "{}: ", n[i]);
                }
                _ => {}
            }
            self.ty(ty);
        }
        if sig.decl.c_variadic {
            if !sig.decl.inputs.is_empty() {
                self.w(", ");
            }
            self.w("...");
        }
        self.w(")");
        self.fn_ret(&sig.decl.output);
        self.where_clause(g);
    }

    /// `self`, `mut self`, `&self`, `&'a mut self`: a receiver written
    /// without a type. Returns false for other first parameters.
    pub(super) fn self_param(&mut self, kind: ImplicitSelfKind, ty: &Ty) -> bool {
        match (kind, &ty.kind) {
            (ImplicitSelfKind::Imm, _) => self.w("self"),
            (ImplicitSelfKind::Mut, _) => self.w("mut self"),
            (ImplicitSelfKind::RefImm | ImplicitSelfKind::RefMut, TyKind::Ref(lifetime, _)) => {
                self.w("&");
                if let LifetimeKind::Named(..) = lifetime.kind {
                    self.lifetime(lifetime);
                    self.w(" ");
                }
                self.w(if kind == ImplicitSelfKind::RefMut { "mut self" } else { "self" });
            }
            _ => return false,
        }
        true
    }

    pub(super) fn fn_ret(&mut self, r: &FnRetTy) {
        if let FnRetTy::Return(t) = r {
            self.w(" -> ");
            self.ty(t);
        }
    }

    pub(super) fn body_value(&mut self, id: BodyId) {
        let body = self.krate.body(id);
        self.expr(&body.value);
    }

    pub(super) fn anon_const(&mut self, c: &AnonConst) {
        self.body_value(c.body);
    }

}
