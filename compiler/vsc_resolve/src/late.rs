//! Resolving names in signatures and bodies (rustc's `late.rs`).
//!
//! Scopes are ribs, innermost last. An `Item` rib is a barrier: a nested item
//! can't see the locals, generic parameters, lifetimes or labels outside it.

use vsc_ast::visit::{self, AssocCtxt, FnKind, Visitor};
use vsc_ast::*;
use vsc_data_structures::FxHashMap;
use vsc_errors::{Applicability, ErrCode};
use vsc_hir::def::{CtorKind, DefKind, Namespace, LifetimeRes, PartialRes, PerNS, Res};
use vsc_span::def_id::LocalDefId;
use vsc_span::{Ident, Span, Symbol, kw};

use crate::Resolver;
use crate::ident::{builtin_type, builtin_value};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RibKind {
    Normal,
    /// A nested item: what's outside isn't visible inside, except items.
    Item,
    /// A constant's value: no locals from outside (E0435).
    Constant,
    /// A function literal or `async` block, for labels: the labels outside
    /// can't be reached from inside.
    FnLiteral,
}

struct Rib<R> {
    kind: RibKind,
    bindings: FxHashMap<Symbol, R>,
}

impl<R> Rib<R> {
    fn new(kind: RibKind) -> Rib<R> {
        Rib { kind, bindings: FxHashMap::default() }
    }
}

/// Where a path is, which decides what it must resolve to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PathSource {
    Expr,
    Type,
    Trait,
    /// A struct literal or struct pattern.
    Struct,
    /// `Path(…)` in a pattern.
    TupleStruct,
    /// A path pattern, such as `Status.Done`.
    Pat,
}

impl PathSource {
    fn ns(self) -> Namespace {
        match self {
            PathSource::Type | PathSource::Trait | PathSource::Struct => Namespace::TypeNS,
            PathSource::Expr | PathSource::TupleStruct | PathSource::Pat => Namespace::ValueNS,
        }
    }

    fn expected(self) -> &'static str {
        match self {
            PathSource::Expr => "value",
            PathSource::Type => "type",
            PathSource::Trait => "trait",
            PathSource::Struct => "struct, variant or union type",
            PathSource::TupleStruct => "tuple struct or tuple variant",
            PathSource::Pat => "unit struct, unit variant or constant",
        }
    }

    fn accepts(self, res: Res) -> bool {
        use DefKind::*;
        match (self, res) {
            (_, Res::Err) => true,
            (PathSource::Expr, Res::Def(kind, _)) => {
                matches!(kind, Fn | Const | ConstParam | Static { .. } | Ctor(..) | AssocFn | AssocConst)
            }
            (PathSource::Expr, Res::Local(_) | Res::SelfCtor(_) | Res::Builtin(_)) => true,
            (PathSource::Type, Res::Def(kind, _)) => {
                matches!(kind, Struct | Class | Union | Enum | TyAlias | ForeignTy | TyParam | AssocTy)
            }
            (PathSource::Type, Res::PrimTy(_) | Res::BuiltinTy(_) | Res::SelfTyParam { .. } | Res::SelfTyAlias { .. }) => true,
            (PathSource::Trait, Res::Def(Trait, _)) => true,
            (PathSource::Struct, Res::Def(kind, _)) => matches!(kind, Struct | Class | Union | Variant | TyAlias | AssocTy),
            (PathSource::Struct, Res::SelfTyAlias { .. } | Res::SelfTyParam { .. }) => true,
            (PathSource::TupleStruct, Res::Def(Ctor(_, CtorKind::Fn), _) | Res::SelfCtor(_)) => true,
            (PathSource::Pat, Res::Def(Ctor(_, CtorKind::Const) | Const | AssocConst, _) | Res::SelfCtor(_)) => true,
            _ => false,
        }
    }
}

pub(crate) fn resolve_bodies(r: &mut Resolver<'_>, krate: &Crate) {
    resolve_package(r, &krate.root);
}

