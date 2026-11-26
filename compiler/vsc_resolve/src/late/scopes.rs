//! Lifetime/generic scopes, function declarations, patterns, labels, and block item ribs.

use vsc_ast::visit::Visitor;
use vsc_ast::*;
use vsc_data_structures::FxHashMap;
use vsc_errors::ErrCode;
use vsc_hir::def::{CtorKind, DefKind, LifetimeRes, Namespace, PartialRes, Res};
use vsc_span::def_id::LocalDefId;
use vsc_span::{Ident, Span, Symbol, kw};

use super::{LateResolutionVisitor, PathSource, Rib, RibKind};

impl<'r, 'a> LateResolutionVisitor<'r, 'a> {
    pub(super) fn lookup_lifetime(&self, ident: Ident) -> Option<LifetimeRes> {
        if ident.name == kw::Static {
            return Some(LifetimeRes::Static);
        }
        for rib in self.lifetime_ribs.iter().rev() {
            if let Some(&res) = rib.bindings.get(&ident.name) {
                return Some(res);
            }
            if rib.kind == RibKind::Item {
                return None;
            }
        }
        None
    }

    pub(super) fn resolve_lifetime(&mut self, ident: Ident) {
        let res = match self.lookup_lifetime(ident) {
            Some(res) => res,
            None => {
                self.r
                    .dcx()
                    .struct_span_err(ident.span, format!("use of undeclared lifetime name `{ident}`"))
                    .with_code(ErrCode(261))
                    .with_span_label(ident.span, "undeclared lifetime")
                    .with_help(format!("consider introducing lifetime `{ident}` here: `<{ident}>`"))
                    .emit();
                LifetimeRes::Err
            }
        };
        self.r.lifetime_res_map.insert(ident.span, res);
    }

