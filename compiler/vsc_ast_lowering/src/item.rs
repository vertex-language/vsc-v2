//! Lowering items (rustc's `item.rs`).
//!
//! Every item, associated item and foreign item becomes an owner. Packages
//! (exception 1) become `Mod` items holding the items of all their files;
//! their imports are gone, since paths already carry their resolutions.

use vsc_ast::{self as ast};
use vsc_hir::def::Res;
use vsc_hir::{self as hir};
use vsc_span::def_id::{CRATE_DEF_ID, LocalDefId};
use vsc_span::{Ident, Span, Symbol, kw};

use crate::LoweringContext;

pub(crate) fn lower_crate(lcx: &mut LoweringContext<'_>, krate: &ast::Crate) {
    let root = &krate.root;
    lcx.with_hir_id_owner(CRATE_DEF_ID, root.id, |this| {
        let attrs = package_attrs(root);
        let id = this.lower_node_id(root.id);
        this.lower_attrs(id, &attrs);
        hir::OwnerNode::Crate(this.lower_mod(root))
    });
}

/// The attributes of a package: its files' `package` declarations' and
/// their `//!` comments (Rust's inner attributes).
fn package_attrs(pkg: &ast::Package) -> Vec<ast::Attribute> {
    let mut attrs = Vec::new();
    for f in &pkg.files {
        attrs.extend(f.inner_attrs.iter().cloned());
        if let Some(decl) = &f.package {
            attrs.extend(decl.attrs.iter().cloned());
        }
    }
    attrs
}

