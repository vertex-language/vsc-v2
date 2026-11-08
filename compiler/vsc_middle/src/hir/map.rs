//! Finding a definition's HIR node (rustc's `hir::map`).

use vsc_data_structures::FxHashMap;
use vsc_hir::intravisit::{self, Visitor};
use vsc_hir::*;
use vsc_span::def_id::{CRATE_DEF_ID, LocalDefId};

/// The HIR node that defines something.
#[derive(Clone, Copy, Debug)]
pub enum Node<'hir> {
    Crate(&'hir Mod),
    Item(&'hir Item),
    AssocItem(&'hir AssocItem),
    ForeignItem(&'hir ForeignItem),
    Variant(&'hir Variant),
    Field(&'hir FieldDef),
    /// A tuple or unit struct's or variant's constructor.
    Ctor(&'hir VariantData),
    GenericParam(&'hir GenericParam),
    AnonConst(&'hir AnonConst),
    ConstBlock(&'hir ConstBlock),
    Closure(&'hir Closure),
}

pub fn index_crate(krate: &Crate) -> FxHashMap<LocalDefId, Node<'_>> {
    let mut v = Indexer { krate, nodes: FxHashMap::default() };
    if let OwnerNode::Crate(m) = &krate.owner(CRATE_DEF_ID).node {
        v.nodes.insert(CRATE_DEF_ID, Node::Crate(m));
        v.visit_mod(m);
    }
    v.nodes
}

struct Indexer<'hir> {
    krate: &'hir Crate,
    nodes: FxHashMap<LocalDefId, Node<'hir>>,
}

impl<'hir> Visitor<'hir> for Indexer<'hir> {
    fn krate(&self) -> Option<&'hir Crate> {
        Some(self.krate)
    }
    fn visit_item(&mut self, i: &'hir Item) {
        self.nodes.insert(i.owner_id, Node::Item(i));
        intravisit::walk_item(self, i)
    }
    fn visit_assoc_item(&mut self, i: &'hir AssocItem) {
        self.nodes.insert(i.owner_id, Node::AssocItem(i));
        intravisit::walk_assoc_item(self, i)
    }
    fn visit_foreign_item(&mut self, i: &'hir ForeignItem) {
        self.nodes.insert(i.owner_id, Node::ForeignItem(i));
        intravisit::walk_foreign_item(self, i)
    }
    fn visit_variant(&mut self, v: &'hir Variant) {
        self.nodes.insert(v.def_id, Node::Variant(v));
        intravisit::walk_variant(self, v)
    }
    fn visit_variant_data(&mut self, d: &'hir VariantData) {
        if let Some(ctor) = d.ctor_def_id() {
            self.nodes.insert(ctor, Node::Ctor(d));
        }
        intravisit::walk_variant_data(self, d)
    }
    fn visit_field_def(&mut self, f: &'hir FieldDef) {
        self.nodes.insert(f.def_id, Node::Field(f));
        intravisit::walk_field_def(self, f)
    }
    fn visit_generic_param(&mut self, p: &'hir GenericParam) {
        self.nodes.insert(p.def_id, Node::GenericParam(p));
        intravisit::walk_generic_param(self, p)
    }
    fn visit_anon_const(&mut self, c: &'hir AnonConst) {
        self.nodes.insert(c.def_id, Node::AnonConst(c));
        intravisit::walk_anon_const(self, c)
    }
    fn visit_expr(&mut self, e: &'hir Expr) {
        match &e.kind {
            ExprKind::Closure(c) => {
                self.nodes.insert(c.def_id, Node::Closure(c));
            }
            ExprKind::ConstBlock(c) => {
                self.nodes.insert(c.def_id, Node::ConstBlock(c));
            }
            _ => {}
        }
        intravisit::walk_expr(self, e)
    }
}
