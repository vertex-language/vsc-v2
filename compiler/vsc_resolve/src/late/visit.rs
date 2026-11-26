//! AST `Visitor` implementation for late name resolution.

use vsc_ast::visit::{self, AssocCtxt, FnKind, Visitor};
use vsc_ast::*;
use vsc_hir::def::{DefKind, Namespace, PartialRes, Res};
use vsc_span::kw;

use super::{LateResolutionVisitor, PathSource, Rib, RibKind};

impl<'ast> Visitor<'ast> for LateResolutionVisitor<'_, '_> {
    fn visit_item(&mut self, item: &'ast Item) {
        self.with_item_ribs(|this| match &item.kind {
            ItemKind::Fn(f) => {
                this.with_generics(&f.generics, &[], |this| this.resolve_fn_decl_and_body(&f.sig.decl, f.body.as_deref()))
            }
            ItemKind::Struct(_, g, data) | ItemKind::Class(_, g, data) | ItemKind::Union(_, g, data) => {
                this.with_generics(g, &[], |this| this.visit_variant_data(data))
            }
            ItemKind::Enum(_, g, def) => this.with_generics(g, &[], |this| {
                for v in &def.variants {
                    this.visit_variant(v);
                }
            }),
            ItemKind::TyAlias(t) => this.with_generics(&t.generics, &[], |this| {
                for b in &t.bounds {
                    this.visit_param_bound(b);
                }
                if let Some(ty) = &t.ty {
                    this.visit_ty(ty);
                }
            }),
            ItemKind::Const(c) => {
                this.visit_ty(&c.ty);
                if let Some(e) = &c.expr {
                    this.with_rib(Namespace::ValueNS, RibKind::Constant, |this| this.visit_expr(e));
                }
            }
            ItemKind::Static(s) => {
                this.visit_ty(&s.ty);
                if let Some(e) = &s.expr {
                    this.with_rib(Namespace::ValueNS, RibKind::Constant, |this| this.visit_expr(e));
                }
            }
            ItemKind::Trait(t) => {
                let def = this.r.local_def_id(item.id).to_def_id();
                // `Self` is in scope in the generics too: `trait Add<Rhs = Self>`.
                this.with_rib(Namespace::TypeNS, RibKind::Normal, |this| {
                    this.bind(Namespace::TypeNS, kw::SelfUpper, Res::SelfTyParam { trait_: def });
                    this.with_generics(&t.generics, &[], |this| {
                        for b in &t.bounds {
                            this.visit_param_bound(b);
                        }
                        for ai in &t.items {
                            this.visit_assoc_item(ai, AssocCtxt::Trait);
                        }
                    })
                })
            }
            ItemKind::Impl(i) => {
                let def = this.r.local_def_id(item.id).to_def_id();
                let extra = this.settle_flat_receiver_params(item, i);
                this.with_generics(&i.generics, &extra, |this| {
                    if let Some(t) = &i.of_trait {
                        this.visit_trait_ref(t);
                    }
                    this.visit_ty(&i.self_ty);
                    this.with_rib(Namespace::TypeNS, RibKind::Normal, |this| {
                        this.with_rib(Namespace::ValueNS, RibKind::Normal, |this| {
                            this.bind(Namespace::TypeNS, kw::SelfUpper, Res::SelfTyAlias { alias_to: def });
                            this.bind(Namespace::ValueNS, kw::SelfUpper, Res::SelfCtor(def));
                            for ai in &i.items {
                                this.visit_assoc_item(ai, AssocCtxt::Impl { of_trait: i.of_trait.is_some() });
                            }
                        })
                    })
                })
            }
            ItemKind::ForeignMod(m) => {
                for fi in &m.items {
                    this.visit_foreign_item(fi);
                }
            }
            ItemKind::GlobalAsm(asm) => this.visit_inline_asm(asm),
        });
    }