impl LoweringContext<'_> {
    fn lower_mod(&mut self, pkg: &ast::Package) -> hir::Mod {
        let mut item_ids = Vec::new();
        for item in pkg.items() {
            item_ids.push(self.lower_item(item));
        }
        for child in &pkg.children {
            item_ids.push(self.lower_package_item(child));
        }
        hir::Mod { item_ids, spans: pkg.span }
    }

    /// A child package, as a `Mod` item.
    fn lower_package_item(&mut self, pkg: &ast::Package) -> hir::ItemId {
        let def = self.local_def_id(pkg.id);
        self.with_hir_id_owner(def, pkg.id, |this| {
            let attrs = package_attrs(pkg);
            let id = this.lower_node_id(pkg.id);
            this.lower_attrs(id, &attrs);
            let kind = hir::ItemKind::Mod(this.lower_mod(pkg));
            hir::OwnerNode::Item(hir::Item { owner_id: def, ident: pkg.name, kind, span: pkg.span, vis_span: pkg.span.shrink_to_lo() })
        });
        hir::ItemId { owner_id: def }
    }

    pub(crate) fn lower_item(&mut self, item: &ast::Item) -> hir::ItemId {
        let def = self.local_def_id(item.id);
        self.with_hir_id_owner(def, item.id, |this| {
            let id = this.lower_node_id(item.id);
            this.lower_attrs(id, &item.attrs);
            let kind = this.lower_item_kind(item, def);
            let ident = item.kind.ident().unwrap_or_else(|| Ident::new(kw::Empty, item.span.shrink_to_lo()));
            hir::OwnerNode::Item(hir::Item { owner_id: def, ident, kind, span: item.span, vis_span: item.vis.span })
        });
        hir::ItemId { owner_id: def }
    }

    fn lower_item_kind(&mut self, item: &ast::Item, def: LocalDefId) -> hir::ItemKind {
        match &item.kind {
            ast::ItemKind::Fn(f) => {
                let generics = self.lower_generics(&f.generics, &[]);
                let sig = self.lower_fn_sig(&f.sig);
                let body = self.lower_fn_body(&f.sig.decl, f.body.as_deref(), item.span);
                hir::ItemKind::Fn { sig, generics, body }
            }
            ast::ItemKind::TyAlias(t) => {
                let generics = self.lower_generics(&t.generics, &[]);
                let ty = match &t.ty {
                    Some(ty) => self.lower_ty(ty),
                    None => {
                        self.dcx()
                            .struct_span_err(item.span, "type alias is missing its type")
                            .with_span_label(item.span, "add `= Type`")
                            .emit();
                        self.ty_err(item.span)
                    }
                };
                if !t.bounds.is_empty() {
                    let span = t.bounds[0].span().to(t.bounds.last().unwrap().span());
                    self.dcx()
                        .struct_span_err(span, "bounds on a type alias aren't enforced")
                        .with_span_label(span, "remove these bounds")
                        .emit();
                }
                hir::ItemKind::TyAlias(ty, generics)
            }
            ast::ItemKind::Struct(_, g, data) => {
                let generics = self.lower_generics(g, &[]);
                hir::ItemKind::Struct(self.lower_variant_data(data), generics)
            }
            ast::ItemKind::Class(_, g, data) => {
                let generics = self.lower_generics(g, &[]);
                hir::ItemKind::Class(self.lower_variant_data(data), generics)
            }
            ast::ItemKind::Union(_, g, data) => {
                let generics = self.lower_generics(g, &[]);
                hir::ItemKind::Union(self.lower_variant_data(data), generics)
            }
            ast::ItemKind::Enum(_, g, e) => {
                let generics = self.lower_generics(g, &[]);
                let variants = e.variants.iter().map(|v| self.lower_variant(v)).collect();
                hir::ItemKind::Enum(hir::EnumDef { variants }, generics)
            }
            ast::ItemKind::Const(c) => {
                let ty = self.lower_ty(&c.ty);
                let body = self.lower_const_body(c.expr.as_deref(), item.span);
                hir::ItemKind::Const(ty, hir::Generics::default(), body)
            }
            ast::ItemKind::Static(s) => {
                let ty = self.lower_ty(&s.ty);
                let body = self.lower_const_body(s.expr.as_deref(), item.span);
                hir::ItemKind::Static(ty, s.mutability, body)
            }
            ast::ItemKind::Trait(t) => {
                let generics = self.lower_generics(&t.generics, &[]);
                let bounds = self.lower_bounds(&t.bounds);
                let saved = self.self_res.replace(Res::SelfTyParam { trait_: def.to_def_id() });
                let items = t.items.iter().map(|ai| self.lower_assoc_item(ai, false)).collect();
                self.self_res = saved;
                hir::ItemKind::Trait { safety: t.safety, generics, bounds, items }
            }
            ast::ItemKind::Impl(i) => {
                let extra = self.resolver.flat_receiver_params.get(&item.id).cloned().unwrap_or_default();
                let generics = self.lower_generics(&i.generics, &extra);
                let of_trait = i.of_trait.as_ref().map(|t| self.lower_trait_ref(t));
                let self_ty = self.lower_ty(&i.self_ty);
                let saved = self.self_res.replace(Res::SelfTyAlias { alias_to: def.to_def_id() });
                let items = i.items.iter().map(|ai| self.lower_assoc_item(ai, true)).collect();
                self.self_res = saved;
                hir::ItemKind::Impl(hir::Impl {
                    safety: i.safety,
                    negative: i.negative,
                    generics,
                    of_trait,
                    self_ty,
                    items,
                    flat_receiver: matches!(i.origin, ast::ImplOrigin::FlatReceiver { .. }),
                })
            }
            ast::ItemKind::ForeignMod(m) => {
                let items = m.items.iter().map(|fi| self.lower_foreign_item(fi)).collect();
                hir::ItemKind::ForeignMod { abi: m.abi.map(|a| a.symbol), items }
            }
            ast::ItemKind::GlobalAsm(asm) => hir::ItemKind::GlobalAsm(self.lower_inline_asm(asm)),
        }
    }

    fn lower_fn_sig(&mut self, sig: &ast::FnSig) -> hir::FnSig {
        hir::FnSig { header: self.lower_fn_header(&sig.header), decl: self.lower_fn_decl(&sig.decl), span: sig.span }
    }

    /// A constant's or static's value. A missing one was reported by the
    /// parser.
    fn lower_const_body(&mut self, expr: Option<&ast::Expr>, span: Span) -> hir::BodyId {
        self.with_new_scopes(|this| {
            let value = match expr {
                Some(e) => this.lower_expr(e),
                None => this.expr_err(span),
            };
            this.record_body(Vec::new(), value)
        })
    }

    fn lower_assoc_item(&mut self, item: &ast::AssocItem, in_impl: bool) -> LocalDefId {
        let def = self.local_def_id(item.id);
        self.with_hir_id_owner(def, item.id, |this| {
            let id = this.lower_node_id(item.id);
            this.lower_attrs(id, &item.attrs);
            let (ident, generics, kind) = match &item.kind {
                ast::AssocItemKind::Fn(f) => {
                    let generics = this.lower_generics(&f.generics, &[]);
                    let sig = this.lower_fn_sig(&f.sig);
                    let body = match (&f.body, in_impl) {
                        (Some(b), _) => Some(this.lower_fn_body(&f.sig.decl, Some(b), item.span)),
                        (None, true) => Some(this.lower_fn_body(&f.sig.decl, None, item.span)),
                        (None, false) => None,
                    };
                    (f.ident, generics, hir::AssocItemKind::Fn(sig, body))
                }
                ast::AssocItemKind::Const(c) => {
                    let ty = this.lower_ty(&c.ty);
                    let body = match (&c.expr, in_impl) {
                        (None, false) => None,
                        (e, _) => Some(this.lower_const_body(e.as_deref(), item.span)),
                    };
                    (c.ident, hir::Generics::default(), hir::AssocItemKind::Const(ty, body))
                }
                ast::AssocItemKind::Type(t) => {
                    let generics = this.lower_generics(&t.generics, &[]);
                    let bounds = this.lower_bounds(&t.bounds);
                    let ty = match (&t.ty, in_impl) {
                        (Some(ty), _) => Some(this.lower_ty(ty)),
                        (None, true) => {
                            this.dcx()
                                .struct_span_err(item.span, "associated type in an `implement` block is missing its type")
                                .with_span_label(item.span, "add `= Type`")
                                .emit();
                            Some(this.ty_err(item.span))
                        }
                        (None, false) => None,
                    };
                    (t.ident, generics, hir::AssocItemKind::Type(bounds, ty))
                }
            };
            hir::OwnerNode::AssocItem(hir::AssocItem { owner_id: def, ident, generics, kind, span: item.span, vis_span: item.vis.span })
        });
        def
    }

    fn lower_foreign_item(&mut self, item: &ast::ForeignItem) -> LocalDefId {
        let def = self.local_def_id(item.id);
        self.with_hir_id_owner(def, item.id, |this| {
            let id = this.lower_node_id(item.id);
            this.lower_attrs(id, &item.attrs);
            let (ident, kind) = match &item.kind {
                ast::ForeignItemKind::Fn(f) => {
                    let generics = this.lower_generics(&f.generics, &[]);
                    let sig = this.lower_fn_sig(&f.sig);
                    let names = f
                        .sig
                        .decl
                        .inputs
                        .iter()
                        .filter(|p| !matches!(p.ty.kind, ast::TyKind::CVarArgs))
                        .map(|p| match p.pat.kind {
                            ast::PatKind::Ident(_, ident, None) => ident,
                            _ => Ident::new(kw::Underscore, p.pat.span),
                        })
                        .collect();
                    (f.ident, hir::ForeignItemKind::Fn(sig, names, generics))
                }
                ast::ForeignItemKind::Static(s) => (s.ident, hir::ForeignItemKind::Static(this.lower_ty(&s.ty), s.mutability, s.safety)),
                ast::ForeignItemKind::TyAlias(t) => (t.ident, hir::ForeignItemKind::Type),
            };
            hir::OwnerNode::ForeignItem(hir::ForeignItem { owner_id: def, ident, kind, span: item.span, vis_span: item.vis.span })
        });
        def
    }

    fn lower_variant(&mut self, v: &ast::Variant) -> hir::Variant {
        let hir_id = self.lower_node_id(v.id);
        self.lower_attrs(hir_id, &v.attrs);
        hir::Variant {
            ident: v.ident,
            hir_id,
            def_id: self.local_def_id(v.id),
            data: self.lower_variant_data(&v.data),
            disr_expr: v.disr_expr.as_ref().map(|d| self.lower_anon_const(d)),
            span: v.span,
        }
    }

    fn lower_variant_data(&mut self, data: &ast::VariantData) -> hir::VariantData {
        match data {
            ast::VariantData::Struct { fields } => {
                hir::VariantData::Struct { fields: fields.iter().enumerate().map(|(i, f)| self.lower_field_def(i, f)).collect() }
            }
            ast::VariantData::Tuple(fields, ctor) => {
                let fields = fields.iter().enumerate().map(|(i, f)| self.lower_field_def(i, f)).collect();
                hir::VariantData::Tuple(fields, self.lower_node_id(*ctor), self.local_def_id(*ctor))
            }
            ast::VariantData::Unit(ctor) => hir::VariantData::Unit(self.lower_node_id(*ctor), self.local_def_id(*ctor)),
        }
    }

    fn lower_field_def(&mut self, index: usize, f: &ast::FieldDef) -> hir::FieldDef {
        let hir_id = self.lower_node_id(f.id);
        self.lower_attrs(hir_id, &f.attrs);
        hir::FieldDef {
            span: f.span,
            vis_span: f.vis.span,
            // Tuple fields are named by their position, as in rustc.
            ident: f.ident.unwrap_or_else(|| Ident::new(Symbol::intern(&index.to_string()), f.span)),
            hir_id,
            def_id: self.local_def_id(f.id),
            ty: self.lower_ty(&f.ty),
        }
    }
}
