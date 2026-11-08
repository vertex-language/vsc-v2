//! Walking the AST and changing it (rustc's `rustc_ast::mut_visit`).
//!
//! Expansion uses this to number nodes ([`MutVisitor::visit_id`]) and to strip
//! `@cfg`-disabled code ([`MutVisitor::keep`]): every list whose elements can
//! carry attributes (items, fields, variants, parameters, statements, arms,
//! struct-literal fields, pattern fields) asks `keep` about each element and
//! drops the ones it rejects.

use crate::ast::*;
use crate::node_id::NodeId;

pub trait MutVisitor: Sized {
    /// Called on every node id, before the node's children.
    fn visit_id(&mut self, _id: &mut NodeId) {}

    /// Whether to keep an element with these attributes. It may rewrite the
    /// attributes (`@cfgAttr`).
    fn keep(&mut self, _attrs: &mut AttrVec) -> bool {
        true
    }

    fn visit_crate(&mut self, krate: &mut Crate) {
        self.visit_id(&mut krate.id);
        self.visit_package(&mut krate.root);
    }

    fn visit_package(&mut self, package: &mut Package) {
        walk_package(self, package)
    }

    fn visit_item(&mut self, item: &mut Item) {
        walk_item(self, item)
    }

    fn visit_assoc_item(&mut self, item: &mut AssocItem) {
        walk_assoc_item(self, item)
    }

    fn visit_foreign_item(&mut self, item: &mut ForeignItem) {
        walk_foreign_item(self, item)
    }

    fn visit_expr(&mut self, e: &mut Expr) {
        walk_expr(self, e)
    }

    fn visit_pat(&mut self, p: &mut Pat) {
        walk_pat(self, p)
    }

    fn visit_ty(&mut self, t: &mut Ty) {
        walk_ty(self, t)
    }

    fn visit_block(&mut self, b: &mut Block) {
        walk_block(self, b)
    }
}

/// Keeps the elements `keep` accepts. `attrs` finds an element's attributes.
fn filter<V: MutVisitor, T>(v: &mut V, list: &mut Vec<T>, mut attrs: impl FnMut(&mut T) -> Option<&mut AttrVec>) {
    list.retain_mut(|elem| match attrs(elem) {
        Some(a) => v.keep(a),
        None => true,
    });
}

pub fn walk_package<V: MutVisitor>(v: &mut V, package: &mut Package) {
    v.visit_id(&mut package.id);
    for file in &mut package.files {
        if let Some(p) = &mut file.package {
            for a in &mut p.attrs {
                walk_attribute(v, a);
            }
        }
        filter(v, &mut file.items, |i| Some(&mut i.attrs));
        for item in &mut file.items {
            v.visit_item(item);
        }
    }
    for child in &mut package.children {
        v.visit_package(child);
    }
}

fn walk_attribute<V: MutVisitor>(v: &mut V, a: &mut Attribute) {
    if let AttrKind::Normal(n) = &mut a.kind {
        walk_path(v, &mut n.path);
        if let AttrArgs::Eq { expr, .. } = &mut n.args {
            v.visit_expr(expr);
        }
    }
}

fn walk_attrs<V: MutVisitor>(v: &mut V, attrs: &mut AttrVec) {
    for a in attrs {
        walk_attribute(v, a);
    }
}

pub fn walk_item<V: MutVisitor>(v: &mut V, item: &mut Item) {
    v.visit_id(&mut item.id);
    walk_attrs(v, &mut item.attrs);
    match &mut item.kind {
        ItemKind::Fn(f) => walk_fn(v, f),
        ItemKind::TyAlias(t) => walk_ty_alias(v, t),
        ItemKind::Struct(_, generics, data) | ItemKind::Class(_, generics, data) | ItemKind::Union(_, generics, data) => {
            walk_generics(v, generics);
            walk_variant_data(v, data);
        }
        ItemKind::Enum(_, generics, def) => {
            walk_generics(v, generics);
            filter(v, &mut def.variants, |var| Some(&mut var.attrs));
            for var in &mut def.variants {
                v.visit_id(&mut var.id);
                walk_attrs(v, &mut var.attrs);
                walk_variant_data(v, &mut var.data);
                if let Some(d) = &mut var.disr_expr {
                    walk_anon_const(v, d);
                }
            }
        }
        ItemKind::Const(c) => walk_const_item(v, c),
        ItemKind::Static(s) => walk_static_item(v, s),
        ItemKind::Trait(t) => {
            walk_generics(v, &mut t.generics);
            walk_bounds(v, &mut t.bounds);
            filter(v, &mut t.items, |i| Some(&mut i.attrs));
            for i in &mut t.items {
                v.visit_assoc_item(i);
            }
        }
        ItemKind::Impl(i) => {
            walk_generics(v, &mut i.generics);
            if let Some(t) = &mut i.of_trait {
                walk_trait_ref(v, t);
            }
            v.visit_ty(&mut i.self_ty);
            filter(v, &mut i.items, |i| Some(&mut i.attrs));
            for item in &mut i.items {
                v.visit_assoc_item(item);
            }
        }
        ItemKind::ForeignMod(m) => {
            filter(v, &mut m.items, |i| Some(&mut i.attrs));
            for i in &mut m.items {
                v.visit_foreign_item(i);
            }
        }
        ItemKind::GlobalAsm(asm) => walk_inline_asm(v, asm),
    }
}