fn resolve_package(r: &mut Resolver<'_>, pkg: &Package) {
    let package = r.local_def_id(pkg.id);
    for (file, f) in pkg.files.iter().enumerate() {
        let mut v = LateResolutionVisitor::new(r, package, file);
        for item in &f.items {
            v.visit_item(item);
        }
    }
    for child in &pkg.children {
        resolve_package(r, child);
    }
}

struct LateResolutionVisitor<'r, 'a> {
    r: &'r mut Resolver<'a>,
    package: LocalDefId,
    file: usize,
    ribs: PerNS<Vec<Rib<Res>>>,
    lifetime_ribs: Vec<Rib<LifetimeRes>>,
    label_ribs: Vec<Rib<NodeId>>,
    /// The bindings of the pattern (or parameter list) being resolved: name →
    /// (binding id, span). Or-patterns push one map per alternative.
    pat_bindings: Vec<FxHashMap<Symbol, (NodeId, Span)>>,
}

impl<'r, 'a> LateResolutionVisitor<'r, 'a> {
    fn new(r: &'r mut Resolver<'a>, package: LocalDefId, file: usize) -> Self {
        LateResolutionVisitor {
            r,
            package,
            file,
            ribs: PerNS::default(),
            lifetime_ribs: Vec::new(),
            label_ribs: Vec::new(),
            pat_bindings: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Ribs
    // -----------------------------------------------------------------------

    fn with_rib<T>(&mut self, ns: Namespace, kind: RibKind, f: impl FnOnce(&mut Self) -> T) -> T {
        self.ribs.get_mut(ns).push(Rib::new(kind));
        let r = f(self);
        self.ribs.get_mut(ns).pop();
        r
    }

    /// A barrier in every kind of rib: the start of a nested item.
    fn with_item_ribs<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.ribs.type_ns.push(Rib::new(RibKind::Item));
        self.ribs.value_ns.push(Rib::new(RibKind::Item));
        self.lifetime_ribs.push(Rib::new(RibKind::Item));
        self.label_ribs.push(Rib::new(RibKind::Item));
        let r = f(self);
        self.ribs.type_ns.pop();
        self.ribs.value_ns.pop();
        self.lifetime_ribs.pop();
        self.label_ribs.pop();
        r
    }

    fn with_label_barrier<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.label_ribs.push(Rib::new(RibKind::FnLiteral));
        let r = f(self);
        self.label_ribs.pop();
        r
    }

    fn bind(&mut self, ns: Namespace, name: Symbol, res: Res) {
        self.ribs.get_mut(ns).last_mut().expect("a rib to bind in").bindings.insert(name, res);
    }

    // -----------------------------------------------------------------------
    // Looking names up
    // -----------------------------------------------------------------------

    /// Ribs, then the package's items, then the file's imports, then the
    /// built-ins. `locals` false skips local bindings (for patterns, which
    /// can name constants but not shadow-check locals).
    fn lookup(&mut self, ident: Ident, ns: Namespace, locals: bool) -> Option<Res> {
        let mut crossed_item = false;
        let mut crossed_const = false;
        for i in (0..self.ribs.get(ns).len()).rev() {
            let rib = &self.ribs.get(ns)[i];
            if let Some(&res) = rib.bindings.get(&ident.name) {
                let is_local = matches!(res, Res::Local(_));
                if is_local && !locals {
                    // Keep looking for an item of the same name.
                } else if is_local && crossed_item {
                    self.r
                        .dcx()
                        .struct_span_err(ident.span, "can't capture dynamic environment in a function item")
                        .with_code(ErrCode(434))
                        .with_help("use a function literal instead: `func(…) { … }`")
                        .emit();
                    return Some(Res::Err);
                } else if is_local && crossed_const {
                    self.r
                        .dcx()
                        .struct_span_err(ident.span, "attempt to use a non-constant value in a constant")
                        .with_code(ErrCode(435))
                        .emit();
                    return Some(Res::Err);
                } else if crossed_item
                    && matches!(
                        res,
                        Res::Def(DefKind::TyParam | DefKind::ConstParam, _)
                            | Res::SelfTyParam { .. }
                            | Res::SelfTyAlias { .. }
                            | Res::SelfCtor(_)
                    )
                {
                    self.r
                        .dcx()
                        .struct_span_err(ident.span, "can't use generic parameters from outer item")
                        .with_code(ErrCode(401))
                        .emit();
                    return Some(Res::Err);
                } else {
                    return Some(res);
                }
            }
            match rib.kind {
                RibKind::Item => crossed_item = true,
                RibKind::Constant => crossed_const = true,
                // Only labels have function-literal barriers.
                RibKind::Normal | RibKind::FnLiteral => {}
            }
        }
        if let Some(b) = self.r.module_binding(self.package, ident.name, ns) {
            return Some(b.res);
        }
        if let Some(res) = self.r.import_res(self.package, self.file, ident.name, ns) {
            return Some(res);
        }
        match ns {
            Namespace::TypeNS => builtin_type(ident.name),
            Namespace::ValueNS => builtin_value(ident.name),
        }
    }

