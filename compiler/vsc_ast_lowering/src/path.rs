//! Lowering paths (rustc's `path.rs`), and exception 7's first half.
//!
//! Name resolution resolves the longest prefix of `a.b.c` that names items
//! and records how many trailing segments it didn't cover. Here those
//! segments get their meaning:
//!
//! - After a type (or `<T>`), the first one is an associated item: a
//!   type-relative path, as rustc's `T::Assoc`. In a type, every further
//!   segment is one too (`T.Item.Output`).
//! - In an expression, the rest are fields, and a callee's last segment is
//!   a method. `expr.rs` builds those from the segments returned here.

use vsc_ast::{self as ast, P};
use vsc_errors::ErrCode;
use vsc_hir::def::{DefKind, PartialRes, Res};
use vsc_hir::{self as hir, HirRes};
use vsc_span::{Ident, Span};

use crate::LoweringContext;

/// Where a path is: in a type (or pattern), every unresolved segment is an
/// associated item; in an expression, only the first one after a type is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathMode {
    Type,
    Expr,
}

/// Does a resolution name a type (so the next segment is an associated
/// item), rather than a value (so the next segment is a field)?
fn is_type_like(res: Res) -> bool {
    match res {
        Res::Def(kind, _) => matches!(
            kind,
            DefKind::Struct
                | DefKind::Class
                | DefKind::Union
                | DefKind::Enum
                | DefKind::Variant
                | DefKind::Trait
                | DefKind::TyAlias
                | DefKind::ForeignTy
                | DefKind::TyParam
                | DefKind::AssocTy
                | DefKind::Mod
        ),
        Res::PrimTy(_) | Res::BuiltinTy(_) | Res::SelfTyParam { .. } | Res::SelfTyAlias { .. } => true,
        Res::SelfCtor(_) | Res::Local(_) | Res::Builtin(_) | Res::Err => false,
    }
}

pub(crate) fn span_of(segments: &[ast::PathSegment]) -> Span {
    let first = segments[0].ident.span;
    let last = segments.last().unwrap();
    let hi = last.args.as_ref().map_or(last.ident.span, |a| a.span());
    first.to(hi)
}