pub fn walk_assoc_item<V: MutVisitor>(v: &mut V, item: &mut AssocItem) {
    v.visit_id(&mut item.id);
    walk_attrs(v, &mut item.attrs);
    match &mut item.kind {
        AssocItemKind::Fn(f) => walk_fn(v, f),
        AssocItemKind::Const(c) => walk_const_item(v, c),
        AssocItemKind::Type(t) => walk_ty_alias(v, t),
    }
}

pub fn walk_foreign_item<V: MutVisitor>(v: &mut V, item: &mut ForeignItem) {
    v.visit_id(&mut item.id);
    walk_attrs(v, &mut item.attrs);
    match &mut item.kind {
        ForeignItemKind::Fn(f) => walk_fn(v, f),
        ForeignItemKind::Static(s) => walk_static_item(v, s),
        ForeignItemKind::TyAlias(t) => walk_ty_alias(v, t),
    }
}

fn walk_fn<V: MutVisitor>(v: &mut V, f: &mut Fn) {
    walk_generics(v, &mut f.generics);
    walk_fn_decl(v, &mut f.sig.decl);
    if let Some(b) = &mut f.body {
        v.visit_block(b);
    }
}

fn walk_ty_alias<V: MutVisitor>(v: &mut V, t: &mut TyAlias) {
    walk_generics(v, &mut t.generics);
    walk_bounds(v, &mut t.bounds);
    if let Some(ty) = &mut t.ty {
        v.visit_ty(ty);
    }
}

fn walk_const_item<V: MutVisitor>(v: &mut V, c: &mut ConstItem) {
    v.visit_ty(&mut c.ty);
    if let Some(e) = &mut c.expr {
        v.visit_expr(e);
    }
}

fn walk_static_item<V: MutVisitor>(v: &mut V, s: &mut StaticItem) {
    v.visit_ty(&mut s.ty);
    if let Some(e) = &mut s.expr {
        v.visit_expr(e);
    }
}

fn walk_fn_decl<V: MutVisitor>(v: &mut V, decl: &mut FnDecl) {
    filter(v, &mut decl.inputs, |p| Some(&mut p.attrs));
    for p in &mut decl.inputs {
        v.visit_id(&mut p.id);
        walk_attrs(v, &mut p.attrs);
        v.visit_pat(&mut p.pat);
        v.visit_ty(&mut p.ty);
    }
    if let FnRetTy::Ty(t) = &mut decl.output {
        v.visit_ty(t);
    }
}

fn walk_generics<V: MutVisitor>(v: &mut V, g: &mut Generics) {
    filter(v, &mut g.params, |p| Some(&mut p.attrs));
    for p in &mut g.params {
        walk_generic_param(v, p);
    }
    for pred in &mut g.where_clause.predicates {
        v.visit_id(&mut pred.id);
        match &mut pred.kind {
            WherePredicateKind::BoundPredicate { bound_generic_params, bounded_ty, bounds } => {
                for gp in bound_generic_params {
                    walk_generic_param(v, gp);
                }
                v.visit_ty(bounded_ty);
                walk_bounds(v, bounds);
            }
        }
    }
}

fn walk_generic_param<V: MutVisitor>(v: &mut V, p: &mut GenericParam) {
    v.visit_id(&mut p.id);
    walk_attrs(v, &mut p.attrs);
    walk_bounds(v, &mut p.bounds);
    match &mut p.kind {
        GenericParamKind::Lifetime => {}
        GenericParamKind::Type { default } => {
            if let Some(d) = default {
                v.visit_ty(d);
            }
        }
        GenericParamKind::Const { ty, default, .. } => {
            v.visit_ty(ty);
            if let Some(d) = default {
                walk_anon_const(v, d);
            }
        }
    }
}

