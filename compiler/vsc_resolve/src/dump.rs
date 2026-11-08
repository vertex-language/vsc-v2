//! `-Z dump-resolutions`: every path and lifetime with what it resolved to, in
//! source order. Tests compare against this.

use std::fmt::Write;

use vsc_ast::visit::{self, Visitor};
use vsc_ast::*;
use vsc_hir::def::{LifetimeRes, PartialRes, Res};
use vsc_span::def_id::{DefId, LocalDefId};
use vsc_span::{Ident, SourceMap, Span};

use crate::ResolverOutputs;

pub fn dump_resolutions(krate: &Crate, out: &ResolverOutputs, sm: &SourceMap) -> String {
    let mut d = Dumper { out, sm, lines: Vec::new() };
    d.visit_crate(krate);
    d.lines.sort_by_key(|(span, _)| span.lo());
    let mut text = String::new();
    for (_, line) in d.lines {
        let _ = writeln!(text, "{line}");
    }
    text
}

struct Dumper<'a> {
    out: &'a ResolverOutputs,
    sm: &'a SourceMap,
    lines: Vec<(Span, String)>,
}

impl Dumper<'_> {
    fn loc(&self, span: Span) -> String {
        match self.sm.lookup_char_pos(span.lo()) {
            Some(l) => format!("{}:{}", l.line, l.col + 1),
            None => "?".to_string(),
        }
    }

    fn def_path(&self, def: DefId) -> String {
        let Some(mut local) = def.as_local() else { return format!("{def:?}") };
        let mut parts = Vec::new();
        loop {
            let d = &self.out.defs[local];
            parts.push(d.name.map(|n| n.to_string()).unwrap_or_else(|| format!("{{{}}}", d.kind.descr())));
            match d.parent {
                Some(p) => local = p,
                None => break,
            }
        }
        parts.reverse();
        parts.join(".")
    }

    fn describe(&self, res: Res) -> String {
        match res {
            Res::Def(kind, def) => format!("{} `{}`", kind.descr(), self.def_path(def)),
            Res::PrimTy(p) => format!("builtin type `{p}`"),
            Res::BuiltinTy(s) => format!("builtin type `{s}`"),
            Res::Builtin(s) => format!("built-in `{s}`"),
            Res::Local(id) => format!("local #{}", id.as_u32()),
            Res::SelfTyParam { trait_ } => format!("`Self` of trait `{}`", self.def_path(trait_)),
            Res::SelfTyAlias { alias_to } => format!("`Self` of `{}`", self.def_path(alias_to)),
            Res::SelfCtor(def) => format!("`Self` constructor of `{}`", self.def_path(def)),
            Res::Err => "error".to_string(),
        }
    }

    fn path(&mut self, path: &Path, id: NodeId, what: &str) {
        let Some(&PartialRes { base_res, unresolved_segments }) = self.out.partial_res_map.get(&id) else { return };
        let text = path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join(".");
        let mut line = format!("{} {what} `{text}` -> {}", self.loc(path.span), self.describe(base_res));
        if unresolved_segments > 0 {
            let rest = path.segments[path.segments.len() - unresolved_segments..]
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join(".");
            let _ = write!(line, " + `{rest}`");
        }
        self.lines.push((path.span, line));
    }

    fn lifetime(&mut self, ident: Ident) {
        let Some(res) = self.out.lifetime_res_map.get(&ident.span) else { return };
        let target = match *res {
            LifetimeRes::Param(def) => format!("lifetime parameter `{}`", self.def_path(def)),
            LifetimeRes::Static => "static".to_string(),
            LifetimeRes::Err => "error".to_string(),
        };
        self.lines.push((ident.span, format!("{} lifetime `{ident}` -> {target}", self.loc(ident.span))));
    }
}

impl<'ast> Visitor<'ast> for Dumper<'_> {
    fn visit_expr(&mut self, e: &'ast Expr) {
        match &e.kind {
            ExprKind::Path(_, path) => self.path(path, e.id, "value"),
            ExprKind::Struct(s) => self.path(&s.path, e.id, "struct"),
            _ => {}
        }
        visit::walk_expr(self, e);
    }

    fn visit_ty(&mut self, ty: &'ast Ty) {
        if let TyKind::Path(_, path) = &ty.kind {
            self.path(path, ty.id, "type");
        }
        visit::walk_ty(self, ty);
    }

    fn visit_pat(&mut self, pat: &'ast Pat) {
        match &pat.kind {
            PatKind::Ident(_, ident, _) => {
                if let Some(&PartialRes { base_res, .. }) = self.out.partial_res_map.get(&pat.id) {
                    let what = match base_res {
                        Res::Local(id) if id == pat.id => format!("binds local #{}", id.as_u32()),
                        other => format!("-> {}", self.describe(other)),
                    };
                    self.lines.push((ident.span, format!("{} pattern `{ident}` {what}", self.loc(ident.span))));
                }
            }
            PatKind::Path(_, path) | PatKind::TupleStruct(_, path, _) | PatKind::Struct(_, path, ..) => {
                self.path(path, pat.id, "pattern")
            }
            _ => {}
        }
        visit::walk_pat(self, pat);
    }

    fn visit_trait_ref(&mut self, t: &'ast TraitRef) {
        self.path(&t.path, t.ref_id, "trait");
        visit::walk_trait_ref(self, t);
    }

    fn visit_lifetime_ref(&mut self, o: &'ast Lifetime) {
        self.lifetime(o.ident);
    }

    fn visit_generic_arg(&mut self, arg: &'ast GenericArg) {
        if let GenericArg::Type(ty) = arg {
            let ident = match &ty.kind {
                TyKind::Path(None, p) => p.as_ident(),
                TyKind::Slice(inner) => match &inner.kind {
                    TyKind::Path(None, p) => p.as_ident(),
                    _ => None,
                },
                _ => None,
            };
            if let Some(ident) = ident {
                if self.out.lifetime_res_map.contains_key(&ident.span) {
                    self.lifetime(ident);
                    return;
                }
            }
        }
        visit::walk_generic_arg(self, arg);
    }
}

#[allow(dead_code)]
fn _unused(_: LocalDefId) {}
