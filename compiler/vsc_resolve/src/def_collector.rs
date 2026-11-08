//! Creating a `LocalDefId` for every definition (rustc's `def_collector.rs`).

use vsc_ast::visit::{self, AssocCtxt, FnKind, Visitor};
use vsc_ast::*;
use vsc_data_structures::{FxHashMap, FxIndexMap};
use vsc_hir::def::{CtorKind, CtorOf, DefKind};
use vsc_span::{Ident, Span};
use vsc_span::def_id::{CRATE_DEF_ID, LocalDefId};

use crate::{ModuleData, Resolver, Visibility};

pub(crate) fn collect_definitions(r: &mut Resolver<'_>, krate: &Crate) {
    let root = r.create_def(None, krate.root.id, Some(krate.root.name.name), DefKind::Mod, krate.root.span);
    assert_eq!(root, CRATE_DEF_ID);
    r.node_id_to_def_id.insert(krate.id, root);
    r.visibilities.insert(root, Visibility::Public);
    collect_package(r, &krate.root, root, None);
}

fn new_module(kind: DefKind, name: vsc_span::Symbol, parent: Option<LocalDefId>, files: usize) -> ModuleData {
    ModuleData {
        kind,
        name,
        parent,
        bindings: FxIndexMap::default(),
        children: FxHashMap::default(),
        files: (0..files).map(|_| Default::default()).collect(),
    }
}

fn collect_package(r: &mut Resolver<'_>, pkg: &Package, def: LocalDefId, parent: Option<LocalDefId>) {
    r.modules.insert(def, new_module(DefKind::Mod, pkg.name.name, parent, pkg.files.len()));
    for (file_index, file) in pkg.files.iter().enumerate() {
        let mut c = DefCollector { r, parent: def, package: def, file: file_index };
        for item in &file.items {
            c.visit_item(item);
        }
    }
    for child in &pkg.children {
        let child_def = r.create_def(Some(def), child.id, Some(child.name.name), DefKind::Mod, child.span);
        r.visibilities.insert(child_def, Visibility::Public);
        r.modules.get_mut(&def).unwrap().children.insert(child.name.name, child_def);
        collect_package(r, child, child_def, Some(def));
    }
}

struct DefCollector<'r, 'a> {
    r: &'r mut Resolver<'a>,
    parent: LocalDefId,
    package: LocalDefId,
    file: usize,
}

impl DefCollector<'_, '_> {
    fn create(&mut self, node: NodeId, name: Option<Ident>, kind: DefKind, span: Span) -> LocalDefId {
        self.r.create_def(Some(self.parent), node, name.map(|i| i.name), kind, span)
    }

    fn vis(&self, vis: &vsc_ast::Visibility) -> Visibility {
        match vis.kind {
            VisibilityKind::Public => Visibility::Public,
            VisibilityKind::Package => Visibility::Restricted(CRATE_DEF_ID),
            VisibilityKind::Inherited => Visibility::Restricted(self.package),
        }
    }

    fn with_parent(&mut self, parent: LocalDefId, f: impl FnOnce(&mut Self)) {
        let old = std::mem::replace(&mut self.parent, parent);
        f(self);
        self.parent = old;
    }

    fn record_generics(&mut self, def: LocalDefId, generics: &Generics) {
        let params = generics
            .params
            .iter()
            .map(|p| {
                let kind = match p.kind {
                    GenericParamKind::Origin => DefKind::OriginParam,
                    GenericParamKind::Type { .. } => DefKind::TyParam,
                    GenericParamKind::Const { .. } => DefKind::ConstParam,
                };
                (p.ident.name, kind)
            })
            .collect();
        self.r.generics_of.insert(def, params);
    }

    /// Fields, and a constructor for tuple and unit forms.
    fn collect_variant_data(&mut self, data: &VariantData, ctor_of: CtorOf, span: Span) {
        let ctor = match data {
            VariantData::Tuple(_, id) => Some((*id, CtorKind::Fn)),
            VariantData::Unit(id) => Some((*id, CtorKind::Const)),
            VariantData::Struct { .. } => None,
        };
        if let Some((id, kind)) = ctor {
            self.create(id, None, DefKind::Ctor(ctor_of, kind), span);
        }
        for f in data.fields() {
            let def = self.create(f.id, f.ident, DefKind::Field, f.span);
            let vis = self.vis(&f.vis);
            self.r.visibilities.insert(def, vis);
            self.with_parent(def, |this| this.visit_ty(&f.ty));
        }
    }
}

