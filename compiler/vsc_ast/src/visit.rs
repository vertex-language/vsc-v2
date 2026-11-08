//! Walking the AST without changing it (rustc's `rustc_ast::visit`).
//!
//! Each `visit_*` method defaults to the matching `walk_*` function, which
//! visits the node's children. Override a method to act on a node, and call
//! the `walk_*` function from it to keep walking.

use vsc_span::Ident;

use crate::ast::*;
use crate::node_id::NodeId;

/// Where an associated item is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssocCtxt {
    Trait,
    Impl { of_trait: bool },
}

/// Where a function is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FnCtxt {
    Free,
    Foreign,
    Assoc(AssocCtxt),
}

#[derive(Clone, Copy, Debug)]
pub enum FnKind<'a> {
    /// A named function.
    Fn(FnCtxt, &'a Visibility, &'a Fn),
    /// A function literal.
    Closure(&'a Closure),
}

impl<'a> FnKind<'a> {
    pub fn decl(&self) -> &'a FnDecl {
        match self {
            FnKind::Fn(_, _, f) => &f.sig.decl,
            FnKind::Closure(c) => &c.fn_decl,
        }
    }
}

pub trait Visitor<'ast>: Sized {
    fn visit_crate(&mut self, krate: &'ast Crate) {
        walk_crate(self, krate)
    }
    fn visit_package(&mut self, package: &'ast Package) {
        walk_package(self, package)
    }
    fn visit_source_file(&mut self, file: &'ast SourceFile) {
        walk_source_file(self, file)
    }
    fn visit_import(&mut self, _import: &'ast Import) {}
    fn visit_ident(&mut self, _ident: &'ast Ident) {}
    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        walk_attribute(self, attr)
    }
    fn visit_item(&mut self, item: &'ast Item) {
        walk_item(self, item)
    }
    fn visit_assoc_item(&mut self, item: &'ast AssocItem, ctxt: AssocCtxt) {
        walk_assoc_item(self, item, ctxt)
    }
    fn visit_foreign_item(&mut self, item: &'ast ForeignItem) {
        walk_foreign_item(self, item)
    }
    fn visit_fn(&mut self, kind: FnKind<'ast>) {
        walk_fn(self, kind)
    }
    fn visit_fn_decl(&mut self, decl: &'ast FnDecl) {
        walk_fn_decl(self, decl)
    }
    fn visit_param(&mut self, param: &'ast Param) {
        walk_param(self, param)
    }
    fn visit_fn_ret_ty(&mut self, ret: &'ast FnRetTy) {
        walk_fn_ret_ty(self, ret)
    }
    fn visit_generics(&mut self, generics: &'ast Generics) {
        walk_generics(self, generics)
    }
    fn visit_generic_param(&mut self, param: &'ast GenericParam) {
        walk_generic_param(self, param)
    }
    fn visit_where_predicate(&mut self, pred: &'ast WherePredicate) {
        walk_where_predicate(self, pred)
    }
    fn visit_param_bound(&mut self, bound: &'ast GenericBound) {
        walk_param_bound(self, bound)
    }
    fn visit_poly_trait_ref(&mut self, t: &'ast PolyTraitRef) {
        walk_poly_trait_ref(self, t)
    }
    fn visit_trait_ref(&mut self, t: &'ast TraitRef) {
        walk_trait_ref(self, t)
    }
    fn visit_lifetime_ref(&mut self, _o: &'ast Lifetime) {}
    fn visit_variant_data(&mut self, data: &'ast VariantData) {
        walk_variant_data(self, data)
    }
    fn visit_variant(&mut self, v: &'ast Variant) {
        walk_variant(self, v)
    }
    fn visit_field_def(&mut self, f: &'ast FieldDef) {
        walk_field_def(self, f)
    }
    fn visit_vis(&mut self, _vis: &'ast Visibility) {}
    fn visit_ty(&mut self, ty: &'ast Ty) {
        walk_ty(self, ty)
    }
    fn visit_qself(&mut self, qself: &'ast Option<P<QSelf>>) {
        if let Some(q) = qself {
            self.visit_ty(&q.ty);
        }
    }
    fn visit_path(&mut self, path: &'ast Path, _id: NodeId) {
        walk_path(self, path)
    }
    fn visit_path_segment(&mut self, seg: &'ast PathSegment) {
        walk_path_segment(self, seg)
    }
    fn visit_generic_args(&mut self, args: &'ast GenericArgs) {
        walk_generic_args(self, args)
    }
    fn visit_generic_arg(&mut self, arg: &'ast GenericArg) {
        walk_generic_arg(self, arg)
    }
    fn visit_assoc_item_constraint(&mut self, c: &'ast AssocItemConstraint) {
        walk_assoc_item_constraint(self, c)
    }
    fn visit_anon_const(&mut self, c: &'ast AnonConst) {
        self.visit_expr(&c.value)
    }
    fn visit_pat(&mut self, pat: &'ast Pat) {
        walk_pat(self, pat)
    }
    fn visit_pat_field(&mut self, f: &'ast PatField) {
        walk_pat_field(self, f)
    }
    fn visit_expr(&mut self, e: &'ast Expr) {
        walk_expr(self, e)
    }
    fn visit_expr_field(&mut self, f: &'ast ExprField) {
        walk_expr_field(self, f)
    }
    fn visit_arm(&mut self, arm: &'ast Arm) {
        walk_arm(self, arm)
    }
    fn visit_label(&mut self, _label: &'ast Label) {}
    fn visit_block(&mut self, b: &'ast Block) {
        walk_block(self, b)
    }
    fn visit_stmt(&mut self, s: &'ast Stmt) {
        walk_stmt(self, s)
    }
    fn visit_local(&mut self, l: &'ast Local) {
        walk_local(self, l)
    }
    fn visit_inline_asm(&mut self, asm: &'ast InlineAsm) {
        walk_inline_asm(self, asm)
    }
}

pub fn walk_crate<'a, V: Visitor<'a>>(v: &mut V, krate: &'a Crate) {
    v.visit_package(&krate.root);
}

pub fn walk_package<'a, V: Visitor<'a>>(v: &mut V, package: &'a Package) {
    for file in &package.files {
        v.visit_source_file(file);
    }
    for child in &package.children {
        v.visit_package(child);
    }
}

pub fn walk_source_file<'a, V: Visitor<'a>>(v: &mut V, file: &'a SourceFile) {
    for a in &file.inner_attrs {
        v.visit_attribute(a);
    }
    if let Some(p) = &file.package {
        for a in &p.attrs {
            v.visit_attribute(a);
        }
    }
    for i in &file.imports {
        v.visit_import(i);
    }
    for item in &file.items {
        v.visit_item(item);
    }
}

pub fn walk_attribute<'a, V: Visitor<'a>>(v: &mut V, attr: &'a Attribute) {
    if let AttrKind::Normal(n) = &attr.kind {
        if let AttrArgs::Eq { expr, .. } = &n.args {
            v.visit_expr(expr);
        }
    }
}

fn walk_attrs<'a, V: Visitor<'a>>(v: &mut V, attrs: &'a [Attribute]) {
    for a in attrs {
        v.visit_attribute(a);
    }
}

pub fn walk_item<'a, V: Visitor<'a>>(v: &mut V, item: &'a Item) {
    walk_attrs(v, &item.attrs);
    v.visit_vis(&item.vis);
    match &item.kind {
        ItemKind::Fn(f) => {
            v.visit_ident(&f.ident);
            v.visit_fn(FnKind::Fn(FnCtxt::Free, &item.vis, f));
        }
        ItemKind::TyAlias(t) => walk_ty_alias(v, t),
        ItemKind::Struct(ident, generics, data)
        | ItemKind::Class(ident, generics, data)
        | ItemKind::Union(ident, generics, data) => {
            v.visit_ident(ident);
            v.visit_generics(generics);
            v.visit_variant_data(data);
        }
        ItemKind::Enum(ident, generics, def) => {
            v.visit_ident(ident);
            v.visit_generics(generics);
            for var in &def.variants {
                v.visit_variant(var);
            }
        }
        ItemKind::Const(c) => walk_const_item(v, c),
        ItemKind::Static(s) => walk_static_item(v, s),
        ItemKind::Trait(t) => {
            v.visit_ident(&t.ident);
            v.visit_generics(&t.generics);
            for b in &t.bounds {
                v.visit_param_bound(b);
            }
            for i in &t.items {
                v.visit_assoc_item(i, AssocCtxt::Trait);
            }
        }
        ItemKind::Impl(i) => {
            v.visit_generics(&i.generics);
            if let Some(t) = &i.of_trait {
                v.visit_trait_ref(t);
            }
            v.visit_ty(&i.self_ty);
            for item in &i.items {
                v.visit_assoc_item(item, AssocCtxt::Impl { of_trait: i.of_trait.is_some() });
            }
        }
        ItemKind::ForeignMod(m) => {
            for i in &m.items {
                v.visit_foreign_item(i);
            }
        }
        ItemKind::GlobalAsm(asm) => v.visit_inline_asm(asm),
    }
}

pub fn walk_ty_alias<'a, V: Visitor<'a>>(v: &mut V, t: &'a TyAlias) {
    v.visit_ident(&t.ident);
    v.visit_generics(&t.generics);
    for b in &t.bounds {
        v.visit_param_bound(b);
    }
    if let Some(ty) = &t.ty {
        v.visit_ty(ty);
    }
}

pub fn walk_const_item<'a, V: Visitor<'a>>(v: &mut V, c: &'a ConstItem) {
    v.visit_ident(&c.ident);
    v.visit_ty(&c.ty);
    if let Some(e) = &c.expr {
        v.visit_expr(e);
    }
}

pub fn walk_static_item<'a, V: Visitor<'a>>(v: &mut V, s: &'a StaticItem) {
    v.visit_ident(&s.ident);
    v.visit_ty(&s.ty);
    if let Some(e) = &s.expr {
        v.visit_expr(e);
    }
}

pub fn walk_assoc_item<'a, V: Visitor<'a>>(v: &mut V, item: &'a AssocItem, ctxt: AssocCtxt) {
    walk_attrs(v, &item.attrs);
    v.visit_vis(&item.vis);
    match &item.kind {
        AssocItemKind::Fn(f) => {
            v.visit_ident(&f.ident);
            v.visit_fn(FnKind::Fn(FnCtxt::Assoc(ctxt), &item.vis, f));
        }
        AssocItemKind::Const(c) => walk_const_item(v, c),
        AssocItemKind::Type(t) => walk_ty_alias(v, t),
    }
}

pub fn walk_foreign_item<'a, V: Visitor<'a>>(v: &mut V, item: &'a ForeignItem) {
    walk_attrs(v, &item.attrs);
    v.visit_vis(&item.vis);
    match &item.kind {
        ForeignItemKind::Fn(f) => {
            v.visit_ident(&f.ident);
            v.visit_fn(FnKind::Fn(FnCtxt::Foreign, &item.vis, f));
        }
        ForeignItemKind::Static(s) => walk_static_item(v, s),
        ForeignItemKind::TyAlias(t) => walk_ty_alias(v, t),
    }
}

pub fn walk_fn<'a, V: Visitor<'a>>(v: &mut V, kind: FnKind<'a>) {
    match kind {
        FnKind::Fn(_, _, f) => {
            v.visit_generics(&f.generics);
            v.visit_fn_decl(&f.sig.decl);
            if let Some(body) = &f.body {
                v.visit_block(body);
            }
        }
        FnKind::Closure(c) => {
            v.visit_fn_decl(&c.fn_decl);
            v.visit_block(&c.body);
        }
    }
}

pub fn walk_fn_decl<'a, V: Visitor<'a>>(v: &mut V, decl: &'a FnDecl) {
    for p in &decl.inputs {
        v.visit_param(p);
    }
    v.visit_fn_ret_ty(&decl.output);
}

pub fn walk_param<'a, V: Visitor<'a>>(v: &mut V, p: &'a Param) {
    walk_attrs(v, &p.attrs);
    v.visit_pat(&p.pat);
    v.visit_ty(&p.ty);
}

pub fn walk_fn_ret_ty<'a, V: Visitor<'a>>(v: &mut V, ret: &'a FnRetTy) {
    if let FnRetTy::Ty(t) = ret {
        v.visit_ty(t);
    }
}

pub fn walk_generics<'a, V: Visitor<'a>>(v: &mut V, g: &'a Generics) {
    for p in &g.params {
        v.visit_generic_param(p);
    }
    for p in &g.where_clause.predicates {
        v.visit_where_predicate(p);
    }
}

pub fn walk_generic_param<'a, V: Visitor<'a>>(v: &mut V, p: &'a GenericParam) {
    walk_attrs(v, &p.attrs);
    v.visit_ident(&p.ident);
    for b in &p.bounds {
        v.visit_param_bound(b);
    }
    match &p.kind {
        GenericParamKind::Lifetime => {}
        GenericParamKind::Type { default } => {
            if let Some(d) = default {
                v.visit_ty(d);
            }
        }
        GenericParamKind::Const { ty, default, .. } => {
            v.visit_ty(ty);
            if let Some(d) = default {
                v.visit_anon_const(d);
            }
        }
    }
}

pub fn walk_where_predicate<'a, V: Visitor<'a>>(v: &mut V, p: &'a WherePredicate) {
    match &p.kind {
        WherePredicateKind::BoundPredicate { bound_generic_params, bounded_ty, bounds } => {
            for gp in bound_generic_params {
                v.visit_generic_param(gp);
            }
            v.visit_ty(bounded_ty);
            for b in bounds {
                v.visit_param_bound(b);
            }
        }
    }
}

pub fn walk_param_bound<'a, V: Visitor<'a>>(v: &mut V, b: &'a GenericBound) {
    match b {
        GenericBound::Trait(t) => v.visit_poly_trait_ref(t),
        GenericBound::Outlives(o) => v.visit_lifetime_ref(o),
        GenericBound::Use(..) => {}
    }
}

pub fn walk_poly_trait_ref<'a, V: Visitor<'a>>(v: &mut V, t: &'a PolyTraitRef) {
    for gp in &t.bound_generic_params {
        v.visit_generic_param(gp);
    }
    v.visit_trait_ref(&t.trait_ref);
}

pub fn walk_trait_ref<'a, V: Visitor<'a>>(v: &mut V, t: &'a TraitRef) {
    v.visit_path(&t.path, t.ref_id);
}

pub fn walk_variant_data<'a, V: Visitor<'a>>(v: &mut V, data: &'a VariantData) {
    for f in data.fields() {
        v.visit_field_def(f);
    }
}

pub fn walk_variant<'a, V: Visitor<'a>>(v: &mut V, var: &'a Variant) {
    walk_attrs(v, &var.attrs);
    v.visit_vis(&var.vis);
    v.visit_ident(&var.ident);
    v.visit_variant_data(&var.data);
    if let Some(d) = &var.disr_expr {
        v.visit_anon_const(d);
    }
}

pub fn walk_field_def<'a, V: Visitor<'a>>(v: &mut V, f: &'a FieldDef) {
    walk_attrs(v, &f.attrs);
    v.visit_vis(&f.vis);
    if let Some(i) = &f.ident {
        v.visit_ident(i);
    }
    v.visit_ty(&f.ty);
}

pub fn walk_ty<'a, V: Visitor<'a>>(v: &mut V, ty: &'a Ty) {
    match &ty.kind {
        TyKind::Slice(t) | TyKind::Paren(t) => v.visit_ty(t),
        TyKind::Array(t, len) => {
            v.visit_ty(t);
            v.visit_anon_const(len);
        }
        TyKind::Ptr(mt) => v.visit_ty(&mt.ty),
        TyKind::Ref(lifetime, mt) => {
            if let Some(o) = lifetime {
                v.visit_lifetime_ref(o);
            }
            v.visit_ty(&mt.ty);
        }
        TyKind::FnPtr(f) => {
            for gp in &f.generic_params {
                v.visit_generic_param(gp);
            }
            v.visit_fn_decl(&f.decl);
        }
        TyKind::FuncValue(f) => {
            for gp in &f.generic_params {
                v.visit_generic_param(gp);
            }
            v.visit_fn_decl(&f.decl);
        }
        TyKind::Tup(ts) => {
            for t in ts {
                v.visit_ty(t);
            }
        }
        TyKind::Path(qself, path) => {
            v.visit_qself(qself);
            v.visit_path(path, ty.id);
        }
        TyKind::TraitObject(bounds) | TyKind::ImplTrait(_, bounds) => {
            for b in bounds {
                v.visit_param_bound(b);
            }
        }
        TyKind::Never | TyKind::Infer | TyKind::ImplicitSelf | TyKind::CVarArgs | TyKind::Err => {}
    }
}

pub fn walk_path<'a, V: Visitor<'a>>(v: &mut V, path: &'a Path) {
    for seg in &path.segments {
        v.visit_path_segment(seg);
    }
}

pub fn walk_path_segment<'a, V: Visitor<'a>>(v: &mut V, seg: &'a PathSegment) {
    v.visit_ident(&seg.ident);
    if let Some(args) = &seg.args {
        v.visit_generic_args(args);
    }
}

pub fn walk_generic_args<'a, V: Visitor<'a>>(v: &mut V, args: &'a GenericArgs) {
    match args {
        GenericArgs::AngleBracketed(a) => {
            for arg in &a.args {
                match arg {
                    AngleBracketedArg::Arg(g) => v.visit_generic_arg(g),
                    AngleBracketedArg::Constraint(c) => v.visit_assoc_item_constraint(c),
                }
            }
        }
        GenericArgs::Parenthesized(p) => {
            for t in &p.inputs {
                v.visit_ty(t);
            }
            v.visit_fn_ret_ty(&p.output);
        }
    }
}

pub fn walk_generic_arg<'a, V: Visitor<'a>>(v: &mut V, arg: &'a GenericArg) {
    match arg {
        GenericArg::Lifetime(o) => v.visit_lifetime_ref(o),
        GenericArg::Type(t) => v.visit_ty(t),
        GenericArg::Const(c) => v.visit_anon_const(c),
    }
}

pub fn walk_assoc_item_constraint<'a, V: Visitor<'a>>(v: &mut V, c: &'a AssocItemConstraint) {
    v.visit_ident(&c.ident);
    if let Some(args) = &c.gen_args {
        v.visit_generic_args(args);
    }
    match &c.kind {
        AssocItemConstraintKind::Equality { term } => v.visit_ty(term),
        AssocItemConstraintKind::Bound { bounds } => {
            for b in bounds {
                v.visit_param_bound(b);
            }
        }
    }
}

pub fn walk_pat<'a, V: Visitor<'a>>(v: &mut V, pat: &'a Pat) {
    match &pat.kind {
        PatKind::Ident(_, ident, sub) => {
            v.visit_ident(ident);
            if let Some(s) = sub {
                v.visit_pat(s);
            }
        }
        PatKind::Struct(qself, path, fields, _) => {
            v.visit_qself(qself);
            v.visit_path(path, pat.id);
            for f in fields {
                v.visit_pat_field(f);
            }
        }
        PatKind::TupleStruct(qself, path, pats) => {
            v.visit_qself(qself);
            v.visit_path(path, pat.id);
            for p in pats {
                v.visit_pat(p);
            }
        }
        PatKind::Path(qself, path) => {
            v.visit_qself(qself);
            v.visit_path(path, pat.id);
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

pub fn walk_pat_field<'a, V: Visitor<'a>>(v: &mut V, f: &'a PatField) {
    walk_attrs(v, &f.attrs);
    v.visit_ident(&f.ident);
    v.visit_pat(&f.pat);
}

pub fn walk_expr<'a, V: Visitor<'a>>(v: &mut V, e: &'a Expr) {
    walk_attrs(v, &e.attrs);
    match &e.kind {
        ExprKind::Array(es) | ExprKind::Tup(es) => {
            for x in es {
                v.visit_expr(x);
            }
        }
        ExprKind::Repeat(x, n) => {
            v.visit_expr(x);
            v.visit_anon_const(n);
        }
        ExprKind::Vec(x) | ExprKind::Paren(x) | ExprKind::Try(x) | ExprKind::Unary(_, x) => v.visit_expr(x),
        ExprKind::ConstBlock(c) => v.visit_anon_const(c),
        ExprKind::Call(f, args) => {
            v.visit_expr(f);
            for a in args {
                v.visit_expr(a);
            }
        }
        ExprKind::MethodCall(m) => {
            v.visit_expr(&m.receiver);
            v.visit_path_segment(&m.seg);
            for a in &m.args {
                v.visit_expr(a);
            }
        }
        ExprKind::Binary(_, l, r) | ExprKind::Assign(l, r, _) | ExprKind::AssignOp(_, l, r) => {
            v.visit_expr(l);
            v.visit_expr(r);
        }
        ExprKind::Lit(_) | ExprKind::Underscore | ExprKind::Err => {}
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
            if let Some(e) = els {
                v.visit_expr(e);
            }
        }
        ExprKind::While(c, body, label) => {
            if let Some(l) = label {
                v.visit_label(l);
            }
            v.visit_expr(c);
            v.visit_block(body);
        }
        ExprKind::ForLoop { pat, iter, body, label } => {
            if let Some(l) = label {
                v.visit_label(l);
            }
            v.visit_pat(pat);
            v.visit_expr(iter);
            v.visit_block(body);
        }
        ExprKind::Loop(body, label, _) => {
            if let Some(l) = label {
                v.visit_label(l);
            }
            v.visit_block(body);
        }
        ExprKind::Match(x, arms) => {
            v.visit_expr(x);
            for a in arms {
                v.visit_arm(a);
            }
        }
        ExprKind::Closure(c) => v.visit_fn(FnKind::Closure(c)),
        ExprKind::Block(b, label) => {
            if let Some(l) = label {
                v.visit_label(l);
            }
            v.visit_block(b);
        }
        ExprKind::Async(b) => v.visit_block(b),
        ExprKind::Await(x, _) => v.visit_expr(x),
        ExprKind::Field(x, ident) => {
            v.visit_expr(x);
            v.visit_ident(ident);
        }
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
            v.visit_qself(qself);
            v.visit_path(path, e.id);
        }
        ExprKind::AddrOf(_, _, x) => v.visit_expr(x),
        ExprKind::Break(label, x) => {
            if let Some(l) = label {
                v.visit_label(l);
            }
            if let Some(x) = x {
                v.visit_expr(x);
            }
        }
        ExprKind::Continue(label) => {
            if let Some(l) = label {
                v.visit_label(l);
            }
        }
        ExprKind::Ret(x) => {
            if let Some(x) = x {
                v.visit_expr(x);
            }
        }
        ExprKind::InlineAsm(asm) => v.visit_inline_asm(asm),
        ExprKind::Builtin(b) => walk_builtin(v, b),
        ExprKind::Struct(s) => {
            v.visit_qself(&s.qself);
            v.visit_path(&s.path, e.id);
            for f in &s.fields {
                v.visit_expr_field(f);
            }
            if let StructRest::Base(b) = &s.rest {
                v.visit_expr(b);
            }
        }
    }
}

pub fn walk_expr_field<'a, V: Visitor<'a>>(v: &mut V, f: &'a ExprField) {
    walk_attrs(v, &f.attrs);
    v.visit_ident(&f.ident);
    v.visit_expr(&f.expr);
}

pub fn walk_arm<'a, V: Visitor<'a>>(v: &mut V, arm: &'a Arm) {
    walk_attrs(v, &arm.attrs);
    v.visit_pat(&arm.pat);
    if let Some(g) = &arm.guard {
        v.visit_expr(g);
    }
    v.visit_expr(&arm.body);
}

pub fn walk_block<'a, V: Visitor<'a>>(v: &mut V, b: &'a Block) {
    for s in &b.stmts {
        v.visit_stmt(s);
    }
}

pub fn walk_stmt<'a, V: Visitor<'a>>(v: &mut V, s: &'a Stmt) {
    match &s.kind {
        StmtKind::Let(l) => v.visit_local(l),
        StmtKind::Item(i) => v.visit_item(i),
        StmtKind::Expr(e) => v.visit_expr(e),
    }
}

pub fn walk_local<'a, V: Visitor<'a>>(v: &mut V, l: &'a Local) {
    walk_attrs(v, &l.attrs);
    v.visit_pat(&l.pat);
    if let Some(t) = &l.ty {
        v.visit_ty(t);
    }
    match &l.kind {
        LocalKind::Decl => {}
        LocalKind::Init(e) => v.visit_expr(e),
        LocalKind::InitElse(e, b) => {
            v.visit_expr(e);
            v.visit_block(b);
        }
    }
}

pub fn walk_inline_asm<'a, V: Visitor<'a>>(v: &mut V, asm: &'a InlineAsm) {
    for (op, _) in &asm.operands {
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
            InlineAsmOperand::Const { anon_const, .. } => v.visit_anon_const(anon_const),
            InlineAsmOperand::Sym { qself, path, id, .. } => {
                v.visit_qself(qself);
                v.visit_path(path, *id);
            }
            InlineAsmOperand::Label { block, .. } => v.visit_block(block),
        }
    }
}

pub fn walk_builtin<'a, V: Visitor<'a>>(v: &mut V, b: &'a BuiltinCall) {
    let fmt = |v: &mut V, f: &'a Option<P<FormatArgs>>| {
        if let Some(f) = f {
            walk_format_args(v, f);
        }
    };
    match &b.kind {
        BuiltinKind::Print { args, .. } | BuiltinKind::Format(args) => walk_format_args(v, args),
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

pub fn walk_format_args<'a, V: Visitor<'a>>(v: &mut V, f: &'a FormatArgs) {
    for arg in &f.arguments {
        v.visit_expr(&arg.expr);
    }
}