    fn visit_assoc_item(&mut self, item: &'ast AssocItem, _ctxt: AssocCtxt) {
        for a in &item.attrs {
            self.visit_attribute(a);
        }
        match &item.kind {
            AssocItemKind::Fn(f) => {
                self.with_generics(&f.generics, &[], |this| this.resolve_fn_decl_and_body(&f.sig.decl, f.body.as_deref()))
            }
            AssocItemKind::Const(c) => {
                self.visit_ty(&c.ty);
                if let Some(e) = &c.expr {
                    self.with_rib(Namespace::ValueNS, RibKind::Constant, |this| this.visit_expr(e));
                }
            }
            AssocItemKind::Type(t) => self.with_generics(&t.generics, &[], |this| {
                for b in &t.bounds {
                    this.visit_param_bound(b);
                }
                if let Some(ty) = &t.ty {
                    this.visit_ty(ty);
                }
            }),
        }
    }

    fn visit_foreign_item(&mut self, item: &'ast ForeignItem) {
        self.with_item_ribs(|this| match &item.kind {
            ForeignItemKind::Fn(f) => this.with_generics(&f.generics, &[], |this| this.resolve_fn_decl_and_body(&f.sig.decl, None)),
            ForeignItemKind::Static(s) => this.visit_ty(&s.ty),
            ForeignItemKind::TyAlias(_) => {}
        });
    }

    fn visit_generic_param(&mut self, _p: &'ast GenericParam) {
        // Handled by `with_generics`.
    }

    fn visit_where_predicate(&mut self, pred: &'ast WherePredicate) {
        let WherePredicateKind::BoundPredicate { bound_generic_params, bounded_ty, bounds } = &pred.kind;
        // `a: [b]`: a lifetime outliving another (Rust's `'a: 'b`).
        if let TyKind::Path(None, p) = &bounded_ty.kind {
            if let Some(ident) = p.as_ident() {
                if self.lookup(ident, Namespace::TypeNS, true).is_none() && self.lookup_lifetime(ident).is_some() {
                    self.resolve_lifetime(ident);
                    for b in bounds {
                        self.visit_param_bound(b);
                    }
                    return;
                }
            }
        }
        self.lifetime_ribs.push(Rib::new(RibKind::Normal));
        self.ribs.type_ns.push(Rib::new(RibKind::Normal));
        for gp in bound_generic_params {
            let def = self.r.local_def_id(gp.id);
            self.bind_generic_param(gp.ident.name, def);
        }
        self.visit_ty(bounded_ty);
        for b in bounds {
            self.visit_param_bound(b);
        }
        self.ribs.type_ns.pop();
        self.lifetime_ribs.pop();
    }

    fn visit_poly_trait_ref(&mut self, t: &'ast PolyTraitRef) {
        self.lifetime_ribs.push(Rib::new(RibKind::Normal));
        self.ribs.type_ns.push(Rib::new(RibKind::Normal));
        for gp in &t.bound_generic_params {
            let def = self.r.local_def_id(gp.id);
            self.bind_generic_param(gp.ident.name, def);
        }
        self.visit_trait_ref(&t.trait_ref);
        self.ribs.type_ns.pop();
        self.lifetime_ribs.pop();
    }

    fn visit_trait_ref(&mut self, t: &'ast TraitRef) {
        self.resolve_path(&t.path, t.ref_id, PathSource::Trait);
        self.visit_path_args(&t.path);
    }

    fn visit_lifetime_ref(&mut self, o: &'ast Lifetime) {
        self.resolve_lifetime(o.ident);
    }

    fn visit_generic_arg(&mut self, arg: &'ast GenericArg) {
        if let GenericArg::Type(ty) = arg {
            if self.generic_arg_as_lifetime(ty) {
                return;
            }
            // A name that isn't a type but is a constant or const parameter
            // is a const argument, as in rustc: `N` in `Stack<T, N>`.
            if let TyKind::Path(None, p) = &ty.kind {
                if let Some(ident) = p.as_ident() {
                    if self.lookup(ident, Namespace::TypeNS, true).is_none() {
                        if let Some(res @ Res::Def(DefKind::ConstParam | DefKind::Const | DefKind::AssocConst, _)) =
                            self.lookup(ident, Namespace::ValueNS, true)
                        {
                            self.record(ty.id, PartialRes::new(res));
                            return;
                        }
                    }
                }
            }
        }
        visit::walk_generic_arg(self, arg)
    }