fn walk_bounds<V: MutVisitor>(v: &mut V, bounds: &mut GenericBounds) {
    for b in bounds {
        if let GenericBound::Trait(t) = b {
            for gp in &mut t.bound_generic_params {
                walk_generic_param(v, gp);
            }
            walk_trait_ref(v, &mut t.trait_ref);
        }
    }
}

fn walk_trait_ref<V: MutVisitor>(v: &mut V, t: &mut TraitRef) {
    v.visit_id(&mut t.ref_id);
    walk_path(v, &mut t.path);
}

fn walk_variant_data<V: MutVisitor>(v: &mut V, data: &mut VariantData) {
    match data {
        VariantData::Struct { fields } => walk_fields(v, fields),
        VariantData::Tuple(fields, id) => {
            v.visit_id(id);
            walk_fields(v, fields);
        }
        VariantData::Unit(id) => v.visit_id(id),
    }
}

fn walk_fields<V: MutVisitor>(v: &mut V, fields: &mut Vec<FieldDef>) {
    filter(v, fields, |f| Some(&mut f.attrs));
    for f in fields {
        v.visit_id(&mut f.id);
        walk_attrs(v, &mut f.attrs);
        v.visit_ty(&mut f.ty);
    }
}

fn walk_anon_const<V: MutVisitor>(v: &mut V, c: &mut AnonConst) {
    v.visit_id(&mut c.id);
    v.visit_expr(&mut c.value);
}

fn walk_qself<V: MutVisitor>(v: &mut V, q: &mut Option<P<QSelf>>) {
    if let Some(q) = q {
        v.visit_ty(&mut q.ty);
    }
}

fn walk_path<V: MutVisitor>(v: &mut V, path: &mut Path) {
    for seg in &mut path.segments {
        v.visit_id(&mut seg.id);
        if let Some(args) = &mut seg.args {
            walk_generic_args(v, args);
        }
    }
}

fn walk_generic_args<V: MutVisitor>(v: &mut V, args: &mut GenericArgs) {
    match args {
        GenericArgs::AngleBracketed(a) => {
            for arg in &mut a.args {
                match arg {
                    AngleBracketedArg::Arg(GenericArg::Type(t)) => v.visit_ty(t),
                    AngleBracketedArg::Arg(GenericArg::Const(c)) => walk_anon_const(v, c),
                    AngleBracketedArg::Arg(GenericArg::Lifetime(_)) => {}
                    AngleBracketedArg::Constraint(c) => {
                        v.visit_id(&mut c.id);
                        if let Some(args) = &mut c.gen_args {
                            walk_generic_args(v, args);
                        }
                        match &mut c.kind {
                            AssocItemConstraintKind::Equality { term } => v.visit_ty(term),
                            AssocItemConstraintKind::Bound { bounds } => walk_bounds(v, bounds),
                        }
                    }
                }
            }
        }
        GenericArgs::Parenthesized(p) => {
            for t in &mut p.inputs {
                v.visit_ty(t);
            }
            if let FnRetTy::Ty(t) = &mut p.output {
                v.visit_ty(t);
            }
        }
    }
}

pub fn walk_ty<V: MutVisitor>(v: &mut V, ty: &mut Ty) {
    v.visit_id(&mut ty.id);
    match &mut ty.kind {
        TyKind::Slice(t) | TyKind::Paren(t) => v.visit_ty(t),
        TyKind::Array(t, len) => {
            v.visit_ty(t);
            walk_anon_const(v, len);
        }
        TyKind::Ptr(mt) | TyKind::Ref(_, mt) => v.visit_ty(&mut mt.ty),
        TyKind::FnPtr(f) => {
            for gp in &mut f.generic_params {
                walk_generic_param(v, gp);
            }
            walk_fn_decl(v, &mut f.decl);
        }
        TyKind::FuncValue(f) => {
            for gp in &mut f.generic_params {
                walk_generic_param(v, gp);
            }
            walk_fn_decl(v, &mut f.decl);
        }
        TyKind::Tup(ts) => {
            for t in ts {
                v.visit_ty(t);
            }
        }
        TyKind::Path(qself, path) => {
            walk_qself(v, qself);
            walk_path(v, path);
        }
        TyKind::TraitObject(bounds) => walk_bounds(v, bounds),
        TyKind::ImplTrait(id, bounds) => {
            v.visit_id(id);
            walk_bounds(v, bounds);
        }
        TyKind::Never | TyKind::Infer | TyKind::ImplicitSelf | TyKind::CVarArgs | TyKind::Err => {}
    }
}