    /// The value ribs only, for locals: exception 7's first step.
    fn lookup_local(&self, ident: Ident) -> Option<Res> {
        let mut crossed_item = false;
        for rib in self.ribs.value_ns.iter().rev() {
            if let Some(&res) = rib.bindings.get(&ident.name) {
                if matches!(res, Res::Local(_)) {
                    return (!crossed_item).then_some(res);
                }
                return None;
            }
            if rib.kind == RibKind::Item {
                crossed_item = true;
            }
        }
        None
    }

    // -----------------------------------------------------------------------
    // Paths (exception 7)
    // -----------------------------------------------------------------------

    fn record(&mut self, id: NodeId, res: PartialRes) {
        if id != DUMMY_NODE_ID {
            self.r.partial_res_map.insert(id, res);
        }
    }

    fn resolve_path(&mut self, path: &Path, id: NodeId, source: PathSource) -> PartialRes {
        let res = self.resolve_path_inner(path, source);
        self.record(id, res);
        res
    }

    fn resolve_path_inner(&mut self, path: &Path, source: PathSource) -> PartialRes {
        let segs = &path.segments;
        let n = segs.len();
        let ns = source.ns();
        let first = segs[0].ident;

        let mut res = if n == 1 {
            match self.lookup(first, ns, true) {
                Some(res) => res,
                None => {
                    self.report_not_found(path, source);
                    return PartialRes::new(Res::Err);
                }
            }
        } else {
            // In expressions, a local shadows packages and types, as in Go:
            // `x.field` and `x.Method()` (exception 7).
            if ns == Namespace::ValueNS {
                if let Some(local) = self.lookup_local(first) {
                    return PartialRes::with_unresolved_segments(local, n - 1);
                }
            }
            match self.lookup(first, Namespace::TypeNS, true) {
                Some(res) => res,
                None => match self.lookup(first, Namespace::ValueNS, true) {
                    // A constant or static with fields: `Lifetime.x`.
                    Some(res) => return PartialRes::with_unresolved_segments(res, n - 1),
                    None => {
                        self.report_failed_to_resolve(first);
                        return PartialRes::new(Res::Err);
                    }
                },
            }
        };

        let mut i = 1;
        while i < n {
            let seg = segs[i].ident;
            let last = i == n - 1;
            match res {
                Res::Def(DefKind::Mod, def) => {
                    let module = def.expect_local();
                    let want = if last { ns } else { Namespace::TypeNS };
                    let other = if want == Namespace::TypeNS { Namespace::ValueNS } else { Namespace::TypeNS };
                    let found = self.r.module_binding(module, seg.name, want).or_else(|| {
                        // An intermediate value (`pkg.Const.field`), or the
                        // wrong namespace at the end, reported below.
                        self.r.module_binding(module, seg.name, other)
                    });
                    match found {
                        Some(b) => {
                            if !self.r.is_accessible_from(b.vis, self.package) {
                                self.r
                                    .dcx()
                                    .struct_span_err(seg.span, format!("{} `{seg}` is private", b.res.descr()))
                                    .with_code(ErrCode(603))
                                    .with_span_label(seg.span, format!("private {}", b.res.descr()))
                                    .with_span_note(b.span, format!("the {} `{seg}` is defined here", b.res.descr()))
                                    .with_help("make it `public`, or `package` to share it within the crate")
                                    .emit();
                            }
                            if !last && b.res.ns() == Some(Namespace::ValueNS) {
                                return PartialRes::with_unresolved_segments(b.res, n - i - 1);
                            }
                            res = b.res;
                        }
                        None => {
                            let pkg = self.r.modules[&module].name;
                            let what = if last { source.expected() } else { "item" };
                            let code = match (last, ns) {
                                (true, Namespace::ValueNS) => 425,
                                (true, Namespace::TypeNS) => 412,
                                _ => 433,
                            };
                            self.r
                                .dcx()
                                .struct_span_err(seg.span, format!("cannot find {what} `{seg}` in package `{pkg}`"))
                                .with_code(ErrCode(code))
                                .with_span_label(seg.span, format!("not found in `{pkg}`"))
                                .emit();
                            return PartialRes::new(Res::Err);
                        }
                    }
                }
                Res::Def(DefKind::Enum | DefKind::Trait, def) => {
                    let module = def.expect_local();
                    let want = if last { ns } else { Namespace::TypeNS };
                    match self.r.module_binding(module, seg.name, want).or_else(|| {
                        self.r.module_binding(module, seg.name, if want == Namespace::TypeNS { Namespace::ValueNS } else { Namespace::TypeNS })
                    }) {
                        Some(b) => res = b.res,
                        // An associated item from an `implement` block.
                        None => return PartialRes::with_unresolved_segments(res, n - i),
                    }
                }
                Res::Err => return PartialRes::new(Res::Err),
                // A type or a value: the rest are associated items, fields or
                // methods, for type checking.
                _ => return PartialRes::with_unresolved_segments(res, n - i),
            }
            i += 1;
        }

        if !source.accepts(res) {
            self.report_wrong_kind(path, res, source);
            return PartialRes::new(Res::Err);
        }
        PartialRes::new(res)
    }

