//! Pretty-printing HIR generics, bounds, types, paths, and patterns.

use std::fmt::Write;

use vsc_ast::{ByRef, Mutability, RangeEnd};
use vsc_hir::*;
use vsc_span::kw;

use super::Printer;

impl<'a> Printer<'a> {
    // -----------------------------------------------------------------------
    // Generics
    // -----------------------------------------------------------------------

    pub(super) fn generic_params(&mut self, params: &[GenericParam]) {
        if params.is_empty() {
            return;
        }
        self.w("<");
        self.comma_sep(params, |p, gp| p.generic_param(gp));
        self.w(">");
    }

    pub(super) fn generic_param(&mut self, gp: &GenericParam) {
        match &gp.kind {
            GenericParamKind::Lifetime => {
                let _ = write!(self.out, "'{}", gp.name);
            }
            GenericParamKind::Type { default } => {
                self.w(gp.name.name.as_str());
                if let Some(d) = default {
                    self.w(" = ");
                    self.ty(d);
                }
            }
            GenericParamKind::Const { ty, default } => {
                let _ = write!(self.out, "const {}: ", gp.name);
                self.ty(ty);
                if let Some(d) = default {
                    self.w(" = ");
                    self.anon_const(d);
                }
            }
        }
    }

    pub(super) fn where_clause(&mut self, g: &Generics) {
        if g.predicates.is_empty() {
            return;
        }
        self.w(" where ");
        self.comma_sep(&g.predicates, |p, pred| match pred {
            WherePredicate::Bound { bound_generic_params, bounded_ty, bounds, .. } => {
                if !bound_generic_params.is_empty() {
                    p.w("for");
                    p.generic_params(bound_generic_params);
                    p.w(" ");
                }
                p.ty(bounded_ty);
                p.w(": ");
                p.bounds(bounds);
            }
            WherePredicate::Lifetime { lifetime, bounds, .. } => {
                p.lifetime(lifetime);
                p.w(": ");
                for (i, b) in bounds.iter().enumerate() {
                    if i > 0 {
                        p.w(" + ");
                    }
                    p.lifetime(b);
                }
            }
        });
    }

    pub(super) fn bounds(&mut self, bounds: &[GenericBound]) {
        for (i, b) in bounds.iter().enumerate() {
            if i > 0 {
                self.w(" + ");
            }
            match b {
                GenericBound::Trait(t) => self.poly_trait_ref(t),
                GenericBound::Outlives(o) => self.lifetime(o),
                GenericBound::Use(args, _) => {
                    self.w("use<");
                    self.comma_sep(args, |p, a| p.w(a.name.as_str()));
                    self.w(">");
                }
            }
        }
    }

    pub(super) fn poly_trait_ref(&mut self, t: &PolyTraitRef) {
        if !t.bound_generic_params.is_empty() {
            self.w("for");
            self.generic_params(&t.bound_generic_params);
            self.w(" ");
        }
        if t.maybe {
            self.w("?");
        }
        self.path(&t.trait_ref.path);
    }