pub fn walk_pat<V: MutVisitor>(v: &mut V, pat: &mut Pat) {
    v.visit_id(&mut pat.id);
    match &mut pat.kind {
        PatKind::Ident(_, _, sub) => {
            if let Some(s) = sub {
                v.visit_pat(s);
            }
        }
        PatKind::Struct(qself, path, fields, _) => {
            walk_qself(v, qself);
            walk_path(v, path);
            filter(v, fields, |f| Some(&mut f.attrs));
            for f in fields {
                v.visit_id(&mut f.id);
                walk_attrs(v, &mut f.attrs);
                v.visit_pat(&mut f.pat);
            }
        }
        PatKind::TupleStruct(qself, path, pats) => {
            walk_qself(v, qself);
            walk_path(v, path);
            for p in pats {
                v.visit_pat(p);
            }
        }
        PatKind::Path(qself, path) => {
            walk_qself(v, qself);
            walk_path(v, path);
        }
        PatKind::Or(pats) | PatKind::Tuple(pats) | PatKind::Slice(pats) => {
            for p in pats {
                v.visit_pat(p);
            }
        }
        PatKind::Ref(p, _) | PatKind::Paren(p) => v.visit_pat(p),
        PatKind::Expr(e) => v.visit_expr(e),
        PatKind::Range(lo, hi, _) => {
            if let Some(e) = lo {
                v.visit_expr(e);
            }
            if let Some(e) = hi {
                v.visit_expr(e);
            }
        }
        PatKind::Wild | PatKind::Rest | PatKind::Err => {}
    }
}

pub fn walk_block<V: MutVisitor>(v: &mut V, b: &mut Block) {
    v.visit_id(&mut b.id);
    filter(v, &mut b.stmts, |s| match &mut s.kind {
        StmtKind::Let(l) => Some(&mut l.attrs),
        StmtKind::Item(i) => Some(&mut i.attrs),
        StmtKind::Expr(e) => Some(&mut e.attrs),
    });
    for s in &mut b.stmts {
        v.visit_id(&mut s.id);
        match &mut s.kind {
            StmtKind::Let(l) => {
                v.visit_id(&mut l.id);
                walk_attrs(v, &mut l.attrs);
                v.visit_pat(&mut l.pat);
                if let Some(t) = &mut l.ty {
                    v.visit_ty(t);
                }
                match &mut l.kind {
                    LocalKind::Decl => {}
                    LocalKind::Init(e) => v.visit_expr(e),
                    LocalKind::InitElse(e, b) => {
                        v.visit_expr(e);
                        v.visit_block(b);
                    }
                }
            }
            StmtKind::Item(i) => v.visit_item(i),
            StmtKind::Expr(e) => v.visit_expr(e),
        }
    }
}