    fn report_not_found(&mut self, path: &Path, source: PathSource) {
        let ident = path.segments[0].ident;
        // Found in the other namespace: a type used as a value, or the reverse.
        let other = if source.ns() == Namespace::TypeNS { Namespace::ValueNS } else { Namespace::TypeNS };
        if let Some(res) = self.lookup(ident, other, true) {
            if res != Res::Err {
                self.report_wrong_kind(path, res, source);
            }
            return;
        }
        let (code, what) = match source {
            PathSource::Expr => (425, "value"),
            PathSource::Type => (412, "type"),
            PathSource::Trait => (405, "trait"),
            PathSource::Struct => (422, "struct, variant or union type"),
            PathSource::TupleStruct | PathSource::Pat => (531, "tuple struct or tuple variant"),
        };
        if ident.name == kw::SelfLower && source == PathSource::Expr {
            self.report_self_value(ident);
            return;
        }
        if ident.name == kw::SelfUpper {
            self.r
                .dcx()
                .struct_span_err(ident.span, "cannot find type `Self` in this scope")
                .with_code(ErrCode(411))
                .with_span_label(ident.span, "`Self` is only available in traits and `implement` blocks")
                .emit();
            return;
        }
        let mut err = self
            .r
            .dcx()
            .struct_span_err(ident.span, format!("cannot find {what} `{ident}` in this scope"))
            .with_code(ErrCode(code))
            .with_span_label(ident.span, "not found in this scope");
        if let Some(note) = std_name_note(ident.name) {
            err.note(note);
        }
        if source == PathSource::Type || source == PathSource::Trait {
            if let Some(hint) = rust_type_hint(ident.name) {
                err.span_suggestion(ident.span, hint.0, hint.1, Applicability::MachineApplicable);
            }
            // `<t>` declared a lifetime (exception 11: types are PascalCase).
            if matches!(self.lookup_lifetime(ident), Some(LifetimeRes::Param(_))) {
                let upper: String = ident.name.as_str().chars().take(1).flat_map(char::to_uppercase).chain(ident.name.as_str().chars().skip(1)).collect();
                err = err
                    .with_note(format!("`{ident}` is a lifetime parameter: lowercase generic parameters are lifetimes"))
                    .with_help(format!("type parameters are `PascalCase`: `{upper}`"));
            }
        }
        err.emit();
    }

