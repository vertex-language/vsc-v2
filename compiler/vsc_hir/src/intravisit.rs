//! Walking HIR (rustc's `rustc_hir::intravisit`).
//!
//! Each `visit_*` method defaults to the matching `walk_*` function, which
//! visits the node's children; override the ones a pass cares about and call
//! `walk_*` to keep going. Items, associated items, foreign items and bodies
//! are *nested*: they're reached through ids, and visited only when
//! `Visitor::krate` returns the crate.

use vsc_span::def_id::LocalDefId;
use vsc_span::{Ident, Span};

use crate::hir::*;
use crate::hir_id::HirId;

pub trait Visitor<'hir>: Sized {
    /// The crate, to visit nested items and bodies. `None` (the default)
    /// stays within one owner and skips bodies.
    fn krate(&self) -> Option<&'hir Crate> {
        None
    }

    fn visit_nested_item(&mut self, id: ItemId) {
        if let Some(k) = self.krate() {
            self.visit_item(k.item(id));
        }
    }

    fn visit_nested_assoc_item(&mut self, def: LocalDefId) {
        if let Some(k) = self.krate() {
            self.visit_assoc_item(k.assoc_item(def));
        }
    }

    fn visit_nested_foreign_item(&mut self, def: LocalDefId) {
        if let Some(k) = self.krate() {
            self.visit_foreign_item(k.foreign_item(def));
        }
    }

    fn visit_nested_body(&mut self, id: BodyId) {
        if let Some(k) = self.krate() {
            self.visit_body(k.body(id));
        }
    }

    /// Every node's own id.
    fn visit_id(&mut self, _id: HirId) {}
    fn visit_ident(&mut self, _ident: Ident) {}

    fn visit_mod(&mut self, m: &'hir Mod) {
        walk_mod(self, m)
    }
    fn visit_item(&mut self, i: &'hir Item) {
        walk_item(self, i)
    }
    fn visit_assoc_item(&mut self, i: &'hir AssocItem) {
        walk_assoc_item(self, i)
    }
    fn visit_foreign_item(&mut self, i: &'hir ForeignItem) {
        walk_foreign_item(self, i)
    }
    fn visit_body(&mut self, b: &'hir Body) {
        walk_body(self, b)
    }
    fn visit_param(&mut self, p: &'hir Param) {
        walk_param(self, p)
    }
    fn visit_variant(&mut self, v: &'hir Variant) {
        walk_variant(self, v)
    }
    fn visit_variant_data(&mut self, d: &'hir VariantData) {
        walk_variant_data(self, d)
    }
    fn visit_field_def(&mut self, f: &'hir FieldDef) {
        walk_field_def(self, f)
    }
    fn visit_anon_const(&mut self, c: &'hir AnonConst) {
        walk_anon_const(self, c)
    }
    fn visit_generics(&mut self, g: &'hir Generics) {
        walk_generics(self, g)
    }
    fn visit_generic_param(&mut self, p: &'hir GenericParam) {
        walk_generic_param(self, p)
    }
    fn visit_where_predicate(&mut self, p: &'hir WherePredicate) {
        walk_where_predicate(self, p)
    }
    fn visit_param_bound(&mut self, b: &'hir GenericBound) {
        walk_param_bound(self, b)
    }
    fn visit_poly_trait_ref(&mut self, t: &'hir PolyTraitRef) {
        walk_poly_trait_ref(self, t)
    }
    fn visit_trait_ref(&mut self, t: &'hir TraitRef) {
        walk_trait_ref(self, t)
    }
    fn visit_lifetime(&mut self, o: &'hir Lifetime) {
        self.visit_id(o.hir_id)
    }
    fn visit_fn_decl(&mut self, d: &'hir FnDecl) {
        walk_fn_decl(self, d)
    }
    fn visit_ty(&mut self, t: &'hir Ty) {
        walk_ty(self, t)
    }
    fn visit_qpath(&mut self, q: &'hir QPath, id: HirId, span: Span) {
        walk_qpath(self, q, id, span)
    }
    fn visit_path(&mut self, p: &'hir Path, _id: HirId) {
        walk_path(self, p)
    }
    fn visit_path_segment(&mut self, s: &'hir PathSegment) {
        walk_path_segment(self, s)
    }
    fn visit_generic_args(&mut self, a: &'hir GenericArgs) {
        walk_generic_args(self, a)
    }
    fn visit_generic_arg(&mut self, a: &'hir GenericArg) {
        walk_generic_arg(self, a)
    }
    fn visit_assoc_item_constraint(&mut self, c: &'hir AssocItemConstraint) {
        walk_assoc_item_constraint(self, c)
    }
    fn visit_pat(&mut self, p: &'hir Pat) {
        walk_pat(self, p)
    }
    fn visit_pat_field(&mut self, f: &'hir PatField) {
        walk_pat_field(self, f)
    }
    fn visit_expr(&mut self, e: &'hir Expr) {
        walk_expr(self, e)
    }
    fn visit_expr_field(&mut self, f: &'hir ExprField) {
        walk_expr_field(self, f)
    }
    fn visit_let_expr(&mut self, l: &'hir LetExpr) {
        walk_let_expr(self, l)
    }
    fn visit_arm(&mut self, a: &'hir Arm) {
        walk_arm(self, a)
    }
    fn visit_block(&mut self, b: &'hir Block) {
        walk_block(self, b)
    }
    fn visit_stmt(&mut self, s: &'hir Stmt) {
        walk_stmt(self, s)
    }
    fn visit_local(&mut self, l: &'hir LetStmt) {
        walk_local(self, l)
    }
    fn visit_inline_asm(&mut self, asm: &'hir InlineAsm, _id: HirId) {
        walk_inline_asm(self, asm)
    }
}

