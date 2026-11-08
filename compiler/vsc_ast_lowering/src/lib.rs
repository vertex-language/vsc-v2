//! Lowering the AST to HIR (rustc's `rustc_ast_lowering`).
//!
//! Lowering reads the expanded AST and what name resolution found, and builds
//! HIR, where:
//!
//! - every node has a `HirId` (its owner and a number within it);
//! - every path carries its resolution, with locals named by `HirId`;
//! - exception 7 is gone: the trailing segments of `a.b.c` that name fields
//!   and methods become field and method-call expressions (`expr.rs`), and
//!   the first segment after a type becomes a type-relative path;
//! - surface forms are desugared as rustc desugars them: `for` and `while`
//!   into `loop` and `match`, `?` into `Try::branch`, ranges into the range
//!   lang items, `vec[…]` and the format built-ins into lang item calls
//!   (`format.rs`), destructuring assignment into `let`.
//!
//! The files follow rustc's: `item.rs`, `expr.rs`, `pat.rs`, `block.rs`,
//! `path.rs`, `format.rs`, `asm.rs`, and this file for types, generics and
//! the bookkeeping of owners and ids.

mod asm;
mod block;
mod expr;
mod format;
mod item;
mod pat;
mod path;

#[cfg(test)]
mod tests;

use std::mem;

use vsc_ast::{self as ast, DUMMY_NODE_ID, NodeId};
use vsc_data_structures::FxHashMap;
use vsc_errors::{DiagCtxt, ErrCode};
use vsc_hir::def::{DefKind, OriginRes, Res};
use vsc_hir::{self as hir, HirId, HirRes, ItemLocalId};
use vsc_index::{Idx, IndexVec};
use vsc_resolve::ResolverOutputs;
use vsc_session::ParseSess;
use vsc_span::def_id::{CRATE_DEF_ID, LocalDefId};
use vsc_span::{Ident, Span, Symbol, kw};

/// Lowers the whole crate. Errors are reported to the session; the HIR
/// still covers every item, with `Err` nodes where something was wrong.
pub fn lower_to_hir(psess: &ParseSess, krate: &ast::Crate, resolver: &ResolverOutputs) -> hir::Crate {
    let owners = IndexVec::from_raw((0..resolver.defs.len()).map(|_| None).collect());
    let mut lcx = LoweringContext {
        psess,
        resolver,
        owners,
        current_hir_id_owner: CRATE_DEF_ID,
        node_id_to_local_id: FxHashMap::default(),
        item_local_id_counter: ItemLocalId::new(1),
        bodies: FxHashMap::default(),
        attrs: FxHashMap::default(),
        loop_scope: None,
        self_res: None,
        pat_var: false,
        assoc_children: None,
    };
    item::lower_crate(&mut lcx, krate);
    hir::Crate { owners: lcx.owners }
}

pub(crate) struct LoweringContext<'a> {
    pub(crate) psess: &'a ParseSess,
    pub(crate) resolver: &'a ResolverOutputs,
    owners: IndexVec<LocalDefId, Option<hir::OwnerInfo>>,

    // The owner being lowered. Nested items save and restore these.
    current_hir_id_owner: LocalDefId,
    node_id_to_local_id: FxHashMap<NodeId, ItemLocalId>,
    item_local_id_counter: ItemLocalId,
    bodies: FxHashMap<ItemLocalId, hir::Body>,
    attrs: FxHashMap<ItemLocalId, Vec<ast::Attribute>>,

    /// The innermost loop, the target of an unlabeled `break` or `continue`.
    /// Function literals, `async` blocks and constants start a new scope.
    pub(crate) loop_scope: Option<HirId>,
    /// What `Self` means here, for `self` parameters written without a type.
    pub(crate) self_res: Option<HirRes>,
    /// Lowering the pattern of a `var` declaration: every binding is mutable
    /// (exception 5).
    pub(crate) pat_var: bool,
    /// (parent, name) → definition, for associated items named through a
    /// qualified path. Built on first use.
    assoc_children: Option<FxHashMap<(LocalDefId, Symbol), LocalDefId>>,
}