    fn report_self_value(&mut self, ident: Ident) {
        self.r
            .dcx()
            .struct_span_err(ident.span, "expected value, found package `self`")
            .with_code(ErrCode(424))
            .with_span_label(ident.span, "`self` value is only available in methods with a `self` parameter")
            .with_help("in a flat method, the receiver has the name you gave it: `func (t: &Task) m()` uses `t`")
            .emit();
    }

    fn report_failed_to_resolve(&mut self, ident: Ident) {
        if ident.name == kw::SelfLower {
            return self.report_self_value(ident);
        }
        let mut err = self
            .r
            .dcx()
            .struct_span_err(ident.span, format!("failed to resolve: use of undeclared type or package `{ident}`"))
            .with_code(ErrCode(433))
            .with_span_label(ident.span, format!("use of undeclared type or package `{ident}`"));
        if let Some(note) = std_name_note(ident.name) {
            err.note(note);
        } else if ident.name.as_str().chars().next().is_some_and(char::is_lowercase) {
            err.help(format!("if `{ident}` is a package, import it: `import \"path/to/{ident}\"`"));
        }
        err.emit();
    }

    fn report_wrong_kind(&mut self, path: &Path, res: Res, source: PathSource) {
        let span = path.span;
        let shown = path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join(".");
        let code = match source {
            PathSource::Expr => 423,
            PathSource::Type => 573,
            PathSource::Trait => 404,
            PathSource::Struct => 574,
            PathSource::TupleStruct | PathSource::Pat => 532,
        };
        let mut err = self
            .r
            .dcx()
            .struct_span_err(span, format!("expected {}, found {} `{shown}`", source.expected(), res.descr()))
            .with_code(ErrCode(code))
            .with_span_label(span, format!("not a {}", source.expected()));
        if source == PathSource::Expr && matches!(res, Res::Def(DefKind::Struct | DefKind::Class, _)) {
            err.help(format!("use struct literal syntax: `{shown} {{ … }}`"));
        }
        if source == PathSource::Type && matches!(res, Res::Def(DefKind::Trait, _)) {
            err.span_suggestion(span.shrink_to_lo(), "trait objects are written with `dyn`", "dyn ", Applicability::MaybeIncorrect);
        }
        err.emit();
    }

    // -----------------------------------------------------------------------
    // Lifetimes (exception 13)
    // -----------------------------------------------------------------------