    fn visit_ty(&mut self, ty: &'ast Ty) {
        match &ty.kind {
            TyKind::Path(qself, path) => self.resolve_qself_path(qself, path, ty.id, PathSource::Type),
            TyKind::FnPtr(f) => {
                self.lifetime_ribs.push(Rib::new(RibKind::Normal));
                for gp in &f.generic_params {
                    let def = self.r.local_def_id(gp.id);
                    self.bind_generic_param(gp.ident.name, def);
                }
                for p in &f.decl.inputs {
                    self.visit_ty(&p.ty);
                }
                self.visit_fn_ret_ty(&f.decl.output);
                self.lifetime_ribs.pop();
            }
            TyKind::FuncValue(f) => {
                self.lifetime_ribs.push(Rib::new(RibKind::Normal));
                for gp in &f.generic_params {
                    let def = self.r.local_def_id(gp.id);
                    self.bind_generic_param(gp.ident.name, def);
                }
                for p in &f.decl.inputs {
                    self.visit_ty(&p.ty);
                }
                self.visit_fn_ret_ty(&f.decl.output);
                self.lifetime_ribs.pop();
            }
            TyKind::Array(t, len) => {
                self.visit_ty(t);
                self.with_rib(Namespace::ValueNS, RibKind::Constant, |this| this.visit_anon_const(len));
            }
            _ => visit::walk_ty(self, ty),
        }
    }

    fn visit_block(&mut self, b: &'ast Block) {
        let (types, values) = self.block_items_rib(b);
        self.ribs.type_ns.push(types);
        self.ribs.value_ns.push(values);
        // Bindings from `let` statements.
        self.ribs.value_ns.push(Rib::new(RibKind::Normal));
        for s in &b.stmts {
            self.visit_stmt(s);
        }
        self.ribs.value_ns.pop();
        self.ribs.value_ns.pop();
        self.ribs.type_ns.pop();
    }

    fn visit_local(&mut self, l: &'ast Local) {
        for a in &l.attrs {
            self.visit_attribute(a);
        }
        if let Some(t) = &l.ty {
            self.visit_ty(t);
        }
        match &l.kind {
            LocalKind::Decl => {}
            LocalKind::Init(e) => self.visit_expr(e),
            LocalKind::InitElse(e, els) => {
                self.visit_expr(e);
                self.visit_block(els);
            }
        }
        self.resolve_pattern(&l.pat, false);
    }

    fn visit_pat(&mut self, pat: &'ast Pat) {
        self.resolve_pattern(pat, false);
    }