    /// A generic argument that names a lifetime rather than a type: `src` in
    /// `Highlight<src>`.
    pub(super) fn generic_arg_as_lifetime(&mut self, ty: &Ty) -> bool {
        let TyKind::Path(None, p) = &ty.kind else { return false };
        let Some(ident) = p.as_ident() else { return false };
        // A bare name that is a type stays a type.
        if self.lookup(ident, Namespace::TypeNS, true).is_some() {
            return false;
        }
        match self.lookup_lifetime(ident) {
            Some(res) if ident.name != kw::Static => {
                self.r.lifetime_res_map.insert(ident.span, res);
                true
            }
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // Generics
    // -----------------------------------------------------------------------

    /// Puts `generics`' parameters (and `extra`, from a flat receiver) in
    /// scope, resolves their bounds and where clause, then runs `f`.
    pub(super) fn with_generics<T>(&mut self, generics: &Generics, extra: &[LocalDefId], f: impl FnOnce(&mut Self) -> T) -> T {
        self.ribs.type_ns.push(Rib::new(RibKind::Normal));
        self.ribs.value_ns.push(Rib::new(RibKind::Normal));
        self.lifetime_ribs.push(Rib::new(RibKind::Normal));
        let mut seen: FxHashMap<Symbol, Span> = FxHashMap::default();
        for p in &generics.params {
            if let Some(&prev) = seen.get(&p.ident.name) {
                self.r
                    .dcx()
                    .struct_span_err(p.ident.span, format!("the name `{}` is already used for a generic parameter", p.ident))
                    .with_code(ErrCode(403))
                    .with_span_label(prev, format!("first use of `{}`", p.ident))
                    .with_span_label(p.ident.span, "already used")
                    .emit();
                continue;
            }
            seen.insert(p.ident.name, p.ident.span);
            let def = self.r.local_def_id(p.id);
            self.bind_generic_param(p.ident.name, def);
        }
        for &def in extra {
            if let Some(name) = self.r.defs[def].name {
                self.bind_generic_param(name, def);
            }
        }
        for p in &generics.params {
            for b in &p.bounds {
                self.visit_param_bound(b);
            }
            match &p.kind {
                GenericParamKind::Type { default: Some(d) } => self.visit_ty(d),
                GenericParamKind::Const { ty, default, .. } => {
                    self.visit_ty(ty);
                    if let Some(d) = default {
                        self.visit_anon_const(d);
                    }
                }
                _ => {}
            }
        }
        for pred in &generics.where_clause.predicates {
            self.visit_where_predicate(pred);
        }
        let r = f(self);
        self.ribs.type_ns.pop();
        self.ribs.value_ns.pop();
        self.lifetime_ribs.pop();
        r
    }

    pub(super) fn bind_generic_param(&mut self, name: Symbol, def: LocalDefId) {
        let kind = self.r.defs[def].kind;
        let res = Res::Def(kind, def.to_def_id());
        match kind {
            DefKind::LifetimeParam => {
                self.lifetime_ribs.last_mut().unwrap().bindings.insert(name, LifetimeRes::Param(def.to_def_id()));
            }
            DefKind::ConstParam => self.bind(Namespace::ValueNS, name, res),
            _ => self.bind(Namespace::TypeNS, name, res),
        }
    }

    /// A flat receiver's introduced parameters take their kinds from the
    /// receiver type's own generics: in `func (h: &Highlight<src>) Part()`,
    /// `src` is a lifetime because `Highlight` declares `src: Lifetime`.
    pub(super) fn settle_flat_receiver_params(&mut self, item: &Item, i: &Impl) -> Vec<LocalDefId> {
        let Some(params) = self.r.flat_receiver_params.get(&item.id).cloned() else { return Vec::new() };
        let TyKind::Path(None, path) = &i.self_ty.kind else { return params };
        // Look the type up quietly: visiting the self type reports errors.
        let [seg] = &path.segments[..] else { return params };
        let Some(Res::Def(_, def)) = self.lookup(seg.ident, Namespace::TypeNS, true) else { return params };
        let Some(declared) = def.as_local().and_then(|d| self.r.generics_of.get(&d)).cloned() else { return params };
        let Some(GenericArgs::AngleBracketed(args)) = path.segments.last().and_then(|s| s.args.as_deref()) else {
            return params;
        };
        for (position, arg) in args.args.iter().enumerate() {
            let name = match arg {
                AngleBracketedArg::Arg(GenericArg::Type(t)) => match &t.kind {
                    TyKind::Path(None, p) => p.as_ident(),
                    TyKind::Slice(inner) => match &inner.kind {
                        TyKind::Path(None, p) => p.as_ident(),
                        _ => None,
                    },
                    _ => None,
                },
                AngleBracketedArg::Arg(GenericArg::Lifetime(o)) => Some(o.ident),
                _ => None,
            };
            let Some(name) = name else { continue };
            let Some(&(_, kind)) = declared.get(position) else { continue };
            if let Some(&param) = params.iter().find(|&&p| self.r.defs[p].name == Some(name.name)) {
                self.r.defs[param].kind = kind;
            }
        }
        params
    }

    // -----------------------------------------------------------------------
    // Functions
    // -----------------------------------------------------------------------

    pub(super) fn resolve_fn_decl_and_body(&mut self, decl: &FnDecl, body: Option<&Block>) {
        self.with_rib(Namespace::ValueNS, RibKind::Normal, |this| {
            this.pat_bindings.push(FxHashMap::default());
            for p in &decl.inputs {
                for a in &p.attrs {
                    this.visit_attribute(a);
                }
                this.visit_ty(&p.ty);
                this.resolve_pattern(&p.pat, true);
            }
            this.pat_bindings.pop();
            if let FnRetTy::Ty(t) = &decl.output {
                this.visit_ty(t);
            }
            if let Some(b) = body {
                this.visit_block(b);
            }
        });
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    /// Resolves a pattern and binds its names in the innermost value rib.
    pub(super) fn resolve_pattern(&mut self, pat: &Pat, in_param_list: bool) {
        if in_param_list {
            // The parameter list shares one set of bindings (E0416 across
            // parameters), pushed by the caller.
            self.resolve_pat_inner(pat);
        } else {
            self.pat_bindings.push(FxHashMap::default());
            self.resolve_pat_inner(pat);
            self.pat_bindings.pop();
        }
    }

    pub(super) fn resolve_pat_inner(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Ident(mode, ident, sub) => {
                if *mode == BindingMode::NONE && sub.is_none() {
                    // A name that is a constant or unit struct/variant in scope
                    // is a path pattern, as in Rust.
                    match self.lookup(*ident, Namespace::ValueNS, false) {
                        Some(res @ Res::Def(DefKind::Ctor(_, CtorKind::Const) | DefKind::Const | DefKind::AssocConst, _)) => {
                            self.record(pat.id, PartialRes::new(res));
                            return;
                        }
                        Some(res @ Res::Def(DefKind::Ctor(_, CtorKind::Fn) | DefKind::Static { .. }, _)) => {
                            self.r
                                .dcx()
                                .struct_span_err(ident.span, format!("bindings cannot shadow {}s", res.descr()))
                                .with_code(ErrCode(530))
                                .with_span_label(ident.span, format!("cannot be named the same as a {}", res.descr()))
                                .emit();
                            self.record(pat.id, PartialRes::new(Res::Err));
                            return;
                        }
                        _ => {}
                    }
                }
                self.new_binding(*ident, pat.id);
                if let Some(s) = sub {
                    self.resolve_pat_inner(s);
                }
            }
            PatKind::Or(alts) => {
                let mut sets = Vec::new();
                let outer = self.pat_bindings.pop().unwrap_or_default();
                for alt in alts {
                    self.pat_bindings.push(outer.clone());
                    self.resolve_pat_inner(alt);
                    sets.push(self.pat_bindings.pop().unwrap());
                }
                // Every alternative binds the same names (E0408), and they
                // all refer to the first alternative's bindings.
                let first = sets[0].clone();
                let mut names: Vec<(Symbol, Span)> = Vec::new();
                for set in &sets {
                    for (name, &(_, span)) in set {
                        if !outer.contains_key(name) && !names.iter().any(|(n, _)| n == name) {
                            names.push((*name, span));
                        }
                    }
                }
                for (name, bound_at) in names {
                    for (i, set) in sets.iter().enumerate() {
                        if !set.contains_key(&name) {
                            self.r
                                .dcx()
                                .struct_span_err(alts[i].span, format!("variable `{name}` is not bound in all patterns"))
                                .with_code(ErrCode(408))
                                .with_span_label(bound_at, "variable not in all patterns")
                                .with_span_label(alts[i].span, format!("pattern doesn't bind `{name}`"))
                                .emit();
                        }
                    }
                }
                for set in &sets[1..] {
                    for (name, &(id, _)) in set {
                        if let Some(&(first_id, _)) = first.get(name) {
                            self.record(id, PartialRes::new(Res::Local(first_id)));
                            self.bind(Namespace::ValueNS, *name, Res::Local(first_id));
                        }
                    }
                }
                for (name, (id, _)) in &first {
                    self.bind(Namespace::ValueNS, *name, Res::Local(*id));
                }
                let mut merged = outer;
                merged.extend(first);
                self.pat_bindings.push(merged);
            }
            PatKind::TupleStruct(qself, path, pats) => {
                self.resolve_qself_path(qself, path, pat.id, PathSource::TupleStruct);
                for p in pats {
                    self.resolve_pat_inner(p);
                }
            }
            PatKind::Struct(qself, path, fields, _) => {
                self.resolve_qself_path(qself, path, pat.id, PathSource::Struct);
                for f in fields {
                    self.resolve_pat_inner(&f.pat);
                }
            }
            PatKind::Path(qself, path) => self.resolve_qself_path(qself, path, pat.id, PathSource::Pat),
            PatKind::Tuple(pats) | PatKind::Slice(pats) => {
                for p in pats {
                    self.resolve_pat_inner(p);
                }
            }
            PatKind::Ref(p, _) | PatKind::Paren(p) => self.resolve_pat_inner(p),
            PatKind::Expr(e) => self.visit_expr(e),
            PatKind::Range(lo, hi, _) => {
                if let Some(e) = lo {
                    self.visit_expr(e);
                }
                if let Some(e) = hi {
                    self.visit_expr(e);
                }
            }
            PatKind::Wild | PatKind::Rest | PatKind::Err => {}
        }
    }

    pub(super) fn new_binding(&mut self, ident: Ident, id: NodeId) {
        if ident.name.is_builtin_name() {
            self.r.report_builtin_redeclared(ident);
        }
        if let Some(set) = self.pat_bindings.last_mut() {
            if let Some(&(_, prev)) = set.get(&ident.name) {
                self.r
                    .dcx()
                    .struct_span_err(ident.span, format!("identifier `{ident}` is bound more than once in the same pattern"))
                    .with_code(ErrCode(416))
                    .with_span_label(prev, "first bound here")
                    .with_span_label(ident.span, "used in a pattern more than once")
                    .emit();
                return;
            }
            set.insert(ident.name, (id, ident.span));
        }
        self.record(id, PartialRes::new(Res::Local(id)));
        self.bind(Namespace::ValueNS, ident.name, Res::Local(id));
    }

    pub(super) fn resolve_qself_path(&mut self, qself: &Option<P<QSelf>>, path: &Path, id: NodeId, source: PathSource) {
        match qself {
            None => {
                self.resolve_path(path, id, source);
                self.visit_path_args(path);
            }
            Some(q) => {
                self.visit_ty(&q.ty);
                if q.position > 0 {
                    let trait_path = Path { span: path.span, segments: path.segments[..q.position].to_vec() };
                    let res = self.resolve_path_inner(&trait_path, PathSource::Trait);
                    let unresolved = path.segments.len() - q.position;
                    self.record(id, PartialRes::with_unresolved_segments(res.base_res, unresolved));
                }
                self.visit_path_args(path);
            }
        }
    }

    pub(super) fn visit_path_args(&mut self, path: &Path) {
        for seg in &path.segments {
            if let Some(args) = &seg.args {
                self.visit_generic_args(args);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Labels
    // -----------------------------------------------------------------------

    pub(super) fn with_label<T>(&mut self, label: Option<&Label>, target: NodeId, f: impl FnOnce(&mut Self) -> T) -> T {
        let Some(label) = label else { return f(self) };
        let mut rib = Rib::new(RibKind::Normal);
        rib.bindings.insert(label.ident.name, target);
        self.label_ribs.push(rib);
        let r = f(self);
        self.label_ribs.pop();
        r
    }

    /// `break name` where `name` is neither a value nor an enclosing label
    /// (the parser reads it as a value, since no enclosing loop has that
    /// label). A label the name could mean is outside a function literal:
    /// Rust's E0767. Returns whether it reported.

    pub(super) fn resolve_label(&mut self, label: &Label, use_id: NodeId) {
        for rib in self.label_ribs.iter().rev() {
            if let Some(&target) = rib.bindings.get(&label.ident.name) {
                self.r.label_res_map.insert(use_id, target);
                return;
            }
            if matches!(rib.kind, RibKind::Item | RibKind::FnLiteral) {
                break;
            }
        }
        self.r
            .dcx()
            .struct_span_err(label.ident.span, format!("use of undeclared label `{}`", label.ident))
            .with_code(ErrCode(426))
            .with_span_label(label.ident.span, format!("undeclared label `{}`", label.ident))
            .emit();
    }

    // -----------------------------------------------------------------------
    // Blocks
    // -----------------------------------------------------------------------

    /// Items declared in a block are visible in all of it.
    pub(super) fn block_items_rib(&mut self, b: &Block) -> (Rib<Res>, Rib<Res>) {
        let mut types = Rib::new(RibKind::Normal);
        let mut values = Rib::new(RibKind::Normal);
        for s in &b.stmts {
            let StmtKind::Item(item) = &s.kind else { continue };
            let def = self.r.local_def_id(item.id);
            let res = Res::Def(self.r.defs[def].kind, def.to_def_id());
            match &item.kind {
                ItemKind::Fn(f) => {
                    values.bindings.insert(f.ident.name, res);
                }
                ItemKind::Const(c) => {
                    values.bindings.insert(c.ident.name, res);
                }
                ItemKind::Static(st) => {
                    values.bindings.insert(st.ident.name, res);
                }
                ItemKind::Struct(ident, _, data) => {
                    types.bindings.insert(ident.name, res);
                    if let Some(ctor) = crate::build_reduced_graph::ctor_id(data) {
                        let cdef = self.r.local_def_id(ctor);
                        values.bindings.insert(ident.name, Res::Def(self.r.defs[cdef].kind, cdef.to_def_id()));
                    }
                }
                ItemKind::Class(ident, ..) | ItemKind::Union(ident, ..) | ItemKind::Enum(ident, ..) => {
                    types.bindings.insert(ident.name, res);
                }
                ItemKind::Trait(t) => {
                    types.bindings.insert(t.ident.name, res);
                }
                ItemKind::TyAlias(t) => {
                    types.bindings.insert(t.ident.name, res);
                }
                ItemKind::Impl(_) | ItemKind::ForeignMod(_) | ItemKind::GlobalAsm(_) => {}
            }
        }
        (types, values)
    }
}

