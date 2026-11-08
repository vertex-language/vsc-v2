//! The names each package, enum and trait defines (rustc's
//! `build_reduced_graph.rs`).
//!
//! A package's names come from all its files. Child packages aren't names in
//! their parent: as in Go, a file imports the packages it uses.

use vsc_ast::*;
use vsc_hir::def::{DefKind, Namespace, Res};
use vsc_span::Ident;
use vsc_span::def_id::LocalDefId;

use crate::{Binding, Resolver, Visibility};

pub(crate) fn build(r: &mut Resolver<'_>, krate: &Crate) {
    build_package(r, &krate.root);
}

fn build_package(r: &mut Resolver<'_>, pkg: &Package) {
    let module = r.local_def_id(pkg.id);
    for file in &pkg.files {
        for item in &file.items {
            add_item(r, module, item);
        }
    }
    for child in &pkg.children {
        build_package(r, child);
    }
}

impl Resolver<'_> {
    fn res_of(&self, node: NodeId) -> Res {
        let def = self.local_def_id(node);
        Res::Def(self.defs[def].kind, def.to_def_id())
    }

    /// Defines `ident` in `module`'s namespace `ns`, reporting duplicates and
    /// built-in names.
    pub(crate) fn define(&mut self, module: LocalDefId, ident: Ident, ns: Namespace, res: Res, vis: Visibility) {
        if ident.name == kw::Underscore {
            return;
        }
        if ident.name.is_builtin_name() {
            self.report_builtin_redeclared(ident);
            return;
        }
        let m = self.modules.get_mut(&module).unwrap();
        if let Some(prev) = m.bindings.get(&(ident.name, ns)).copied() {
            let in_what = match m.kind {
                DefKind::Enum => "enum",
                DefKind::Trait => "trait",
                _ => "package",
            };
            self.dcx()
                .struct_span_err(ident.span, format!("the name `{ident}` is defined multiple times"))
                .with_code(vsc_errors::ErrCode(428))
                .with_span_label(prev.span, format!("previous definition of the {} `{ident}` here", prev.res.descr()))
                .with_span_label(ident.span, format!("`{ident}` redefined here"))
                .with_note(format!("`{ident}` must be defined only once in the {} namespace of this {in_what}", ns.descr()))
                .emit();
            return;
        }
        m.bindings.insert((ident.name, ns), Binding { res, vis, span: ident.span });
    }
}

use vsc_span::kw;

fn add_item(r: &mut Resolver<'_>, module: LocalDefId, item: &Item) {
    let def = r.local_def_id(item.id);
    let vis = r.visibilities[&def];
    let res = r.res_of(item.id);
    match &item.kind {
        ItemKind::Fn(f) => r.define(module, f.ident, Namespace::ValueNS, res, vis),
        ItemKind::Const(c) => r.define(module, c.ident, Namespace::ValueNS, res, vis),
        ItemKind::Static(s) => r.define(module, s.ident, Namespace::ValueNS, res, vis),
        ItemKind::TyAlias(t) => r.define(module, t.ident, Namespace::TypeNS, res, vis),
        ItemKind::Class(ident, ..) | ItemKind::Union(ident, ..) => r.define(module, *ident, Namespace::TypeNS, res, vis),
        ItemKind::Struct(ident, _, data) => {
            r.define(module, *ident, Namespace::TypeNS, res, vis);
            if let Some(ctor) = ctor_id(data) {
                let ctor_res = r.res_of(ctor);
                r.define(module, *ident, Namespace::ValueNS, ctor_res, vis);
            }
        }
        ItemKind::Enum(ident, _, enum_def) => {
            r.define(module, *ident, Namespace::TypeNS, res, vis);
            for v in &enum_def.variants {
                let vres = r.res_of(v.id);
                r.define(def, v.ident, Namespace::TypeNS, vres, Visibility::Public);
                if let Some(ctor) = ctor_id(&v.data) {
                    let ctor_res = r.res_of(ctor);
                    r.define(def, v.ident, Namespace::ValueNS, ctor_res, Visibility::Public);
                }
            }
        }
        ItemKind::Trait(t) => {
            r.define(module, t.ident, Namespace::TypeNS, res, vis);
            for ai in &t.items {
                let ares = r.res_of(ai.id);
                let (ident, ns) = match &ai.kind {
                    AssocItemKind::Fn(f) => (f.ident, Namespace::ValueNS),
                    AssocItemKind::Const(c) => (c.ident, Namespace::ValueNS),
                    AssocItemKind::Type(t) => (t.ident, Namespace::TypeNS),
                };
                r.define(def, ident, ns, ares, Visibility::Public);
            }
        }
        ItemKind::ForeignMod(m) => {
            for fi in &m.items {
                let fdef = r.local_def_id(fi.id);
                let fvis = r.visibilities[&fdef];
                let fres = r.res_of(fi.id);
                match &fi.kind {
                    ForeignItemKind::Fn(f) => r.define(module, f.ident, Namespace::ValueNS, fres, fvis),
                    ForeignItemKind::Static(s) => r.define(module, s.ident, Namespace::ValueNS, fres, fvis),
                    ForeignItemKind::TyAlias(t) => r.define(module, t.ident, Namespace::TypeNS, fres, fvis),
                }
            }
        }
        ItemKind::Impl(_) | ItemKind::GlobalAsm(_) => {}
    }
}

/// The constructor's node id of a tuple or unit struct or variant.
pub(crate) fn ctor_id(data: &VariantData) -> Option<NodeId> {
    match data {
        VariantData::Tuple(_, id) | VariantData::Unit(id) => Some(*id),
        VariantData::Struct { .. } => None,
    }
}