impl<'ast> Visitor<'ast> for DefCollector<'_, '_> {
    fn visit_item(&mut self, item: &'ast Item) {
        self.r.item_file.insert(item.id, (self.package, self.file));
        let ident = item.kind.ident();
        let kind = match &item.kind {
            ItemKind::Fn(_) => DefKind::Fn,
            ItemKind::TyAlias(_) => DefKind::TyAlias,
            ItemKind::Struct(..) => DefKind::Struct,
            ItemKind::Class(..) => DefKind::Class,
            ItemKind::Union(..) => DefKind::Union,
            ItemKind::Enum(..) => DefKind::Enum,
            ItemKind::Const(_) => DefKind::Const,
            ItemKind::Static(s) => DefKind::Static { mutability: s.mutability },
            ItemKind::Trait(_) => DefKind::Trait,
            ItemKind::Impl(i) => DefKind::Impl { of_trait: i.of_trait.is_some() },
            ItemKind::ForeignMod(_) => DefKind::ForeignMod,
            ItemKind::GlobalAsm(_) => DefKind::GlobalAsm,
        };
        let def = self.create(item.id, ident, kind, item.span);
        let vis = self.vis(&item.vis);
        self.r.visibilities.insert(def, vis);

        match &item.kind {
            ItemKind::Struct(_, generics, data) | ItemKind::Class(_, generics, data) | ItemKind::Union(_, generics, data) => {
                self.record_generics(def, generics);
                self.with_parent(def, |this| {
                    this.visit_generics(generics);
                    this.collect_variant_data(data, CtorOf::Struct, item.span);
                });
            }
            ItemKind::Enum(name, generics, enum_def) => {
                self.record_generics(def, generics);
                let package = self.package;
                self.r.modules.insert(def, new_module(DefKind::Enum, name.name, Some(package), 0));
                self.with_parent(def, |this| {
                    this.visit_generics(generics);
                    for v in &enum_def.variants {
                        let vdef = this.create(v.id, Some(v.ident), DefKind::Variant, v.span);
                        this.r.visibilities.insert(vdef, vis);
                        this.with_parent(vdef, |this| {
                            this.collect_variant_data(&v.data, CtorOf::Variant, v.span);
                            if let Some(d) = &v.disr_expr {
                                this.visit_anon_const(d);
                            }
                        });
                    }
                });
            }
            ItemKind::Trait(t) => {
                self.record_generics(def, &t.generics);
                let package = self.package;
                self.r.modules.insert(def, new_module(DefKind::Trait, t.ident.name, Some(package), 0));
                self.with_parent(def, |this| visit::walk_item(this, item));
            }
            ItemKind::TyAlias(t) => {
                self.record_generics(def, &t.generics);
                self.with_parent(def, |this| visit::walk_item(this, item));
            }
            ItemKind::Impl(i) => {
                if let ImplOrigin::FlatReceiver { introduced_params } = &i.origin {
                    // Kinds are decided once the receiver type is resolved.
                    let params: Vec<LocalDefId> = introduced_params
                        .iter()
                        .map(|p| self.r.create_def(Some(def), DUMMY_NODE_ID, Some(p.name), DefKind::TyParam, p.span))
                        .collect();
                    self.r.flat_receiver_params.insert(item.id, params);
                }
                self.with_parent(def, |this| visit::walk_item(this, item));
            }
            _ => self.with_parent(def, |this| visit::walk_item(this, item)),
        }
    }

    fn visit_assoc_item(&mut self, item: &'ast AssocItem, ctxt: AssocCtxt) {
        let (kind, ident) = match &item.kind {
            AssocItemKind::Fn(f) => (DefKind::AssocFn, f.ident),
            AssocItemKind::Const(c) => (DefKind::AssocConst, c.ident),
            AssocItemKind::Type(t) => (DefKind::AssocTy, t.ident),
        };
        let def = self.create(item.id, Some(ident), kind, item.span);
        let vis = match ctxt {
            // Trait items, and items of trait impls, are as visible as the trait.
            AssocCtxt::Trait | AssocCtxt::Impl { of_trait: true } => Visibility::Public,
            AssocCtxt::Impl { of_trait: false } => self.vis(&item.vis),
        };
        self.r.visibilities.insert(def, vis);
        self.with_parent(def, |this| visit::walk_assoc_item(this, item, ctxt));
    }

    fn visit_foreign_item(&mut self, item: &'ast ForeignItem) {
        let (kind, ident) = match &item.kind {
            ForeignItemKind::Fn(f) => (DefKind::Fn, f.ident),
            ForeignItemKind::Static(s) => (DefKind::Static { mutability: s.mutability }, s.ident),
            ForeignItemKind::TyAlias(t) => (DefKind::ForeignTy, t.ident),
        };
        self.r.item_file.insert(item.id, (self.package, self.file));
        let def = self.create(item.id, Some(ident), kind, item.span);
        let vis = self.vis(&item.vis);
        self.r.visibilities.insert(def, vis);
        self.with_parent(def, |this| visit::walk_foreign_item(this, item));
    }

    fn visit_generic_param(&mut self, p: &'ast GenericParam) {
        let kind = match p.kind {
            GenericParamKind::Origin => DefKind::OriginParam,
            GenericParamKind::Type { .. } => DefKind::TyParam,
            GenericParamKind::Const { .. } => DefKind::ConstParam,
        };
        self.create(p.id, Some(p.ident), kind, p.span);
        visit::walk_generic_param(self, p);
    }

    fn visit_fn(&mut self, kind: FnKind<'ast>) {
        visit::walk_fn(self, kind)
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        match &e.kind {
            ExprKind::Closure(_) | ExprKind::Async(_) => {
                let def = self.create(e.id, None, DefKind::Closure, e.span);
                self.with_parent(def, |this| visit::walk_expr(this, e));
            }
            // `vec[x; n]` takes a run-time count, unlike an array's length.
            ExprKind::Vec(inner) => match &inner.kind {
                ExprKind::Repeat(x, n) => {
                    self.visit_expr(x);
                    self.visit_expr(&n.value);
                }
                _ => self.visit_expr(inner),
            },
            ExprKind::ConstBlock(c) => {
                let def = self.create(c.id, None, DefKind::InlineConst, e.span);
                self.with_parent(def, |this| this.visit_expr(&c.value));
            }
            _ => visit::walk_expr(self, e),
        }
    }

    fn visit_anon_const(&mut self, c: &'ast AnonConst) {
        let def = self.create(c.id, None, DefKind::AnonConst, c.value.span);
        self.with_parent(def, |this| this.visit_expr(&c.value));
    }
}
