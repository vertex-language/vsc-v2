//! Pretty-printing generics, bounds, paths, types, and patterns.

use vsc_ast::*;

use super::Printer;

impl Printer {
    // -----------------------------------------------------------------------
    // Generics
    // -----------------------------------------------------------------------

    pub(super) fn generic_params(&mut self, params: &[GenericParam]) {
        if params.is_empty() {
            return;
        }
        self.w("<");
        self.comma_sep(params, |p, param| p.generic_param(param));
        self.w(">");
    }

    pub(super) fn generic_param(&mut self, param: &GenericParam) {
        for a in &param.attrs {
            self.attr(a);
            self.w(" ");
        }
        match &param.kind {
            GenericParamKind::Lifetime => {
                self.ident(param.ident);
                if !param.bounds.is_empty() {
                    self.w(": ");
                    self.bounds(&param.bounds);
                }
            }
            GenericParamKind::Type { default } => {
                self.ident(param.ident);
                if !param.bounds.is_empty() {
                    self.w(": ");
                    self.bounds(&param.bounds);
                }
                if let Some(d) = default {
                    self.w(" = ");
                    self.ty(d);
                }
            }
            GenericParamKind::Const { ty, default, .. } => {
                self.w("const ");
                self.ident(param.ident);
                self.w(": ");
                self.ty(ty);
                if let Some(d) = default {
                    self.w(" = ");
                    self.expr(&d.value);
                }
            }
        }
    }

    pub(super) fn where_clause(&mut self, wc: &WhereClause) {
        if wc.predicates.is_empty() {
            return;
        }
        self.w(" where ");
        self.comma_sep(&wc.predicates, |p, pred| match &pred.kind {
            WherePredicateKind::BoundPredicate { bound_generic_params, bounded_ty, bounds } => {
                if !bound_generic_params.is_empty() {
                    p.w("for");
                    p.generic_params(bound_generic_params);
                    p.w(" ");
                }
                p.ty(bounded_ty);
                p.w(": ");
                p.bounds(bounds);
            }
        });
    }

    pub(super) fn bounds(&mut self, bounds: &[GenericBound]) {
        for (i, b) in bounds.iter().enumerate() {
            if i > 0 {
                self.w(" + ");
            }
            self.bound(b);
        }
    }

    pub(super) fn bound(&mut self, b: &GenericBound) {
        match b {
            GenericBound::Trait(t) => {
                if t.parens {
                    self.w("(");
                }
                if t.modifiers.maybe {
                    self.w("?");
                }
                if !t.bound_generic_params.is_empty() {
                    self.w("for");
                    self.generic_params(&t.bound_generic_params);
                    self.w(" ");
                }
                self.path(&t.trait_ref.path, true);
                if t.parens {
                    self.w(")");
                }
            }
            GenericBound::Outlives(o) => self.lifetime_ref(o),
            GenericBound::Use(idents, _) => {
                self.w("use<");
                self.comma_sep(idents, |p, i| p.ident(*i));
                self.w(">");
            }
        }
    }

    pub(super) fn lifetime_ref(&mut self, o: &Lifetime) {
        self.w(o.ident.name.as_str());
    }

    // -----------------------------------------------------------------------
    // Paths and types
    // -----------------------------------------------------------------------

    pub(super) fn path(&mut self, path: &Path, with_args: bool) {
        for (i, seg) in path.segments.iter().enumerate() {
            if i > 0 {
                self.w(".");
            }
            self.ident(seg.ident);
            if with_args {
                if let Some(args) = &seg.args {
                    self.generic_args(args);
                }
            }
        }
    }

    pub(super) fn qpath(&mut self, qself: &QSelf, path: &Path) {
        self.w("<");
        self.ty(&qself.ty);
        if qself.position > 0 {
            self.w(" as ");
            let trait_path = Path { span: path.span, segments: path.segments[..qself.position].to_vec() };
            self.path(&trait_path, true);
        }
        self.w(">");
        for seg in &path.segments[qself.position..] {
            self.w(".");
            self.ident(seg.ident);
            if let Some(args) = &seg.args {
                self.generic_args(args);
            }
        }
    }