pub fn walk_expr<V: MutVisitor>(v: &mut V, e: &mut Expr) {
    v.visit_id(&mut e.id);
    walk_attrs(v, &mut e.attrs);
    match &mut e.kind {
        ExprKind::Array(es) | ExprKind::Tup(es) => {
            for x in es {
                v.visit_expr(x);
            }
        }
        ExprKind::Repeat(x, n) => {
            v.visit_expr(x);
            walk_anon_const(v, n);
        }
        ExprKind::Vec(x) | ExprKind::Paren(x) | ExprKind::Try(x) | ExprKind::Unary(_, x) | ExprKind::Await(x, _) => {
            v.visit_expr(x)
        }
        ExprKind::ConstBlock(c) => walk_anon_const(v, c),
        ExprKind::Call(f, args) => {
            v.visit_expr(f);
            for a in args {
                v.visit_expr(a);
            }
        }
        ExprKind::MethodCall(m) => {
            v.visit_expr(&mut m.receiver);
            v.visit_id(&mut m.seg.id);
            for a in &mut m.args {
                v.visit_expr(a);
            }
        }
        ExprKind::Binary(_, l, r) | ExprKind::Assign(l, r, _) | ExprKind::AssignOp(_, l, r) => {
            v.visit_expr(l);
            v.visit_expr(r);
        }
        ExprKind::Lit(_) | ExprKind::Underscore | ExprKind::Err | ExprKind::Continue(_) => {}
        ExprKind::Cast(x, t) => {
            v.visit_expr(x);
            v.visit_ty(t);
        }
        ExprKind::Let(p, x, _) => {
            v.visit_pat(p);
            v.visit_expr(x);
        }
        ExprKind::If(c, then, els) => {
            v.visit_expr(c);
            v.visit_block(then);
            if let Some(x) = els {
                v.visit_expr(x);
            }
        }
        ExprKind::While(c, body, _) => {
            v.visit_expr(c);
            v.visit_block(body);
        }
        ExprKind::ForLoop { pat, iter, body, .. } => {
            v.visit_pat(pat);
            v.visit_expr(iter);
            v.visit_block(body);
        }
        ExprKind::Loop(body, ..) | ExprKind::Block(body, _) | ExprKind::Async(body) => v.visit_block(body),
        ExprKind::Match(x, arms) => {
            v.visit_expr(x);
            filter(v, arms, |a| Some(&mut a.attrs));
            for a in arms {
                v.visit_id(&mut a.id);
                walk_attrs(v, &mut a.attrs);
                v.visit_pat(&mut a.pat);
                if let Some(g) = &mut a.guard {
                    v.visit_expr(g);
                }
                v.visit_expr(&mut a.body);
            }
        }
        ExprKind::Closure(c) => {
            walk_fn_decl(v, &mut c.fn_decl);
            v.visit_block(&mut c.body);
        }
        ExprKind::Field(x, _) => v.visit_expr(x),
        ExprKind::Index(x, i, _) => {
            v.visit_expr(x);
            v.visit_expr(i);
        }
        ExprKind::Range(lo, hi, _) => {
            if let Some(x) = lo {
                v.visit_expr(x);
            }
            if let Some(x) = hi {
                v.visit_expr(x);
            }
        }
        ExprKind::Path(qself, path) => {
            walk_qself(v, qself);
            walk_path(v, path);
        }
        ExprKind::AddrOf(_, _, x) => v.visit_expr(x),
        ExprKind::Break(_, x) | ExprKind::Ret(x) => {
            if let Some(x) = x {
                v.visit_expr(x);
            }
        }
        ExprKind::InlineAsm(asm) => walk_inline_asm(v, asm),
        ExprKind::Builtin(b) => {
            let fmt = |v: &mut V, f: &mut Option<P<FormatArgs>>| {
                if let Some(f) = f {
                    for a in &mut f.arguments {
                        v.visit_expr(&mut a.expr);
                    }
                }
            };
            match &mut b.kind {
                BuiltinKind::Print { args, .. } | BuiltinKind::Format(args) => {
                    for a in &mut args.arguments {
                        v.visit_expr(&mut a.expr);
                    }
                }
                BuiltinKind::Panic(f) | BuiltinKind::Todo(f) | BuiltinKind::Unreachable(f) => fmt(v, f),
                BuiltinKind::Assert { cond, msg } => {
                    v.visit_expr(cond);
                    fmt(v, msg);
                }
                BuiltinKind::AssertEq { left, right, msg } => {
                    v.visit_expr(left);
                    v.visit_expr(right);
                    fmt(v, msg);
                }
            }
        }
        ExprKind::Struct(s) => {
            walk_qself(v, &mut s.qself);
            walk_path(v, &mut s.path);
            filter(v, &mut s.fields, |f| Some(&mut f.attrs));
            for f in &mut s.fields {
                v.visit_id(&mut f.id);
                walk_attrs(v, &mut f.attrs);
                v.visit_expr(&mut f.expr);
            }
            if let StructRest::Base(b) = &mut s.rest {
                v.visit_expr(b);
            }
        }
    }
}

fn walk_inline_asm<V: MutVisitor>(v: &mut V, asm: &mut InlineAsm) {
    for (op, _) in &mut asm.operands {
        match op {
            InlineAsmOperand::In { expr, .. } | InlineAsmOperand::InOut { expr, .. } => v.visit_expr(expr),
            InlineAsmOperand::Out { expr, .. } => {
                if let Some(e) = expr {
                    v.visit_expr(e);
                }
            }
            InlineAsmOperand::SplitInOut { in_expr, out_expr, .. } => {
                v.visit_expr(in_expr);
                if let Some(e) = out_expr {
                    v.visit_expr(e);
                }
            }
            InlineAsmOperand::Const { anon_const, .. } => walk_anon_const(v, anon_const),
            InlineAsmOperand::Sym { id, qself, path, .. } => {
                v.visit_id(id);
                walk_qself(v, qself);
                walk_path(v, path);
            }
            InlineAsmOperand::Label { block, .. } => v.visit_block(block),
        }
    }
}