pub fn walk_mod<'v, V: Visitor<'v>>(v: &mut V, m: &'v Mod) {
    for &id in &m.item_ids {
        v.visit_nested_item(id);
    }
}

pub fn walk_item<'v, V: Visitor<'v>>(v: &mut V, i: &'v Item) {
    v.visit_id(i.hir_id());
    v.visit_ident(i.ident);
    match &i.kind {
        ItemKind::Mod(m) => v.visit_mod(m),
        ItemKind::Fn { sig, generics, body } => {
            v.visit_generics(generics);
            v.visit_fn_decl(&sig.decl);
            v.visit_nested_body(*body);
        }
        ItemKind::Static(ty, _, body) => {
            v.visit_ty(ty);
            v.visit_nested_body(*body);
        }
        ItemKind::Const(ty, generics, body) => {
            v.visit_generics(generics);
            v.visit_ty(ty);
            v.visit_nested_body(*body);
        }
        ItemKind::TyAlias(ty, generics) => {
            v.visit_generics(generics);
            v.visit_ty(ty);
        }
        ItemKind::Enum(def, generics) => {
            v.visit_generics(generics);
            for variant in &def.variants {
                v.visit_variant(variant);
            }
        }
        ItemKind::Struct(data, generics) | ItemKind::Class(data, generics) | ItemKind::Union(data, generics) => {
            v.visit_generics(generics);
            v.visit_variant_data(data);
        }
        ItemKind::Trait { generics, bounds, items, .. } => {
            v.visit_generics(generics);
            for b in bounds {
                v.visit_param_bound(b);
            }
            for &item in items {
                v.visit_nested_assoc_item(item);
            }
        }
        ItemKind::Impl(imp) => {
            v.visit_generics(&imp.generics);
            if let Some(t) = &imp.of_trait {
                v.visit_trait_ref(t);
            }
            v.visit_ty(&imp.self_ty);
            for &item in &imp.items {
                v.visit_nested_assoc_item(item);
            }
        }
        ItemKind::ForeignMod { items, .. } => {
            for &item in items {
                v.visit_nested_foreign_item(item);
            }
        }
        ItemKind::GlobalAsm(asm) => v.visit_inline_asm(asm, i.hir_id()),
    }
}