impl LoweringContext<'_> {
    pub(crate) fn lower_path_segment(&mut self, seg: &ast::PathSegment, res: HirRes) -> hir::PathSegment {
        hir::PathSegment {
            ident: seg.ident,
            hir_id: self.lower_node_id(seg.id),
            res,
            args: seg.args.as_ref().map(|a| Box::new(self.lower_generic_args(a))),
        }
    }

    /// A resolved path: the last segment carries the resolution, the others
    /// don't need one.
    pub(crate) fn lower_path_with_res(&mut self, segments: &[ast::PathSegment], res: HirRes, span: Span) -> hir::Path {
        let n = segments.len();
        let segments = segments
            .iter()
            .enumerate()
            .map(|(i, s)| self.lower_path_segment(s, if i + 1 == n { res } else { Res::Err }))
            .collect();
        hir::Path { span, res, segments }
    }

    pub(crate) fn partial_res(&self, id: ast::NodeId) -> PartialRes {
        self.resolver.partial_res_map.get(&id).copied().unwrap_or(PartialRes::new(Res::Err))
    }

    /// A path in a type or pattern: every segment is consumed. `expected`
    /// names what the path must be, for the error when it names a value.
    pub(crate) fn lower_qpath_ty(
        &mut self,
        id: ast::NodeId,
        qself: &Option<P<ast::QSelf>>,
        path: &ast::Path,
        expected: &str,
    ) -> hir::QPath {
        let (qpath, consumed) = self.lower_qpath(id, qself, path, PathMode::Type);
        if consumed < path.segments.len() {
            // A value followed by more segments, where a type is needed.
            let shown = path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join(".");
            let res = self.partial_res(id).base_res;
            self.dcx()
                .struct_span_err(path.span, format!("expected {expected}, found {} `{shown}`", res.descr()))
                .with_code(ErrCode(573))
                .with_span_label(path.span, format!("not a {expected}"))
                .emit();
            let err = self.lower_path_with_res(&path.segments, Res::Err, path.span);
            return hir::QPath::Resolved(None, err);
        }
        qpath
    }

    /// Lowers the part of a path that names items, returning it and how many
    /// segments it used. In `PathMode::Type`, a path that resolved to a type
    /// uses every segment; a path that resolved to a value may not.
    pub(crate) fn lower_qpath(
        &mut self,
        id: ast::NodeId,
        qself: &Option<P<ast::QSelf>>,
        path: &ast::Path,
        mode: PathMode,
    ) -> (hir::QPath, usize) {
        let segs = &path.segments;
        let n = segs.len();
        let pr = self.partial_res(id);

        if let Some(q) = qself {
            let qself_ty = self.lower_ty(&q.ty);
            if q.position == 0 {
                // `<T>.Assoc…`
                return self.type_relative_chain(qself_ty, segs, 0, mode);
            }
            // `<T as Trait>.Assoc…`: find the associated item in the trait.
            let trait_res = pr.base_res;
            let assoc = &segs[q.position];
            let res = match trait_res {
                Res::Def(DefKind::Trait, def) => match def.as_local().and_then(|d| self.assoc_item_of(d, assoc.ident.name)) {
                    Some((kind, item)) => Res::Def(kind, item.to_def_id()),
                    None => {
                        let trait_name = segs[q.position - 1].ident;
                        self.dcx()
                            .struct_span_err(assoc.ident.span, format!("cannot find associated item `{}` in trait `{trait_name}`", assoc.ident))
                            .with_code(ErrCode(576))
                            .with_span_label(assoc.ident.span, format!("not found in `{trait_name}`"))
                            .emit();
                        Res::Err
                    }
                },
                _ => Res::Err,
            };
            let res = self.lower_res(res);
            let trait_res = self.lower_res(trait_res);
            let mut hir_segs: Vec<hir::PathSegment> = Vec::new();
            for (i, s) in segs[..=q.position].iter().enumerate() {
                let r = if i + 1 == q.position {
                    trait_res
                } else if i == q.position {
                    res
                } else {
                    Res::Err
                };
                hir_segs.push(self.lower_path_segment(s, r));
            }
            let span = span_of(&segs[..=q.position]);
            let qpath = hir::QPath::Resolved(Some(Box::new(qself_ty)), hir::Path { span, res, segments: hir_segs });
            return self.continue_type_relative(qpath, segs, q.position + 1, mode);
        }

        let unresolved = pr.unresolved_segments.min(n);
        let resolved = n - unresolved;
        if unresolved == 0 || pr.base_res == Res::Err || resolved == 0 {
            let res = if unresolved == 0 { pr.base_res } else { Res::Err };
            let res = self.lower_res(res);
            return (hir::QPath::Resolved(None, self.lower_path_with_res(segs, res, path.span)), n);
        }

        let res = self.lower_res(pr.base_res);
        let prefix_span = span_of(&segs[..resolved]);
        let prefix = self.lower_path_with_res(&segs[..resolved], res, prefix_span);
        let qpath = hir::QPath::Resolved(None, prefix);
        if is_type_like(pr.base_res) {
            let ty = hir::Ty { hir_id: self.next_id(), kind: hir::TyKind::Path(qpath), span: prefix_span };
            self.type_relative_chain(ty, segs, resolved, mode)
        } else {
            // A value: the rest are fields and methods.
            (qpath, resolved)
        }
    }

    /// `Ty.seg[start]`, then, in a type, `.seg` for each further segment.
    fn type_relative_chain(
        &mut self,
        ty: hir::Ty,
        segs: &[ast::PathSegment],
        start: usize,
        mode: PathMode,
    ) -> (hir::QPath, usize) {
        let seg = self.lower_path_segment(&segs[start], Res::Err);
        let qpath = hir::QPath::TypeRelative(Box::new(ty), seg);
        self.continue_type_relative(qpath, segs, start + 1, mode)
    }

    fn continue_type_relative(
        &mut self,
        mut qpath: hir::QPath,
        segs: &[ast::PathSegment],
        mut next: usize,
        mode: PathMode,
    ) -> (hir::QPath, usize) {
        if mode == PathMode::Type {
            while next < segs.len() {
                let span = qpath.span();
                let ty = hir::Ty { hir_id: self.next_id(), kind: hir::TyKind::Path(qpath), span };
                let seg = self.lower_path_segment(&segs[next], Res::Err);
                qpath = hir::QPath::TypeRelative(Box::new(ty), seg);
                next += 1;
            }
        }
        (qpath, next)
    }

    /// A type naming a resolution directly, such as a generic parameter in a
    /// predicate made from its inline bounds.
    pub(crate) fn ty_path_to(&mut self, res: Res, ident: Ident) -> hir::Ty {
        let res = self.lower_res(res);
        let seg = hir::PathSegment { ident, hir_id: self.next_id(), res, args: None };
        let path = hir::Path { span: ident.span, res, segments: vec![seg] };
        hir::Ty { hir_id: self.next_id(), kind: hir::TyKind::Path(hir::QPath::Resolved(None, path)), span: ident.span }
    }
}
