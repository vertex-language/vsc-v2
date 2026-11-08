//! Lowering patterns (rustc's `pat.rs`).

use vsc_ast::{self as ast, P};
use vsc_hir::def::Res;
use vsc_hir::{self as hir};

use crate::LoweringContext;

impl LoweringContext<'_> {
    /// The pattern of a `var` declaration: every binding is mutable.
    pub(crate) fn lower_pat_var(&mut self, p: &ast::Pat, is_var: bool) -> hir::Pat {
        let saved = std::mem::replace(&mut self.pat_var, is_var);
        let pat = self.lower_pat(p);
        self.pat_var = saved;
        pat
    }

    pub(crate) fn lower_pat(&mut self, p: &ast::Pat) -> hir::Pat {
        use ast::PatKind as K;
        let kind = match &p.kind {
            K::Paren(inner) => return self.lower_pat(inner),
            K::Wild => hir::PatKind::Wild,
            K::Ident(mode, ident, sub) => {
                // A name that resolved to a constant or a unit struct or
                // variant is a path pattern.
                match self.resolver.partial_res_map.get(&p.id).and_then(|pr| pr.full_res()) {
                    Some(res @ Res::Def(..)) | Some(res @ Res::SelfCtor(_)) => {
                        let res = self.lower_res(res);
                        let seg = ast::PathSegment::from_ident(*ident);
                        let path = self.lower_path_with_res(std::slice::from_ref(&seg), res, ident.span);
                        hir::PatKind::Path(hir::QPath::Resolved(None, path))
                    }
                    Some(Res::Err) => hir::PatKind::Err,
                    _ => {
                        let ast::BindingMode(by_ref, mut mutbl) = *mode;
                        if self.pat_var && by_ref == ast::ByRef::No {
                            mutbl = ast::Mutability::Mut;
                        }
                        let hir_id = self.lower_node_id(p.id);
                        let sub = sub.as_ref().map(|s| Box::new(self.lower_pat(s)));
                        return hir::Pat {
                            hir_id,
                            kind: hir::PatKind::Binding(hir::BindingMode(by_ref, mutbl), hir_id, *ident, sub),
                            span: p.span,
                        };
                    }
                }
            }
            K::Struct(qself, path, fields, rest) => {
                let qpath = self.lower_qpath_ty(p.id, qself, path, "struct, variant or union type");
                let fields = fields
                    .iter()
                    .map(|f| {
                        let hir_id = self.lower_node_id(f.id);
                        self.lower_attrs(hir_id, &f.attrs);
                        hir::PatField { hir_id, ident: f.ident, pat: self.lower_pat(&f.pat), is_shorthand: f.is_shorthand, span: f.span }
                    })
                    .collect();
                hir::PatKind::Struct(qpath, fields, *rest == ast::PatFieldsRest::Rest)
            }
            K::TupleStruct(qself, path, pats) => {
                let qpath = self.lower_qpath_ty(p.id, qself, path, "tuple struct or tuple variant");
                let (pats, rest) = self.lower_pat_tuple(pats, "tuple struct or tuple variant");
                hir::PatKind::TupleStruct(qpath, pats, rest)
            }
            K::Or(alts) => hir::PatKind::Or(alts.iter().map(|a| self.lower_pat(a)).collect()),
            K::Path(qself, path) => hir::PatKind::Path(self.lower_qpath_ty(p.id, qself, path, "unit struct, unit variant or constant")),
            K::Tuple(pats) => {
                let (pats, rest) = self.lower_pat_tuple(pats, "tuple");
                hir::PatKind::Tuple(pats, rest)
            }
            K::Ref(inner, m) => hir::PatKind::Ref(Box::new(self.lower_pat(inner)), *m),
            K::Expr(e) => hir::PatKind::Expr(Box::new(self.lower_expr(e))),
            K::Range(lo, hi, end) => hir::PatKind::Range(
                lo.as_ref().map(|e| Box::new(self.lower_expr(e))),
                hi.as_ref().map(|e| Box::new(self.lower_expr(e))),
                *end,
            ),
            K::Slice(pats) => self.lower_pat_slice(pats),
            K::Rest => {
                self.dcx()
                    .struct_span_err(p.span, "`..` patterns are not allowed here")
                    .with_note("only allowed in tuple, tuple struct, and slice patterns")
                    .emit();
                hir::PatKind::Err
            }
            K::Err => hir::PatKind::Err,
        };
        hir::Pat { hir_id: self.lower_node_id(p.id), kind, span: p.span }
    }

    /// The elements of a tuple or tuple struct pattern, and where `..` is.
    fn lower_pat_tuple(&mut self, pats: &[P<ast::Pat>], ctx: &str) -> (Vec<hir::Pat>, Option<usize>) {
        let mut out = Vec::with_capacity(pats.len());
        let mut rest: Option<(usize, vsc_span::Span)> = None;
        for p in pats {
            match &p.kind {
                ast::PatKind::Rest => match rest {
                    Some((_, prev)) => {
                        self.dcx()
                            .struct_span_err(p.span, format!("`..` can only be used once per {ctx} pattern"))
                            .with_span_label(p.span, "can only be used once per pattern")
                            .with_span_label(prev, "previously used here")
                            .emit();
                    }
                    None => rest = Some((out.len(), p.span)),
                },
                ast::PatKind::Ident(_, ident, Some(sub)) if matches!(sub.kind, ast::PatKind::Rest) => {
                    self.dcx()
                        .struct_span_err(p.span, format!("`{ident} @` is not allowed in a {ctx}"))
                        .with_span_label(p.span, "this is only allowed in slice patterns")
                        .with_help("remove this and bind each element separately")
                        .emit();
                    if rest.is_none() {
                        rest = Some((out.len(), p.span));
                    }
                }
                _ => out.push(self.lower_pat(p)),
            }
        }
        (out, rest.map(|(i, _)| i))
    }

    /// `[a, rest @ .., z]`: the elements before the rest, the rest, and the
    /// elements after.
    fn lower_pat_slice(&mut self, pats: &[P<ast::Pat>]) -> hir::PatKind {
        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut slice: Option<hir::Pat> = None;
        let mut prev_rest: Option<vsc_span::Span> = None;
        for p in pats {
            let rest_pat = match &p.kind {
                ast::PatKind::Rest => Some(hir::Pat { hir_id: self.lower_node_id(p.id), kind: hir::PatKind::Wild, span: p.span }),
                ast::PatKind::Ident(mode, ident, Some(sub)) if matches!(sub.kind, ast::PatKind::Rest) => {
                    let hir_id = self.lower_node_id(p.id);
                    let ast::BindingMode(by_ref, mut mutbl) = *mode;
                    if self.pat_var && by_ref == ast::ByRef::No {
                        mutbl = ast::Mutability::Mut;
                    }
                    Some(hir::Pat { hir_id, kind: hir::PatKind::Binding(hir::BindingMode(by_ref, mutbl), hir_id, *ident, None), span: p.span })
                }
                _ => None,
            };
            match rest_pat {
                Some(r) => {
                    if let Some(prev) = prev_rest {
                        self.dcx()
                            .struct_span_err(p.span, "`..` can only be used once per slice pattern")
                            .with_span_label(p.span, "can only be used once per slice pattern")
                            .with_span_label(prev, "previously used here")
                            .emit();
                        continue;
                    }
                    prev_rest = Some(p.span);
                    slice = Some(r);
                }
                None if slice.is_some() => after.push(self.lower_pat(p)),
                None => before.push(self.lower_pat(p)),
            }
        }
        hir::PatKind::Slice(before, slice.map(Box::new), after)
    }
}