impl<'a> LoweringContext<'a> {
    pub(crate) fn dcx(&self) -> &'a DiagCtxt {
        &self.psess.dcx
    }

    // -----------------------------------------------------------------------
    // Owners and ids
    // -----------------------------------------------------------------------

    /// Lowers one owner (an item, associated item or foreign item, or the
    /// crate root). Its nodes are numbered from 1; the owner itself is 0.
    pub(crate) fn with_hir_id_owner(
        &mut self,
        owner: LocalDefId,
        owner_node: NodeId,
        f: impl FnOnce(&mut Self) -> hir::OwnerNode,
    ) {
        let saved_owner = mem::replace(&mut self.current_hir_id_owner, owner);
        let saved_ids = mem::take(&mut self.node_id_to_local_id);
        let saved_counter = mem::replace(&mut self.item_local_id_counter, ItemLocalId::new(1));
        let saved_bodies = mem::take(&mut self.bodies);
        let saved_attrs = mem::take(&mut self.attrs);
        let saved_loop = self.loop_scope.take();

        self.node_id_to_local_id.insert(owner_node, ItemLocalId::ZERO);
        let node = f(self);
        let info = hir::OwnerInfo { node, bodies: mem::take(&mut self.bodies), attrs: mem::take(&mut self.attrs) };
        debug_assert!(self.owners[owner].is_none(), "{owner:?} lowered twice");
        self.owners[owner] = Some(info);

        self.current_hir_id_owner = saved_owner;
        self.node_id_to_local_id = saved_ids;
        self.item_local_id_counter = saved_counter;
        self.bodies = saved_bodies;
        self.attrs = saved_attrs;
        self.loop_scope = saved_loop;
    }

    /// A fresh id, for a node the AST doesn't have (desugarings).
    pub(crate) fn next_id(&mut self) -> HirId {
        let local_id = self.item_local_id_counter;
        self.item_local_id_counter = self.item_local_id_counter.plus(1);
        HirId { owner: self.current_hir_id_owner, local_id }
    }

    /// The HIR id for an AST node, assigned on first use. Uses can be lowered
    /// before their definition (a `break` before its loop is finished), so
    /// the id is made whichever comes first.
    pub(crate) fn lower_node_id(&mut self, id: NodeId) -> HirId {
        if id == DUMMY_NODE_ID {
            return self.next_id();
        }
        if let Some(&local_id) = self.node_id_to_local_id.get(&id) {
            return HirId { owner: self.current_hir_id_owner, local_id };
        }
        let hir_id = self.next_id();
        self.node_id_to_local_id.insert(id, hir_id.local_id);
        hir_id
    }

    pub(crate) fn local_def_id(&self, id: NodeId) -> LocalDefId {
        *self.resolver.node_id_to_def_id.get(&id).unwrap_or_else(|| panic!("no definition for {id:?}"))
    }

    pub(crate) fn lower_attrs(&mut self, id: HirId, attrs: &[ast::Attribute]) {
        if !attrs.is_empty() {
            debug_assert_eq!(id.owner, self.current_hir_id_owner);
            self.attrs.entry(id.local_id).or_default().extend(attrs.iter().cloned());
        }
    }

    pub(crate) fn record_body(&mut self, params: Vec<hir::Param>, value: hir::Expr) -> hir::BodyId {
        let id = hir::BodyId { hir_id: value.hir_id };
        self.bodies.insert(value.hir_id.local_id, hir::Body { params, value });
        id
    }

    /// Runs `f` outside every loop: for function literals, `async` blocks
    /// and constants, which `break` can't leave.
    pub(crate) fn with_new_scopes<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.loop_scope.take();
        let r = f(self);
        self.loop_scope = saved;
        r
    }

    pub(crate) fn with_loop_scope<T>(&mut self, loop_id: HirId, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.loop_scope.replace(loop_id);
        let r = f(self);
        self.loop_scope = saved;
        r
    }

    // -----------------------------------------------------------------------
    // Resolutions
    // -----------------------------------------------------------------------

    pub(crate) fn lower_res(&mut self, res: Res) -> HirRes {
        res.map_id(|id| self.lower_node_id(id))
    }

    /// An associated item of a local trait, by name.
    pub(crate) fn assoc_item_of(&mut self, parent: LocalDefId, name: Symbol) -> Option<(DefKind, LocalDefId)> {
        let defs = &self.resolver.defs;
        let map = self.assoc_children.get_or_insert_with(|| {
            let mut m = FxHashMap::default();
            for (id, d) in defs.iter_enumerated() {
                if let (Some(p), Some(n)) = (d.parent, d.name) {
                    if matches!(d.kind, DefKind::AssocFn | DefKind::AssocConst | DefKind::AssocTy) {
                        m.insert((p, n), id);
                    }
                }
            }
            m
        });
        let def = *map.get(&(parent, name))?;
        Some((defs[def].kind, def))
    }

    // -----------------------------------------------------------------------
    // Origins (exception 13)
    // -----------------------------------------------------------------------

    pub(crate) fn lower_origin_ref(&mut self, o: Option<&ast::OriginRef>, elided_span: Span) -> hir::Origin {
        match o {
            None => hir::Origin { hir_id: self.next_id(), span: elided_span, kind: hir::OriginKind::Elided },
            Some(o) => {
                let names = o.origins.iter().map(|&ident| (ident, self.origin_res(ident))).collect();
                hir::Origin { hir_id: self.next_id(), span: o.span, kind: hir::OriginKind::Named(names) }
            }
        }
    }

    pub(crate) fn origin_res(&mut self, ident: Ident) -> OriginRes<HirId> {
        let res = self.resolver.origin_res_map.get(&ident.span).copied().unwrap_or(OriginRes::Err);
        res.map_id(|id| self.lower_node_id(id))
    }

    /// One named origin: a generic argument or bound that turned out to be
    /// an origin.
    pub(crate) fn origin_from_ident(&mut self, ident: Ident, span: Span) -> hir::Origin {
        let res = self.origin_res(ident);
        hir::Origin { hir_id: self.next_id(), span, kind: hir::OriginKind::Named(vec![(ident, res)]) }
    }

    /// A generic argument written as a type that name resolution found to be
    /// an origin: `src` in `Highlight<src>`, `[doc]` in `Lines<[doc]>`.
    fn generic_arg_origin(&self, ty: &ast::Ty) -> Option<Ident> {
        let ident = match &ty.kind {
            ast::TyKind::Path(None, p) => p.as_ident(),
            ast::TyKind::Slice(inner) => match &inner.kind {
                ast::TyKind::Path(None, p) => p.as_ident(),
                _ => None,
            },
            _ => None,
        }?;
        let is_origin = self.resolver.origin_res_map.contains_key(&ident.span)
            && !self.resolver.partial_res_map.contains_key(&ty.id)
            && !matches!(&ty.kind, ast::TyKind::Slice(inner) if self.resolver.partial_res_map.contains_key(&inner.id));
        is_origin.then_some(ident)
    }

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    pub(crate) fn lower_ty(&mut self, t: &ast::Ty) -> hir::Ty {
        use ast::TyKind as T;
        let kind = match &t.kind {
            T::Paren(inner) => return self.lower_ty(inner),
            T::Slice(inner) => hir::TyKind::Slice(Box::new(self.lower_ty(inner))),
            T::Array(inner, len) => hir::TyKind::Array(Box::new(self.lower_ty(inner)), self.lower_anon_const(len)),
            T::Ptr(mt) => hir::TyKind::Ptr(self.lower_mt(mt)),
            T::Ref(origin, mt) => {
                let origin = self.lower_origin_ref(origin.as_ref(), t.span.shrink_to_lo());
                hir::TyKind::Ref(origin, self.lower_mt(mt))
            }
            T::FnPtr(f) => {
                let generic_params = self.lower_generic_params(&f.generic_params);
                let param_idents = f
                    .decl
                    .inputs
                    .iter()
                    .filter(|p| !matches!(p.ty.kind, T::CVarArgs))
                    .map(|p| match p.pat.kind {
                        ast::PatKind::Ident(_, ident, None) if ident.name != kw::Underscore => Some(ident),
                        _ => None,
                    })
                    .collect();
                hir::TyKind::FnPtr(Box::new(hir::FnPtrTy {
                    safety: f.safety,
                    abi: lower_abi(f.ext),
                    generic_params,
                    decl: self.lower_fn_decl(&f.decl, false),
                    param_idents,
                }))
            }
            T::FuncValue(f) => {
                let generic_params = self.lower_generic_params(&f.generic_params);
                hir::TyKind::FuncValue(Box::new(hir::FuncValueTy { generic_params, decl: self.lower_fn_decl(&f.decl, false) }))
            }
            T::Never => hir::TyKind::Never,
            T::Tup(tys) => hir::TyKind::Tup(tys.iter().map(|t| self.lower_ty(t)).collect()),
            T::Path(qself, path) => hir::TyKind::Path(self.lower_qpath_ty(t.id, qself, path, "type")),
            T::TraitObject(bounds) => {
                let mut origin = None;
                let mut traits = Vec::new();
                for b in bounds {
                    match b {
                        ast::GenericBound::Trait(p) => traits.push(self.lower_poly_trait_ref(p)),
                        ast::GenericBound::Outlives(o) if origin.is_none() => {
                            origin = Some(self.lower_origin_ref(Some(o), o.span))
                        }
                        ast::GenericBound::Outlives(o) => {
                            self.dcx()
                                .struct_span_err(o.span, "only a single explicit origin bound is permitted")
                                .with_code(ErrCode(226))
                                .emit();
                        }
                        ast::GenericBound::Use(_, span) => {
                            self.dcx().struct_span_err(*span, "`use<…>` is only allowed on `impl` types").emit();
                        }
                    }
                }
                hir::TyKind::TraitObject(traits, origin)
            }
            T::ImplTrait(_, bounds) => hir::TyKind::OpaqueDef(self.lower_bounds(bounds)),
            T::Infer => hir::TyKind::Infer,
            T::ImplicitSelf => match self.self_res {
                Some(res) => {
                    let seg = hir::PathSegment {
                        ident: Ident::new(kw::SelfUpper, t.span),
                        hir_id: self.next_id(),
                        res,
                        args: None,
                    };
                    hir::TyKind::Path(hir::QPath::Resolved(None, hir::Path { span: t.span, res, segments: vec![seg] }))
                }
                // `self` outside a trait or impl; the parser reported it.
                None => hir::TyKind::Err,
            },
            T::CVarArgs => {
                self.dcx().struct_span_err(t.span, "only foreign functions can be C-variadic").with_code(ErrCode(45)).emit();
                hir::TyKind::Err
            }
            T::Err => hir::TyKind::Err,
        };
        hir::Ty { hir_id: self.lower_node_id(t.id), kind, span: t.span }
    }

    fn lower_mt(&mut self, mt: &ast::MutTy) -> hir::MutTy {
        hir::MutTy { ty: Box::new(self.lower_ty(&mt.ty)), mutbl: mt.mutbl }
    }

    pub(crate) fn ty_err(&mut self, span: Span) -> hir::Ty {
        hir::Ty { hir_id: self.next_id(), kind: hir::TyKind::Err, span }
    }

    pub(crate) fn lower_anon_const(&mut self, c: &ast::AnonConst) -> hir::AnonConst {
        let def_id = self.local_def_id(c.id);
        let hir_id = self.lower_node_id(c.id);
        let body = self.with_new_scopes(|this| {
            let value = this.lower_expr(&c.value);
            this.record_body(Vec::new(), value)
        });
        hir::AnonConst { hir_id, def_id, body, span: c.value.span }
    }

    // -----------------------------------------------------------------------
    // Function signatures
    // -----------------------------------------------------------------------

    /// The declaration of a function, method, function literal or function
    /// type. `has_body_params` is false for function types, whose parameter
    /// names aren't bindings.
    pub(crate) fn lower_fn_decl(&mut self, decl: &ast::FnDecl, has_body_params: bool) -> hir::FnDecl {
        let c_variadic = decl.c_variadic();
        let mut inputs = Vec::with_capacity(decl.inputs.len());
        let mut param_bindings = Vec::with_capacity(decl.inputs.len());
        for p in &decl.inputs {
            if matches!(p.ty.kind, ast::TyKind::CVarArgs) {
                continue;
            }
            let binding = match p.pat.kind {
                ast::PatKind::Ident(_, _, None) if has_body_params => Some(self.lower_node_id(p.pat.id)),
                _ => None,
            };
            param_bindings.push(binding);
            inputs.push(self.lower_ty(&p.ty));
        }
        let output = match &decl.output {
            ast::FnRetTy::Default(span) => hir::FnRetTy::DefaultReturn(*span),
            ast::FnRetTy::Ty(t) => hir::FnRetTy::Return(self.lower_ty(t)),
        };
        let implicit_self = match decl.inputs.first() {
            Some(p) if p.flat_receiver => hir::ImplicitSelfKind::Explicit,
            Some(p) if p.is_self() => {
                let mutbl = match p.pat.kind {
                    ast::PatKind::Ident(ast::BindingMode(_, m), ..) => m,
                    _ => ast::Mutability::Not,
                };
                match &p.ty.kind {
                    ast::TyKind::ImplicitSelf if mutbl.is_mut() => hir::ImplicitSelfKind::Mut,
                    ast::TyKind::ImplicitSelf => hir::ImplicitSelfKind::Imm,
                    ast::TyKind::Ref(_, mt) if matches!(mt.ty.kind, ast::TyKind::ImplicitSelf) => {
                        if mt.mutbl.is_mut() { hir::ImplicitSelfKind::RefMut } else { hir::ImplicitSelfKind::RefImm }
                    }
                    _ => hir::ImplicitSelfKind::Explicit,
                }
            }
            _ => hir::ImplicitSelfKind::None,
        };
        hir::FnDecl { inputs, param_bindings, output, c_variadic, implicit_self }
    }

    pub(crate) fn lower_fn_header(&self, h: &ast::FnHeader) -> hir::FnHeader {
        hir::FnHeader {
            safety: h.safety,
            is_async: h.asyncness.is_some(),
            is_const: matches!(h.constness, ast::Const::Yes(_)),
            abi: lower_abi(h.ext),
        }
    }

    /// A function's parameters and body. A missing body (already reported by
    /// the parser) becomes an error expression.
    pub(crate) fn lower_fn_body(&mut self, decl: &ast::FnDecl, body: Option<&ast::Block>, span: Span) -> hir::BodyId {
        self.with_new_scopes(|this| {
            let params = decl
                .inputs
                .iter()
                .filter(|p| !matches!(p.ty.kind, ast::TyKind::CVarArgs))
                .map(|p| this.lower_param(p))
                .collect();
            let value = match body {
                Some(b) => this.lower_block_expr(b),
                None => this.expr_err(span),
            };
            this.record_body(params, value)
        })
    }

    pub(crate) fn lower_param(&mut self, p: &ast::Param) -> hir::Param {
        let hir_id = self.lower_node_id(p.id);
        self.lower_attrs(hir_id, &p.attrs);
        hir::Param { hir_id, pat: self.lower_pat(&p.pat), ty_span: p.ty.span, span: p.span }
    }

    // -----------------------------------------------------------------------
    // Generics and bounds
    // -----------------------------------------------------------------------

    /// Generics, with bounds written on the parameters moved into the where
    /// clause, as rustc does. `extra` are a flat receiver's introduced
    /// parameters (exception 8).
    pub(crate) fn lower_generics(&mut self, g: &ast::Generics, extra: &[LocalDefId]) -> hir::Generics {
        let mut params = self.lower_generic_params(&g.params);
        for &def in extra {
            let data = &self.resolver.defs[def];
            let (name, span, kind) = (data.name.unwrap_or(kw::Empty), data.span, data.kind);
            let kind = match kind {
                DefKind::OriginParam => hir::GenericParamKind::Origin,
                DefKind::ConstParam => hir::GenericParamKind::Const { ty: self.ty_err(span), default: None },
                _ => hir::GenericParamKind::Type { default: None },
            };
            params.push(hir::GenericParam {
                hir_id: self.next_id(),
                def_id: def,
                name: Ident::new(name, span),
                kind,
                span,
                from_flat_receiver: true,
            });
        }

        let mut predicates = Vec::new();
        for p in &g.params {
            if p.bounds.is_empty() {
                continue;
            }
            let def = self.local_def_id(p.id);
            match p.kind {
                ast::GenericParamKind::Origin => {
                    let origin = hir::Origin {
                        hir_id: self.next_id(),
                        span: p.ident.span,
                        kind: hir::OriginKind::Named(vec![(p.ident, OriginRes::Param(def.to_def_id()))]),
                    };
                    let bounds = self.lower_outlives_bounds(&p.bounds);
                    predicates.push(hir::WherePredicate::Origin { span: p.span, origin, bounds });
                }
                _ => {
                    let res = Res::Def(self.resolver.defs[def].kind, def.to_def_id());
                    let bounded_ty = self.ty_path_to(res, p.ident);
                    predicates.push(hir::WherePredicate::Bound {
                        hir_id: self.next_id(),
                        span: p.span,
                        bound_generic_params: Vec::new(),
                        bounded_ty,
                        bounds: self.lower_bounds(&p.bounds),
                    });
                }
            }
        }
        for pred in &g.where_clause.predicates {
            predicates.push(self.lower_where_predicate(pred));
        }
        hir::Generics { params, predicates, span: g.span }
    }

    pub(crate) fn lower_generic_params(&mut self, params: &[ast::GenericParam]) -> Vec<hir::GenericParam> {
        params.iter().map(|p| self.lower_generic_param(p)).collect()
    }

    fn lower_generic_param(&mut self, p: &ast::GenericParam) -> hir::GenericParam {
        let hir_id = self.lower_node_id(p.id);
        self.lower_attrs(hir_id, &p.attrs);
        let kind = match &p.kind {
            ast::GenericParamKind::Origin => hir::GenericParamKind::Origin,
            ast::GenericParamKind::Type { default } => {
                hir::GenericParamKind::Type { default: default.as_ref().map(|t| self.lower_ty(t)) }
            }
            ast::GenericParamKind::Const { ty, default, .. } => hir::GenericParamKind::Const {
                ty: self.lower_ty(ty),
                default: default.as_ref().map(|d| self.lower_anon_const(d)),
            },
        };
        hir::GenericParam {
            hir_id,
            def_id: self.local_def_id(p.id),
            name: p.ident,
            kind,
            span: p.span,
            from_flat_receiver: false,
        }
    }

    fn lower_where_predicate(&mut self, pred: &ast::WherePredicate) -> hir::WherePredicate {
        let ast::WherePredicateKind::BoundPredicate { bound_generic_params, bounded_ty, bounds } = &pred.kind;
        // `a: [b]`: name resolution found `a` to be an origin.
        if let ast::TyKind::Path(None, p) = &bounded_ty.kind {
            if let Some(ident) = p.as_ident() {
                if self.resolver.origin_res_map.contains_key(&ident.span)
                    && !self.resolver.partial_res_map.contains_key(&bounded_ty.id)
                {
                    let origin = self.origin_from_ident(ident, ident.span);
                    let bounds = self.lower_outlives_bounds(bounds);
                    return hir::WherePredicate::Origin { span: pred.span, origin, bounds };
                }
            }
        }
        hir::WherePredicate::Bound {
            hir_id: self.lower_node_id(pred.id),
            span: pred.span,
            bound_generic_params: self.lower_generic_params(bound_generic_params),
            bounded_ty: self.lower_ty(bounded_ty),
            bounds: self.lower_bounds(bounds),
        }
    }

    /// The bounds of an origin: only other origins (`a: [b] + [c]`).
    fn lower_outlives_bounds(&mut self, bounds: &[ast::GenericBound]) -> Vec<hir::Origin> {
        let mut out = Vec::new();
        for b in bounds {
            match b {
                ast::GenericBound::Outlives(o) => {
                    for &ident in &o.origins {
                        out.push(self.origin_from_ident(ident, o.span));
                    }
                }
                other => {
                    self.dcx()
                        .struct_span_err(other.span(), "an origin can only be bounded by other origins")
                        .with_span_label(other.span(), "not an origin")
                        .emit();
                }
            }
        }
        out
    }

    pub(crate) fn lower_bounds(&mut self, bounds: &[ast::GenericBound]) -> hir::GenericBounds {
        bounds
            .iter()
            .map(|b| match b {
                ast::GenericBound::Trait(p) => hir::GenericBound::Trait(self.lower_poly_trait_ref(p)),
                ast::GenericBound::Outlives(o) => hir::GenericBound::Outlives(self.lower_origin_ref(Some(o), o.span)),
                ast::GenericBound::Use(args, span) => hir::GenericBound::Use(args.clone(), *span),
            })
            .collect()
    }

    pub(crate) fn lower_poly_trait_ref(&mut self, p: &ast::PolyTraitRef) -> hir::PolyTraitRef {
        hir::PolyTraitRef {
            bound_generic_params: self.lower_generic_params(&p.bound_generic_params),
            maybe: p.modifiers.maybe,
            trait_ref: self.lower_trait_ref(&p.trait_ref),
            span: p.span,
        }
    }

    pub(crate) fn lower_trait_ref(&mut self, t: &ast::TraitRef) -> hir::TraitRef {
        let res = match self.resolver.partial_res_map.get(&t.ref_id) {
            Some(pr) => match pr.full_res() {
                Some(res) => res,
                None => {
                    self.dcx()
                        .struct_span_err(t.path.span, "expected a trait, found an associated item")
                        .with_code(ErrCode(404))
                        .emit();
                    Res::Err
                }
            },
            None => Res::Err,
        };
        let res = self.lower_res(res);
        let path = self.lower_path_with_res(&t.path.segments, res, t.path.span);
        hir::TraitRef { path, hir_ref_id: self.lower_node_id(t.ref_id) }
    }

    // -----------------------------------------------------------------------
    // Generic arguments
    // -----------------------------------------------------------------------

    pub(crate) fn lower_generic_args(&mut self, args: &ast::GenericArgs) -> hir::GenericArgs {
        match args {
            ast::GenericArgs::AngleBracketed(a) => {
                let mut out = hir::GenericArgs { args: Vec::new(), constraints: Vec::new(), parenthesized: false, span: a.span };
                for arg in &a.args {
                    match arg {
                        ast::AngleBracketedArg::Arg(arg) => {
                            let arg = self.lower_generic_arg(arg);
                            out.args.push(arg);
                        }
                        ast::AngleBracketedArg::Constraint(c) => {
                            let c = self.lower_assoc_item_constraint(c);
                            out.constraints.push(c);
                        }
                    }
                }
                out
            }
            // `Fn(A, B) -> C` is `Fn<(A, B), Output = C>`, as in rustc.
            ast::GenericArgs::Parenthesized(p) => {
                let inputs = p.inputs.iter().map(|t| self.lower_ty(t)).collect();
                let tuple = hir::Ty { hir_id: self.next_id(), kind: hir::TyKind::Tup(inputs), span: p.inputs_span };
                let output = match &p.output {
                    ast::FnRetTy::Ty(t) => self.lower_ty(t),
                    ast::FnRetTy::Default(span) => hir::Ty { hir_id: self.next_id(), kind: hir::TyKind::Tup(Vec::new()), span: *span },
                };
                let output_span = output.span;
                let constraint = hir::AssocItemConstraint {
                    hir_id: self.next_id(),
                    ident: Ident::new(Symbol::intern("Output"), output_span),
                    kind: hir::AssocItemConstraintKind::Equality(output),
                    span: output_span,
                };
                hir::GenericArgs { args: vec![hir::GenericArg::Type(tuple)], constraints: vec![constraint], parenthesized: true, span: p.span }
            }
        }
    }

    fn lower_generic_arg(&mut self, arg: &ast::GenericArg) -> hir::GenericArg {
        match arg {
            ast::GenericArg::Origin(o) => hir::GenericArg::Origin(self.lower_origin_ref(Some(o), o.span)),
            ast::GenericArg::Type(t) => {
                if let Some(ident) = self.generic_arg_origin(t) {
                    return hir::GenericArg::Origin(self.origin_from_ident(ident, t.span));
                }
                // A name that resolved to a value is a const argument.
                if let ast::TyKind::Path(None, p) = &t.kind {
                    let is_value = self
                        .resolver
                        .partial_res_map
                        .get(&t.id)
                        .and_then(|pr| pr.full_res())
                        .is_some_and(|res| res.ns() == Some(vsc_hir::def::Namespace::ValueNS));
                    if is_value {
                        let (qpath, _) = self.lower_qpath(t.id, &None, p, crate::path::PathMode::Type);
                        let hir_id = self.lower_node_id(t.id);
                        return hir::GenericArg::Const(hir::ConstArg { hir_id, kind: hir::ConstArgKind::Path(qpath), span: t.span });
                    }
                }
                match t.kind {
                    ast::TyKind::Infer => hir::GenericArg::Infer(t.span),
                    _ => hir::GenericArg::Type(self.lower_ty(t)),
                }
            }
            ast::GenericArg::Const(c) => {
                let span = c.value.span;
                hir::GenericArg::Const(hir::ConstArg { hir_id: self.next_id(), kind: hir::ConstArgKind::Anon(self.lower_anon_const(c)), span })
            }
        }
    }

    fn lower_assoc_item_constraint(&mut self, c: &ast::AssocItemConstraint) -> hir::AssocItemConstraint {
        if let Some(args) = &c.gen_args {
            self.dcx()
                .struct_span_err(args.span(), "generic associated types aren't supported yet")
                .with_span_label(args.span(), "remove these arguments")
                .emit();
        }
        let kind = match &c.kind {
            ast::AssocItemConstraintKind::Equality { term } => hir::AssocItemConstraintKind::Equality(self.lower_ty(term)),
            ast::AssocItemConstraintKind::Bound { bounds } => hir::AssocItemConstraintKind::Bound(self.lower_bounds(bounds)),
        };
        hir::AssocItemConstraint { hir_id: self.lower_node_id(c.id), ident: c.ident, kind, span: c.span }
    }
}

/// The ABI named by `extern`: `None` for Rust's, `"C"` for a bare `extern`.
pub(crate) fn lower_abi(ext: ast::Extern) -> Option<Symbol> {
    match ext {
        ast::Extern::None => None,
        ast::Extern::Implicit(_) => Some(vsc_span::sym::C),
        ast::Extern::Explicit(s, _) => Some(s.symbol),
    }
}
