//! Resolving names in signatures and bodies (rustc's `late.rs`).
//!
//! Scopes are ribs, innermost last. An `Item` rib is a barrier: a nested item
//! can't see the locals, generic parameters, lifetimes or labels outside it.

mod diagnostics;
mod scopes;
mod visit;

use vsc_ast::visit::Visitor;
use vsc_ast::*;
use vsc_data_structures::FxHashMap;
use vsc_errors::ErrCode;
use vsc_hir::def::{CtorKind, DefKind, LifetimeRes, Namespace, PartialRes, PerNS, Res};
use vsc_span::def_id::LocalDefId;
use vsc_span::{Ident, Span, Symbol};

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
    for ext in &krate.extern_crates {
        resolve_package(r, ext);
    }
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

}