    fn lookup_lifetime(&self, ident: Ident) -> Option<LifetimeRes> {
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

    fn resolve_lifetime(&mut self, ident: Ident) {
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
    fn generic_arg_as_lifetime(&mut self, ty: &Ty) -> bool {
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
    fn with_generics<T>(&mut self, generics: &Generics, extra: &[LocalDefId], f: impl FnOnce(&mut Self) -> T) -> T {
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

    fn bind_generic_param(&mut self, name: Symbol, def: LocalDefId) {
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
    fn settle_flat_receiver_params(&mut self, item: &Item, i: &Impl) -> Vec<LocalDefId> {
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

    fn resolve_fn_decl_and_body(&mut self, decl: &FnDecl, body: Option<&Block>) {
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
        self.lifetime_ribs.pop();
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    /// Resolves a pattern and binds its names in the innermost value rib.
    fn resolve_pattern(&mut self, pat: &Pat, in_param_list: bool) {
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

    fn resolve_pat_inner(&mut self, pat: &Pat) {
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

    fn new_binding(&mut self, ident: Ident, id: NodeId) {
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

    fn resolve_qself_path(&mut self, qself: &Option<P<QSelf>>, path: &Path, id: NodeId, source: PathSource) {
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

    fn visit_path_args(&mut self, path: &Path) {
        for seg in &path.segments {
            if let Some(args) = &seg.args {
                self.visit_generic_args(args);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Labels
    // -----------------------------------------------------------------------

    fn with_label<T>(&mut self, label: Option<&Label>, target: NodeId, f: impl FnOnce(&mut Self) -> T) -> T {
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
    fn report_break_name(&mut self, v: &Expr) -> bool {
        let ExprKind::Path(None, p) = &v.kind else { return false };
        let Some(ident) = p.as_ident() else { return false };
        if self.lookup(ident, Namespace::ValueNS, true).is_some() {
            return false;
        }
        let mut crossed = false;
        for rib in self.label_ribs.iter().rev() {
            if rib.bindings.contains_key(&ident.name) {
                if crossed {
                    self.r
                        .dcx()
                        .struct_span_err(ident.span, format!("use of unreachable label `{ident}`"))
                        .with_code(ErrCode(767))
                        .with_span_label(ident.span, format!("unreachable label `{ident}`"))
                        .with_note("labels are unreachable through functions and function literals")
                        .emit();
                    return true;
                }
                break;
            }
            match rib.kind {
                RibKind::Item => break,
                RibKind::FnLiteral => crossed = true,
                _ => {}
            }
        }
        false
    }

    fn resolve_label(&mut self, label: &Label, use_id: NodeId) {
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
    fn block_items_rib(&mut self, b: &Block) -> (Rib<Res>, Rib<Res>) {
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
                this.with_generics(&t.generics, &[], |this| {
                    this.with_rib(Namespace::TypeNS, RibKind::Normal, |this| {
                        this.bind(Namespace::TypeNS, kw::SelfUpper, Res::SelfTyParam { trait_: def });
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

/// A note for Rust standard library names, which can't resolve until
/// vsc-v2 can build `library/`.
fn std_name_note(name: Symbol) -> Option<String> {
    const STD_NAMES: &[&str] = &[
        "Option", "Some", "None", "Result", "Ok", "Err", "Vec", "Box", "String", "Rc", "Arc", "Weak", "Default", "Clone",
        "Copy", "Debug", "Display", "PartialEq", "Eq", "PartialOrd", "Ord", "Hash", "Iterator", "IntoIterator", "Fn",
        "FnMut", "FnOnce", "Drop", "From", "Into", "Send", "Sync", "Sized", "ToString", "AsRef", "Deref", "HashMap",
    ];
    STD_NAMES
        .contains(&name.as_str())
        .then(|| format!("`{name}` comes from the standard library, which vsc-v2 can't build yet"))
}

/// Rust's names for Vertex's primitive types (exception 2).
fn rust_type_hint(name: Symbol) -> Option<(&'static str, &'static str)> {
    Some(match name.as_str() {
        "i8" => ("Vertex writes `i8` as `int8`", "int8"),
        "i16" => ("Vertex writes `i16` as `int16`", "int16"),
        "i32" => ("Vertex writes `i32` as `int32`", "int32"),
        "i64" => ("Vertex writes `i64` as `int64`", "int64"),
        "i128" => ("Vertex writes `i128` as `int128`", "int128"),
        "isize" => ("Vertex writes `isize` as `int`", "int"),
        "u8" => ("Vertex writes `u8` as `uint8`", "uint8"),
        "u16" => ("Vertex writes `u16` as `uint16`", "uint16"),
        "u32" => ("Vertex writes `u32` as `uint32`", "uint32"),
        "u64" => ("Vertex writes `u64` as `uint64`", "uint64"),
        "u128" => ("Vertex writes `u128` as `uint128`", "uint128"),
        "usize" => ("Vertex writes `usize` as `uint`", "uint"),
        "f32" => ("Vertex writes `f32` as `float32`", "float32"),
        "f64" => ("Vertex writes `f64` as `float64`", "float64"),
        "str" => ("Vertex writes `str` as `string`", "string"),
        "String" => ("Vertex writes `String` as `string`", "string"),
        _ => return None,
    })
}