pub fn walk_assoc_item<'v, V: Visitor<'v>>(v: &mut V, i: &'v AssocItem) {
    v.visit_id(HirId::make_owner(i.owner_id));
    v.visit_ident(i.ident);
    v.visit_generics(&i.generics);
    match &i.kind {
        AssocItemKind::Const(ty, body) => {
            v.visit_ty(ty);
            if let Some(b) = body {
                v.visit_nested_body(*b);
            }
        }
        AssocItemKind::Fn(sig, body) => {
            v.visit_fn_decl(&sig.decl);
            if let Some(b) = body {
                v.visit_nested_body(*b);
            }
        }
        AssocItemKind::Type(bounds, ty) => {
            for b in bounds {
                v.visit_param_bound(b);
            }
            if let Some(t) = ty {
                v.visit_ty(t);
            }
        }
    }
}

pub fn walk_foreign_item<'v, V: Visitor<'v>>(v: &mut V, i: &'v ForeignItem) {
    v.visit_id(HirId::make_owner(i.owner_id));
    v.visit_ident(i.ident);
    match &i.kind {
        ForeignItemKind::Fn(sig, names, generics) => {
            v.visit_generics(generics);
            v.visit_fn_decl(&sig.decl);
            for &n in names {
                v.visit_ident(n);
            }
        }
        ForeignItemKind::Static(ty, ..) => v.visit_ty(ty),
        ForeignItemKind::Type => {}
    }
}

pub fn walk_body<'v, V: Visitor<'v>>(v: &mut V, b: &'v Body) {
    for p in &b.params {
        v.visit_param(p);
    }
    v.visit_expr(&b.value);
}

pub fn walk_param<'v, V: Visitor<'v>>(v: &mut V, p: &'v Param) {
    v.visit_id(p.hir_id);
    v.visit_pat(&p.pat);
}

pub fn walk_variant<'v, V: Visitor<'v>>(v: &mut V, variant: &'v Variant) {
    v.visit_id(variant.hir_id);
    v.visit_ident(variant.ident);
    v.visit_variant_data(&variant.data);
    if let Some(d) = &variant.disr_expr {
        v.visit_anon_const(d);
    }
}

pub fn walk_variant_data<'v, V: Visitor<'v>>(v: &mut V, d: &'v VariantData) {
    match d {
        VariantData::Struct { .. } => {}
        VariantData::Tuple(_, id, _) | VariantData::Unit(id, _) => v.visit_id(*id),
    }
    for f in d.fields() {
        v.visit_field_def(f);
    }
}

pub fn walk_field_def<'v, V: Visitor<'v>>(v: &mut V, f: &'v FieldDef) {
    v.visit_id(f.hir_id);
    v.visit_ident(f.ident);
    v.visit_ty(&f.ty);
}

pub fn walk_anon_const<'v, V: Visitor<'v>>(v: &mut V, c: &'v AnonConst) {
    v.visit_id(c.hir_id);
    v.visit_nested_body(c.body);
}

pub fn walk_generics<'v, V: Visitor<'v>>(v: &mut V, g: &'v Generics) {
    for p in &g.params {
        v.visit_generic_param(p);
    }
    for p in &g.predicates {
        v.visit_where_predicate(p);
    }
}

pub fn walk_generic_param<'v, V: Visitor<'v>>(v: &mut V, p: &'v GenericParam) {
    v.visit_id(p.hir_id);
    v.visit_ident(p.name);
    match &p.kind {
        GenericParamKind::Lifetime => {}
        GenericParamKind::Type { default } => {
            if let Some(t) = default {
                v.visit_ty(t);
            }
        }
        GenericParamKind::Const { ty, default } => {
            v.visit_ty(ty);
            if let Some(d) = default {
                v.visit_anon_const(d);
            }
        }
    }
}

pub fn walk_where_predicate<'v, V: Visitor<'v>>(v: &mut V, p: &'v WherePredicate) {
    match p {
        WherePredicate::Bound { hir_id, bound_generic_params, bounded_ty, bounds, .. } => {
            v.visit_id(*hir_id);
            for gp in bound_generic_params {
                v.visit_generic_param(gp);
            }
            v.visit_ty(bounded_ty);
            for b in bounds {
                v.visit_param_bound(b);
            }
        }
        WherePredicate::Lifetime { lifetime, bounds, .. } => {
            v.visit_lifetime(lifetime);
            for b in bounds {
                v.visit_lifetime(b);
            }
        }
    }
}