    fn visit_expr(&mut self, e: &'ast Expr) {
        for a in &e.attrs {
            self.visit_attribute(a);
        }
        match &e.kind {
            ExprKind::Path(qself, path) => self.resolve_qself_path(qself, path, e.id, PathSource::Expr),
            ExprKind::Struct(s) => {
                self.resolve_qself_path(&s.qself, &s.path, e.id, PathSource::Struct);
                for f in &s.fields {
                    self.visit_expr(&f.expr);
                }
                if let StructRest::Base(b) = &s.rest {
                    self.visit_expr(b);
                }
            }
            ExprKind::Let(pat, scrutinee, _) => {
                self.visit_expr(scrutinee);
                // Bound into the enclosing condition's rib.
                self.resolve_pattern(pat, false);
            }
            ExprKind::If(cond, then, els) => {
                self.with_rib(Namespace::ValueNS, RibKind::Normal, |this| {
                    this.visit_expr(cond);
                    this.visit_block(then);
                });
                if let Some(els) = els {
                    self.visit_expr(els);
                }
            }
            ExprKind::While(cond, body, label) => self.with_label(label.as_ref(), e.id, |this| {
                this.with_rib(Namespace::ValueNS, RibKind::Normal, |this| {
                    this.visit_expr(cond);
                    this.visit_block(body);
                })
            }),
            ExprKind::ForLoop { pat, iter, body, label } => {
                self.visit_expr(iter);
                self.with_label(label.as_ref(), e.id, |this| {
                    this.with_rib(Namespace::ValueNS, RibKind::Normal, |this| {
                        this.resolve_pattern(pat, false);
                        this.visit_block(body);
                    })
                })
            }
            ExprKind::Loop(body, label, _) => self.with_label(label.as_ref(), e.id, |this| this.visit_block(body)),
            ExprKind::Block(b, label) => self.with_label(label.as_ref(), e.id, |this| this.visit_block(b)),
            ExprKind::Match(scrutinee, arms) => {
                self.visit_expr(scrutinee);
                for arm in arms {
                    self.visit_arm(arm);
                }
            }
            ExprKind::Closure(c) => self.with_label_barrier(|this| this.resolve_fn_decl_and_body(&c.fn_decl, Some(&c.body))),
            ExprKind::Async(b) => self.with_label_barrier(|this| this.visit_block(b)),
            ExprKind::Break(label, value) => {
                if let Some(l) = label {
                    self.resolve_label(l, e.id);
                }
                if let Some(v) = value {
                    if label.is_none() && self.report_break_name(v) {
                        return;
                    }
                    self.visit_expr(v);
                }
            }
            ExprKind::Continue(label) => {
                if let Some(l) = label {
                    self.resolve_label(l, e.id);
                }
            }
            ExprKind::ConstBlock(c) => self.with_rib(Namespace::ValueNS, RibKind::Constant, |this| this.visit_expr(&c.value)),
            ExprKind::Repeat(x, n) => {
                self.visit_expr(x);
                self.with_rib(Namespace::ValueNS, RibKind::Constant, |this| this.visit_anon_const(n));
            }
            // `vec[x; n]` takes a run-time count.
            ExprKind::Vec(inner) => match &inner.kind {
                ExprKind::Repeat(x, n) => {
                    self.visit_expr(x);
                    self.visit_expr(&n.value);
                }
                _ => self.visit_expr(inner),
            },
            ExprKind::MethodCall(m) => {
                self.visit_expr(&m.receiver);
                for a in &m.args {
                    self.visit_expr(a);
                }
            }
            ExprKind::Field(x, _) => self.visit_expr(x),
            _ => visit::walk_expr(self, e),
        }
    }

    fn visit_arm(&mut self, arm: &'ast Arm) {
        self.with_rib(Namespace::ValueNS, RibKind::Normal, |this| {
            this.resolve_pattern(&arm.pat, false);
            if let Some(g) = &arm.guard {
                this.visit_expr(g);
            }
            this.visit_expr(&arm.body);
        });
    }

    fn visit_inline_asm(&mut self, asm: &'ast InlineAsm) {
        for (op, _) in &asm.operands {
            match op {
                InlineAsmOperand::Sym { qself, path, id, .. } => self.resolve_qself_path(qself, path, *id, PathSource::Expr),
                InlineAsmOperand::Const { anon_const, .. } => {
                    self.with_rib(Namespace::ValueNS, RibKind::Constant, |this| this.visit_anon_const(anon_const))
                }
                InlineAsmOperand::In { expr, .. } | InlineAsmOperand::InOut { expr, .. } => self.visit_expr(expr),
                InlineAsmOperand::Out { expr, .. } => {
                    if let Some(e) = expr {
                        self.visit_expr(e);
                    }
                }
                InlineAsmOperand::SplitInOut { in_expr, out_expr, .. } => {
                    self.visit_expr(in_expr);
                    if let Some(e) = out_expr {
                        self.visit_expr(e);
                    }
                }
                InlineAsmOperand::Label { block, .. } => self.visit_block(block),
            }
        }
    }

    fn visit_fn(&mut self, kind: FnKind<'ast>) {
        // Every function goes through `resolve_fn_decl_and_body`.
        let _ = kind;
    }
}