    pub(super) fn lifetime(&mut self, o: &Lifetime) {
        match &o.kind {
            LifetimeKind::Elided => self.w("'_"),
            LifetimeKind::Named(name, _) => {
                let _ = write!(self.out, "'{name}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    pub(super) fn ty(&mut self, t: &Ty) {
        match &t.kind {
            TyKind::Slice(inner) => {
                self.w("[");
                self.ty(inner);
                self.w("]");
            }
            TyKind::Array(inner, len) => {
                self.w("[");
                self.ty(inner);
                self.w("; ");
                self.anon_const(len);
                self.w("]");
            }
            TyKind::Ptr(mt) => {
                self.w(if mt.mutbl.is_mut() { "*mut " } else { "*const " });
                self.ty(&mt.ty);
            }
            TyKind::Ref(lifetime, mt) => {
                self.w("&");
                if let LifetimeKind::Named(..) = lifetime.kind {
                    self.lifetime(lifetime);
                    self.w(" ");
                }
                if mt.mutbl.is_mut() {
                    self.w("mut ");
                }
                self.ty(&mt.ty);
            }
            TyKind::FnPtr(f) => {
                if !f.generic_params.is_empty() {
                    self.w("for");
                    self.generic_params(&f.generic_params);
                    self.w(" ");
                }
                self.safety(f.safety);
                if let Some(abi) = f.abi {
                    let _ = write!(self.out, "extern \"{abi}\" ");
                }
                self.w("fn(");
                for (i, ty) in f.decl.inputs.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    if let Some(Some(n)) = f.param_idents.get(i) {
                        let _ = write!(self.out, "{n}: ");
                    }
                    self.ty(ty);
                }
                if f.decl.c_variadic {
                    self.w(", ...");
                }
                self.w(")");
                self.fn_ret(&f.decl.output);
            }
            TyKind::FuncValue(f) => {
                self.w("func(");
                self.comma_sep(&f.decl.inputs, |p, t| p.ty(t));
                self.w(")");
                self.fn_ret(&f.decl.output);
            }
            TyKind::Never => self.w("!"),
            TyKind::Tup(tys) => {
                self.w("(");
                self.comma_sep(tys, |p, t| p.ty(t));
                if tys.len() == 1 {
                    self.w(",");
                }
                self.w(")");
            }
            TyKind::Path(q) => self.qpath(q, false),
            TyKind::TraitObject(traits, lifetime) => {
                self.w("dyn ");
                for (i, t) in traits.iter().enumerate() {
                    if i > 0 {
                        self.w(" + ");
                    }
                    self.poly_trait_ref(t);
                }
                if let Some(o) = lifetime {
                    self.w(" + ");
                    self.lifetime(o);
                }
            }
            TyKind::OpaqueDef(_, bounds) => {
                self.w("impl ");
                self.bounds(bounds);
            }
            TyKind::Infer => self.w("_"),
            TyKind::Err => self.w("/*error*/"),
        }
    }

    // -----------------------------------------------------------------------
    // Paths
    // -----------------------------------------------------------------------

    pub(super) fn path(&mut self, p: &Path) {
        self.segments(&p.segments, false);
    }

    pub(super) fn segments(&mut self, segs: &[PathSegment], in_expr: bool) {
        for (i, s) in segs.iter().enumerate() {
            if i > 0 {
                self.w("::");
            }
            self.segment(s, in_expr);
        }
    }

    pub(super) fn segment(&mut self, s: &PathSegment, in_expr: bool) {
        if s.ident.name != kw::Empty {
            self.w(s.ident.name.as_str());
        }
        if let Some(args) = &s.args {
            self.generic_args(args, in_expr);
        }
    }

    pub(super) fn generic_args(&mut self, a: &GenericArgs, in_expr: bool) {
        if a.parenthesized {
            // `Fn<(A, B), Output = C>` prints as `Fn(A, B) -> C`.
            self.w("(");
            if let Some(GenericArg::Type(Ty { kind: TyKind::Tup(inputs), .. })) = a.args.first() {
                self.comma_sep(inputs, |p, t| p.ty(t));
            }
            self.w(")");
            if let Some(AssocItemConstraint { kind: AssocItemConstraintKind::Equality(out), .. }) = a.constraints.first() {
                if !matches!(&out.kind, TyKind::Tup(v) if v.is_empty()) {
                    self.w(" -> ");
                    self.ty(out);
                }
            }
            return;
        }
        if a.args.is_empty() && a.constraints.is_empty() {
            return;
        }
        if in_expr {
            self.w("::");
        }
        self.w("<");
        let mut first = true;
        for arg in &a.args {
            if !first {
                self.w(", ");
            }
            first = false;
            match arg {
                GenericArg::Lifetime(o) => self.lifetime(o),
                GenericArg::Type(t) => self.ty(t),
                GenericArg::Const(c) => match &c.kind {
                    ConstArgKind::Path(q) => self.qpath(q, false),
                    ConstArgKind::Anon(a) => {
                        self.w("{ ");
                        self.anon_const(a);
                        self.w(" }");
                    }
                },
                GenericArg::Infer(_) => self.w("_"),
            }
        }
        for c in &a.constraints {
            if !first {
                self.w(", ");
            }
            first = false;
            let _ = write!(self.out, "{}", c.ident);
            match &c.kind {
                AssocItemConstraintKind::Equality(t) => {
                    self.w(" = ");
                    self.ty(t);
                }
                AssocItemConstraintKind::Bound(b) => {
                    self.w(": ");
                    self.bounds(b);
                }
            }
        }
        self.w(">");
    }

    pub(super) fn qpath(&mut self, q: &QPath, in_expr: bool) {
        match q {
            QPath::Resolved(None, p) => self.segments(&p.segments, in_expr),
            QPath::Resolved(Some(qself), p) => {
                // `<T as Trait>::Assoc`: the last segment is the item, the
                // ones before it the trait.
                self.w("<");
                self.ty(qself);
                self.w(" as ");
                let n = p.segments.len();
                self.segments(&p.segments[..n - 1], false);
                self.w(">::");
                self.segment(&p.segments[n - 1], in_expr);
            }
            QPath::TypeRelative(ty, seg) => {
                // A bare path type prints as `T::Assoc`; anything else needs
                // `<…>`.
                match &ty.kind {
                    TyKind::Path(inner) => self.qpath(inner, false),
                    _ => {
                        self.w("<");
                        self.ty(ty);
                        self.w(">");
                    }
                }
                self.w("::");
                self.segment(seg, in_expr);
            }
            QPath::LangItem(item, _) => {
                let _ = write!(self.out, "#[lang = \"{}\"]", item.name());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    pub(super) fn pat(&mut self, p: &Pat) {
        match &p.kind {
            PatKind::Wild => self.w("_"),
            PatKind::Binding(BindingMode(by_ref, mutbl), _, ident, sub) => {
                match by_ref {
                    ByRef::Yes(Mutability::Mut) => self.w("ref mut "),
                    ByRef::Yes(Mutability::Not) => self.w("ref "),
                    ByRef::No if mutbl.is_mut() => self.w("mut "),
                    ByRef::No => {}
                }
                self.w(ident.name.as_str());
                if let Some(s) = sub {
                    self.w(" @ ");
                    self.pat(s);
                }
            }
            PatKind::Struct(q, fields, rest) => {
                self.qpath(q, false);
                self.w(" { ");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    if !f.is_shorthand {
                        let _ = write!(self.out, "{}: ", f.ident);
                    }
                    self.pat(&f.pat);
                }
                if *rest {
                    if !fields.is_empty() {
                        self.w(", ");
                    }
                    self.w("..");
                }
                self.w(" }");
            }
            PatKind::TupleStruct(q, pats, rest) => {
                self.qpath(q, false);
                self.tuple_pats(pats, *rest);
            }
            PatKind::Or(alts) => {
                for (i, a) in alts.iter().enumerate() {
                    if i > 0 {
                        self.w(" | ");
                    }
                    self.pat(a);
                }
            }
            PatKind::Path(q) => self.qpath(q, false),
            PatKind::Tuple(pats, rest) => {
                if pats.len() == 1 && rest.is_none() {
                    self.w("(");
                    self.pat(&pats[0]);
                    self.w(",)");
                } else {
                    self.tuple_pats(pats, *rest);
                }
            }
            PatKind::Ref(inner, m) => {
                self.w(if m.is_mut() { "&mut " } else { "&" });
                self.pat(inner);
            }
            PatKind::Expr(e) => self.expr(e),
            PatKind::Range(lo, hi, end) => {
                if let Some(lo) = lo {
                    self.expr(lo);
                }
                self.w(if *end == RangeEnd::Included { "..=" } else { ".." });
                if let Some(hi) = hi {
                    self.expr(hi);
                }
            }
            PatKind::Slice(before, slice, after) => {
                self.w("[");
                let mut first = true;
                for p in before {
                    if !first {
                        self.w(", ");
                    }
                    first = false;
                    self.pat(p);
                }
                if let Some(s) = slice {
                    if !first {
                        self.w(", ");
                    }
                    first = false;
                    match s.kind {
                        PatKind::Wild => {}
                        _ => {
                            self.pat(s);
                            self.w(" @ ");
                        }
                    }
                    self.w("..");
                }
                for p in after {
                    if !first {
                        self.w(", ");
                    }
                    first = false;
                    self.pat(p);
                }
                self.w("]");
            }
            PatKind::Err => self.w("/*error*/"),
        }
    }

    pub(super) fn tuple_pats(&mut self, pats: &[Pat], rest: Option<usize>) {
        self.w("(");
        let mut first = true;
        for (i, p) in pats.iter().enumerate() {
            if rest == Some(i) {
                if !first {
                    self.w(", ");
                }
                first = false;
                self.w("..");
            }
            if !first {
                self.w(", ");
            }
            first = false;
            self.pat(p);
        }
        if rest == Some(pats.len()) {
            if !first {
                self.w(", ");
            }
            self.w("..");
        }
        self.w(")");
    }

}