    pub(super) fn generic_args(&mut self, args: &GenericArgs) {
        match args {
            GenericArgs::AngleBracketed(a) => {
                self.w("<");
                self.comma_sep(&a.args, |p, arg| match arg {
                    AngleBracketedArg::Arg(GenericArg::Type(t)) => p.ty(t),
                    AngleBracketedArg::Arg(GenericArg::Lifetime(o)) => p.lifetime_ref(o),
                    AngleBracketedArg::Arg(GenericArg::Const(c)) => p.expr(&c.value),
                    AngleBracketedArg::Constraint(c) => {
                        p.ident(c.ident);
                        match &c.kind {
                            AssocItemConstraintKind::Equality { term } => {
                                p.w(" = ");
                                p.ty(term);
                            }
                            AssocItemConstraintKind::Bound { bounds } => {
                                p.w(": ");
                                p.bounds(bounds);
                            }
                        }
                    }
                });
                self.w(">");
            }
            GenericArgs::Parenthesized(a) => {
                self.w("(");
                self.comma_sep(&a.inputs, |p, t| p.ty(t));
                self.w(")");
                self.ret_ty(&a.output);
            }
        }
    }

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
                self.expr(&len.value);
                self.w("]");
            }
            TyKind::Ptr(mt) => {
                self.w(if mt.mutbl.is_mut() { "*mut " } else { "*const " });
                self.ty(&mt.ty);
            }
            TyKind::Ref(lifetime, mt) => {
                self.w("&");
                if let Some(o) = lifetime {
                    self.lifetime_ref(o);
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
                if matches!(f.safety, Safety::Unsafe(_)) {
                    self.w("unsafe ");
                }
                self.extern_(f.ext);
                self.w("func");
                self.fn_ty_decl(&f.decl);
            }
            TyKind::FuncValue(f) => {
                if !f.generic_params.is_empty() {
                    self.w("for");
                    self.generic_params(&f.generic_params);
                    self.w(" ");
                }
                self.w("func");
                self.fn_ty_decl(&f.decl);
            }
            TyKind::Never => self.w("never"),
            TyKind::Tup(tys) => {
                self.w("(");
                self.comma_sep(tys, |p, t| p.ty(t));
                if tys.len() == 1 {
                    self.w(",");
                }
                self.w(")");
            }
            TyKind::Path(None, path) => self.path(path, true),
            TyKind::Path(Some(q), path) => self.qpath(q, path),
            TyKind::TraitObject(bounds) => {
                self.w("dyn ");
                self.bounds(bounds);
            }
            TyKind::ImplTrait(_, bounds) => {
                self.w("impl ");
                self.bounds(bounds);
            }
            TyKind::Paren(inner) => {
                self.w("(");
                self.ty(inner);
                self.w(")");
            }
            TyKind::Infer => self.w("_"),
            TyKind::ImplicitSelf => self.w("Self"),
            TyKind::CVarArgs => self.w("..."),
            TyKind::Err => self.w("/*error*/"),
        }
    }

    pub(super) fn fn_ty_decl(&mut self, decl: &FnDecl) {
        self.w("(");
        self.comma_sep(&decl.inputs, |p, param| {
            if let TyKind::CVarArgs = param.ty.kind {
                p.w("...");
                return;
            }
            match &param.pat.kind {
                PatKind::Ident(_, ident, _) => {
                    p.ident(*ident);
                    p.w(": ");
                }
                PatKind::Wild if !param.pat.span.is_empty() => p.w("_: "),
                _ => {}
            }
            p.ty(&param.ty);
        });
        self.w(")");
        self.ret_ty(&decl.output);
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    pub(super) fn pat(&mut self, pat: &Pat) {
        match &pat.kind {
            PatKind::Wild => self.w("_"),
            PatKind::Ident(mode, ident, sub) => {
                self.binding_mode(*mode);
                self.ident(*ident);
                if let Some(sub) = sub {
                    self.w(" @ ");
                    self.pat(sub);
                }
            }
            PatKind::Struct(qself, path, fields, rest) => {
                self.maybe_qpath(qself.as_deref(), path);
                self.w(" {");
                if fields.is_empty() && *rest == PatFieldsRest::None {
                    self.w("}");
                    return;
                }
                self.w(" ");
                self.comma_sep(fields, |p, f| {
                    if f.is_shorthand {
                        p.pat(&f.pat);
                    } else {
                        p.ident(f.ident);
                        p.w(": ");
                        p.pat(&f.pat);
                    }
                });
                if *rest == PatFieldsRest::Rest {
                    if !fields.is_empty() {
                        self.w(", ");
                    }
                    self.w("..");
                }
                self.w(" }");
            }
            PatKind::TupleStruct(qself, path, pats) => {
                self.maybe_qpath(qself.as_deref(), path);
                self.w("(");
                self.comma_sep(pats, |p, pat| p.pat(pat));
                self.w(")");
            }
            PatKind::Or(pats) => {
                for (i, p) in pats.iter().enumerate() {
                    if i > 0 {
                        self.w(" | ");
                    }
                    self.pat(p);
                }
            }
            PatKind::Path(qself, path) => self.maybe_qpath(qself.as_deref(), path),
            PatKind::Tuple(pats) => {
                self.w("(");
                self.comma_sep(pats, |p, pat| p.pat(pat));
                if pats.len() == 1 && !matches!(pats[0].kind, PatKind::Rest) {
                    self.w(",");
                }
                self.w(")");
            }
            PatKind::Ref(inner, m) => {
                self.w("&");
                if m.is_mut() {
                    self.w("mut ");
                }
                self.pat(inner);
            }
            PatKind::Expr(e) => self.expr(e),
            PatKind::Range(lo, hi, end) => {
                if let Some(lo) = lo {
                    self.expr(lo);
                }
                self.w(match end {
                    RangeEnd::Included => "..=",
                    RangeEnd::Excluded => "..",
                });
                if let Some(hi) = hi {
                    self.expr(hi);
                }
            }
            PatKind::Slice(pats) => {
                self.w("[");
                self.comma_sep(pats, |p, pat| p.pat(pat));
                self.w("]");
            }
            PatKind::Rest => self.w(".."),
            PatKind::Paren(inner) => {
                self.w("(");
                self.pat(inner);
                self.w(")");
            }
            PatKind::Err => self.w("/*error*/"),
        }
    }

    pub(super) fn binding_mode(&mut self, mode: BindingMode) {
        match mode.0 {
            ByRef::Yes(m) => self.w(if m.is_mut() { "ref mut " } else { "ref " }),
            ByRef::No if mode.1.is_mut() => self.w("var "),
            ByRef::No => {}
        }
    }

    pub(super) fn maybe_qpath(&mut self, qself: Option<&QSelf>, path: &Path) {
        match qself {
            Some(q) => self.qpath(q, path),
            None => self.path(path, true),
        }
    }

}