pub fn walk_param_bound<'v, V: Visitor<'v>>(v: &mut V, b: &'v GenericBound) {
    match b {
        GenericBound::Trait(t) => v.visit_poly_trait_ref(t),
        GenericBound::Outlives(o) => v.visit_lifetime(o),
        GenericBound::Use(args, _) => {
            for &a in args {
                v.visit_ident(a);
            }
        }
    }
}

pub fn walk_poly_trait_ref<'v, V: Visitor<'v>>(v: &mut V, t: &'v PolyTraitRef) {
    for gp in &t.bound_generic_params {
        v.visit_generic_param(gp);
    }
    v.visit_trait_ref(&t.trait_ref);
}

pub fn walk_trait_ref<'v, V: Visitor<'v>>(v: &mut V, t: &'v TraitRef) {
    v.visit_id(t.hir_ref_id);
    v.visit_path(&t.path, t.hir_ref_id);
}

pub fn walk_fn_decl<'v, V: Visitor<'v>>(v: &mut V, d: &'v FnDecl) {
    for t in &d.inputs {
        v.visit_ty(t);
    }
    if let FnRetTy::Return(t) = &d.output {
        v.visit_ty(t);
    }
}

pub fn walk_ty<'v, V: Visitor<'v>>(v: &mut V, t: &'v Ty) {
    v.visit_id(t.hir_id);
    match &t.kind {
        TyKind::Slice(inner) => v.visit_ty(inner),
        TyKind::Array(inner, len) => {
            v.visit_ty(inner);
            v.visit_anon_const(len);
        }
        TyKind::Ptr(mt) => v.visit_ty(&mt.ty),
        TyKind::Ref(lifetime, mt) => {
            v.visit_lifetime(lifetime);
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
        TyKind::Tup(tys) => {
            for t in tys {
                v.visit_ty(t);
            }
        }
        TyKind::Path(q) => v.visit_qpath(q, t.hir_id, t.span),
        TyKind::TraitObject(traits, lifetime) => {
            for p in traits {
                v.visit_poly_trait_ref(p);
            }
            if let Some(o) = lifetime {
                v.visit_lifetime(o);
            }
        }
        TyKind::OpaqueDef(bounds) => {
            for b in bounds {
                v.visit_param_bound(b);
            }
        }
        TyKind::Never | TyKind::Infer | TyKind::Err => {}
    }
}

pub fn walk_qpath<'v, V: Visitor<'v>>(v: &mut V, q: &'v QPath, id: HirId, _span: Span) {
    match q {
        QPath::Resolved(qself, path) => {
            if let Some(t) = qself {
                v.visit_ty(t);
            }
            v.visit_path(path, id);
        }
        QPath::TypeRelative(ty, seg) => {
            v.visit_ty(ty);
            v.visit_path_segment(seg);
        }
        QPath::LangItem(..) => {}
    }
}

pub fn walk_path<'v, V: Visitor<'v>>(v: &mut V, p: &'v Path) {
    for s in &p.segments {
        v.visit_path_segment(s);
    }
}

pub fn walk_path_segment<'v, V: Visitor<'v>>(v: &mut V, s: &'v PathSegment) {
    v.visit_id(s.hir_id);
    v.visit_ident(s.ident);
    if let Some(a) = &s.args {
        v.visit_generic_args(a);
    }
}

pub fn walk_generic_args<'v, V: Visitor<'v>>(v: &mut V, a: &'v GenericArgs) {
    for arg in &a.args {
        v.visit_generic_arg(arg);
    }
    for c in &a.constraints {
        v.visit_assoc_item_constraint(c);
    }
}

pub fn walk_generic_arg<'v, V: Visitor<'v>>(v: &mut V, a: &'v GenericArg) {
    match a {
        GenericArg::Lifetime(o) => v.visit_lifetime(o),
        GenericArg::Type(t) => v.visit_ty(t),
        GenericArg::Const(c) => {
            v.visit_id(c.hir_id);
            match &c.kind {
                ConstArgKind::Path(q) => v.visit_qpath(q, c.hir_id, c.span),
                ConstArgKind::Anon(a) => v.visit_anon_const(a),
            }
        }
        GenericArg::Infer(_) => {}
    }
}

pub fn walk_assoc_item_constraint<'v, V: Visitor<'v>>(v: &mut V, c: &'v AssocItemConstraint) {
    v.visit_id(c.hir_id);
    v.visit_ident(c.ident);
    match &c.kind {
        AssocItemConstraintKind::Equality(t) => v.visit_ty(t),
        AssocItemConstraintKind::Bound(bounds) => {
            for b in bounds {
                v.visit_param_bound(b);
            }
        }
    }
}

pub fn walk_pat<'v, V: Visitor<'v>>(v: &mut V, p: &'v Pat) {
    v.visit_id(p.hir_id);
    match &p.kind {
        PatKind::Wild | PatKind::Err => {}
        PatKind::Binding(_, _, ident, sub) => {
            v.visit_ident(*ident);
            if let Some(s) = sub {
                v.visit_pat(s);
            }
        }
        PatKind::Struct(q, fields, _) => {
            v.visit_qpath(q, p.hir_id, p.span);
            for f in fields {
                v.visit_pat_field(f);
            }
        }
        PatKind::TupleStruct(q, pats, _) => {
            v.visit_qpath(q, p.hir_id, p.span);
            for p in pats {
                v.visit_pat(p);
            }
        }
        PatKind::Or(pats) | PatKind::Tuple(pats, _) => {
            for p in pats {
                v.visit_pat(p);
            }
        }
        PatKind::Path(q) => v.visit_qpath(q, p.hir_id, p.span),
        PatKind::Ref(inner, _) => v.visit_pat(inner),
        PatKind::Expr(e) => v.visit_expr(e),
        PatKind::Range(lo, hi, _) => {
            if let Some(e) = lo {
                v.visit_expr(e);
            }
            if let Some(e) = hi {
                v.visit_expr(e);
            }
        }
        PatKind::Slice(before, slice, after) => {
            for p in before {
                v.visit_pat(p);
            }
            if let Some(s) = slice {
                v.visit_pat(s);
            }
            for p in after {
                v.visit_pat(p);
            }
        }
    }
}

pub fn walk_pat_field<'v, V: Visitor<'v>>(v: &mut V, f: &'v PatField) {
    v.visit_id(f.hir_id);
    v.visit_ident(f.ident);
    v.visit_pat(&f.pat);
}

pub fn walk_expr<'v, V: Visitor<'v>>(v: &mut V, e: &'v Expr) {
    v.visit_id(e.hir_id);
    match &e.kind {
        ExprKind::ConstBlock(c) => {
            v.visit_id(c.hir_id);
            v.visit_nested_body(c.body);
        }
        ExprKind::Array(es) | ExprKind::Tup(es) => {
            for e in es {
                v.visit_expr(e);
            }
        }
        ExprKind::Call(f, args) => {
            v.visit_expr(f);
            for a in args {
                v.visit_expr(a);
            }
        }
        ExprKind::MethodCall(seg, recv, args, _) => {
            v.visit_path_segment(seg);
            v.visit_expr(recv);
            for a in args {
                v.visit_expr(a);
            }
        }
        ExprKind::Binary(_, a, b) | ExprKind::Assign(a, b, _) | ExprKind::AssignOp(_, a, b) | ExprKind::Index(a, b, _) => {
            v.visit_expr(a);
            v.visit_expr(b);
        }
        ExprKind::Unary(_, x)
        | ExprKind::DropTemps(x)
        | ExprKind::AddrOf(_, _, x)
        | ExprKind::Await(x)
        | ExprKind::Field(x, _) => v.visit_expr(x),
        ExprKind::Lit(_) | ExprKind::Err => {}
        ExprKind::Cast(x, t) => {
            v.visit_expr(x);
            v.visit_ty(t);
        }
        ExprKind::Let(l) => v.visit_let_expr(l),
        ExprKind::If(c, t, e) => {
            v.visit_expr(c);
            v.visit_expr(t);
            if let Some(e) = e {
                v.visit_expr(e);
            }
        }
        ExprKind::Loop(b, ..) | ExprKind::Block(b, _) => v.visit_block(b),
        ExprKind::Match(x, arms, _) => {
            v.visit_expr(x);
            for a in arms {
                v.visit_arm(a);
            }
        }
        ExprKind::Closure(c) => {
            v.visit_fn_decl(&c.fn_decl);
            v.visit_nested_body(c.body);
        }
        ExprKind::Path(q) => v.visit_qpath(q, e.hir_id, e.span),
        ExprKind::Break(_, value) | ExprKind::Ret(value) => {
            if let Some(x) = value {
                v.visit_expr(x);
            }
        }
        ExprKind::Continue(_) => {}
        ExprKind::InlineAsm(asm) => v.visit_inline_asm(asm, e.hir_id),
        ExprKind::Struct(q, fields, tail) => {
            v.visit_qpath(q, e.hir_id, e.span);
            for f in fields {
                v.visit_expr_field(f);
            }
            if let StructTailExpr::Base(b) = tail {
                v.visit_expr(b);
            }
        }
        ExprKind::Repeat(x, n) => {
            v.visit_expr(x);
            v.visit_anon_const(n);
        }
    }
}

pub fn walk_expr_field<'v, V: Visitor<'v>>(v: &mut V, f: &'v ExprField) {
    v.visit_id(f.hir_id);
    v.visit_ident(f.ident);
    v.visit_expr(&f.expr);
}

pub fn walk_let_expr<'v, V: Visitor<'v>>(v: &mut V, l: &'v LetExpr) {
    v.visit_expr(&l.init);
    v.visit_pat(&l.pat);
    if let Some(t) = &l.ty {
        v.visit_ty(t);
    }
}

pub fn walk_arm<'v, V: Visitor<'v>>(v: &mut V, a: &'v Arm) {
    v.visit_id(a.hir_id);
    v.visit_pat(&a.pat);
    if let Some(g) = &a.guard {
        v.visit_expr(g);
    }
    v.visit_expr(&a.body);
}

pub fn walk_block<'v, V: Visitor<'v>>(v: &mut V, b: &'v Block) {
    v.visit_id(b.hir_id);
    for s in &b.stmts {
        v.visit_stmt(s);
    }
    if let Some(e) = &b.expr {
        v.visit_expr(e);
    }
}

pub fn walk_stmt<'v, V: Visitor<'v>>(v: &mut V, s: &'v Stmt) {
    v.visit_id(s.hir_id);
    match &s.kind {
        StmtKind::Let(l) => v.visit_local(l),
        StmtKind::Item(id) => v.visit_nested_item(*id),
        StmtKind::Semi(e) => v.visit_expr(e),
    }
}

pub fn walk_local<'v, V: Visitor<'v>>(v: &mut V, l: &'v LetStmt) {
    v.visit_id(l.hir_id);
    if let Some(e) = &l.init {
        v.visit_expr(e);
    }
    v.visit_pat(&l.pat);
    if let Some(t) = &l.ty {
        v.visit_ty(t);
    }
    if let Some(b) = &l.els {
        v.visit_block(b);
    }
}

pub fn walk_inline_asm<'v, V: Visitor<'v>>(v: &mut V, asm: &'v InlineAsm) {
    for (op, _) in &asm.operands {
        match op {
            InlineAsmOperand::In { expr, .. } | InlineAsmOperand::InOut { expr, .. } | InlineAsmOperand::SymFn { expr } => {
                v.visit_expr(expr)
            }
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
            InlineAsmOperand::Const { anon_const } => v.visit_anon_const(anon_const),
            InlineAsmOperand::Label { block } => v.visit_block(block),
        }
    }
}
