//! Lowering HIR types to semantic types (rustc's `hir_ty_lowering`).
//!
//! The Vertex exceptions decided here:
//!
//! - 2: `string` alone is the `String` lang item; `&string` is `&str` and
//!   `&mut string` is `&mut String`. `cstring` is `CString`, `&cstring` is
//!   `&CStr`. The C types (`c_int`, …) are the target's integer types.
//! - 3: naming a class gives the `Class` lang item around the class's struct.
//! - 13: lifetimes are Rust's, without the `'`. In a function signature,
//!   each elided input reference gets its own bound region, and an elided
//!   output reference follows Rust's elision rules.
//! - 14: `func(T) -> U` is the `Func` lang item applied to `fn(T) -> U`.

mod path;

use vsc_errors::ErrCode;
use vsc_hir::def::{DefKind, LifetimeRes, PrimTy, Res};
use vsc_hir::{self as hir, LangItem};
use vsc_middle::ty::*;
use vsc_span::def_id::DefId;
use vsc_span::{Ident, Span, Symbol, kw, sym};

/// What an elided lifetime means where it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElidedRegions {
    /// Not allowed: struct fields, type aliases, impl headers, bounds.
    Error,
    /// Constants and statics: `static`.
    Static,
    /// A function signature: see `FnRegions`.
    Fn,
    /// A type written in a body: elided lifetimes are erased (borrow checking
    /// infers them), `_` is a type to infer, and missing generic arguments
    /// are inferred.
    Body,
}

/// The regions of one binder being lowered: a function's signature, or a
/// function type inside it.
#[derive(Default)]
struct FnRegions<'tcx> {
    /// The binder's variables' names.
    vars: Vec<Symbol>,
    in_output: bool,
    /// The regions of the inputs that belong to this binder or are free,
    /// for output elision.
    input_regions: Vec<Region<'tcx>>,
    self_region: Option<Region<'tcx>>,
    reported_output_elision: bool,
}

pub struct ItemCtxt<'tcx> {
    pub tcx: TyCtxt<'tcx>,
    /// The item whose generic parameters are in scope.
    pub item_def: DefId,
    pub elided: ElidedRegions,
    /// The binders being lowered, innermost last.
    binders: Vec<FnRegions<'tcx>>,
    /// `_`s handed out in body mode.
    fresh: u32,
}

impl<'tcx> ItemCtxt<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, item_def: DefId, elided: ElidedRegions) -> Self {
        ItemCtxt { tcx, item_def, elided, binders: Vec::new(), fresh: 0 }
    }

    // -----------------------------------------------------------------------
    // Function signatures
    // -----------------------------------------------------------------------

    /// A function's signature, or a function type's (`header` is `None` and
    /// `safety`/`abi` come from the type).
    pub fn lower_fn_sig(&mut self, decl: &hir::FnDecl, header: Option<&hir::FnHeader>, foreign_abi: Option<Symbol>) -> PolyFnSig<'tcx> {
        let (safety, abi) = match header {
            Some(h) => (if matches!(h.safety, hir::Safety::Unsafe(_)) { Safety::Unsafe } else { Safety::Safe }, h.abi),
            None => (Safety::Safe, None),
        };
        // Foreign functions are unsafe to call unless marked `safe`, and use
        // their block's ABI.
        let (safety, abi) = match foreign_abi {
            Some(a) => (
                if header.is_some_and(|h| matches!(h.safety, hir::Safety::Safe(_))) { Safety::Safe } else { Safety::Unsafe },
                Some(a),
            ),
            None => (safety, abi),
        };
        self.lower_binder(decl, safety, abi)
    }

    /// Lowers a declaration inside a new binder.
    fn lower_binder(&mut self, decl: &hir::FnDecl, safety: Safety, abi: Option<Symbol>) -> PolyFnSig<'tcx> {
        self.binders.push(FnRegions::default());
        let mut io = Vec::with_capacity(decl.inputs.len() + 1);
        for (i, input) in decl.inputs.iter().enumerate() {
            let ty = self.lower_ty(input);
            let regions = binder_regions(ty);
            let fr = self.binders.last_mut().unwrap();
            if i == 0 && matches!(decl.implicit_self, hir::ImplicitSelfKind::RefImm | hir::ImplicitSelfKind::RefMut) {
                fr.self_region = regions.first().copied();
            }
            fr.input_regions.extend(regions);
            io.push(ty);
        }
        self.binders.last_mut().unwrap().in_output = true;
        let output = match &decl.output {
            hir::FnRetTy::DefaultReturn(_) => self.tcx.types.unit,
            hir::FnRetTy::Return(t) => self.lower_ty(t),
        };
        io.push(output);
        let fr = self.binders.pop().unwrap();
        let vars: Vec<BoundVar> = fr.vars.iter().map(|&name| BoundVar { name }).collect();
        Binder {
            value: FnSig { inputs_and_output: self.tcx.mk_type_list(&io), c_variadic: decl.c_variadic, safety, abi },
            bound_vars: self.tcx.mk_bound_vars(&vars),
        }
    }

    /// A new variable of the innermost binder.
    fn new_bound_region(&mut self, name: Symbol) -> Region<'tcx> {
        let fr = self.binders.last_mut().unwrap();
        let var = fr.vars.len() as u32;
        fr.vars.push(name);
        self.tcx.mk_region(RegionKind::ReBound(DebruijnIndex::INNERMOST, BoundRegion { var }))
    }

    // -----------------------------------------------------------------------
    // Regions
    // -----------------------------------------------------------------------

    pub fn lower_lifetime(&mut self, o: &hir::Lifetime) -> Region<'tcx> {
        match o.kind {
            hir::LifetimeKind::Elided => self.elided_region(o.span),
            hir::LifetimeKind::Named(ident, res) => self.named_region(ident, res),
        }
    }

    fn named_region(&mut self, ident: Ident, res: LifetimeRes) -> Region<'tcx> {
        match res {
            LifetimeRes::Static => self.tcx.regions.re_static,
            LifetimeRes::Err => self.tcx.regions.re_error,
            LifetimeRes::Param(def) => match self.param_index(def) {
                Some(index) => self.tcx.mk_early_region(index, ident.name),
                // A `for<a>` parameter: its binder isn't represented
                // yet, so the region is left for later passes.
                None => self.tcx.regions.re_erased,
            },
        }
    }

    fn elided_region(&mut self, span: Span) -> Region<'tcx> {
        match self.binders.last() {
            // Inside a function's or function type's inputs: a new region.
            Some(fr) if !fr.in_output => self.new_bound_region(kw::Empty),
            Some(_) => self.output_elision(span),
            None => match self.elided {
                ElidedRegions::Static => self.tcx.regions.re_static,
                ElidedRegions::Body => self.tcx.regions.re_erased,
                ElidedRegions::Fn | ElidedRegions::Error => {
                    self.tcx
                        .dcx()
                        .struct_span_err(span, "missing lifetime specifier")
                        .with_code(ErrCode(106))
                        .with_span_label(span, "expected named lifetime parameter")
                        .with_help("consider introducing a named lifetime parameter: `<a>`, then `&a T`")
                        .emit();
                    self.tcx.regions.re_error
                }
            },
        }
    }

    /// Rust's elision rules for an elided output reference: `&self`'s
    /// region, or the only region among the inputs.
    fn output_elision(&mut self, span: Span) -> Region<'tcx> {
        let fr = self.binders.last_mut().unwrap();
        if let Some(r) = fr.self_region {
            return r;
        }
        let mut distinct: Vec<Region<'tcx>> = Vec::new();
        for r in &fr.input_regions {
            if !distinct.contains(r) {
                distinct.push(*r);
            }
        }
        if distinct.len() == 1 {
            return distinct[0];
        }
        if !fr.reported_output_elision {
            fr.reported_output_elision = true;
            let mut err = self
                .tcx
                .dcx()
                .struct_span_err(span, "missing lifetime specifier")
                .with_code(ErrCode(106))
                .with_span_label(span, "expected named lifetime parameter");
            if distinct.is_empty() {
                err = err
                    .with_help("this function's return type contains a borrowed value, but there is no value for it to be borrowed from")
                    .with_help("consider using the `static` lifetime (`&static T`), or returning an owned value");
            } else {
                err = err
                    .with_help("this function's return type contains a borrowed value, but the signature does not say which one of the parameters it is borrowed from")
                    .with_help("consider introducing a named lifetime parameter: `func f<a>(x: &a T, y: &a T) -> &a T`");
            }
            err.emit();
        }
        self.tcx.regions.re_error
    }

    // -----------------------------------------------------------------------
    // Parameters
    // -----------------------------------------------------------------------

    /// A generic parameter's index, searching the item and its parents.
    pub fn param_index(&self, param: DefId) -> Option<u32> {
        let mut def = Some(self.item_def);
        while let Some(d) = def {
            let g = self.tcx.generics_of(d);
            if let Some(&i) = g.param_def_id_to_index.get(&param) {
                return Some(i);
            }
            def = g.parent;
        }
        None
    }

    fn param_ty(&self, param: DefId, span: Span) -> Ty<'tcx> {
        match self.param_index(param) {
            Some(i) => self.tcx.mk_param(i, self.tcx.item_name(param)),
            None => {
                self.tcx.dcx().struct_span_err(span, "can't use a generic parameter from an outer item").with_code(ErrCode(401)).emit();
                self.tcx.types.error
            }
        }
    }

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    pub fn lower_ty(&mut self, t: &hir::Ty) -> Ty<'tcx> {
        let tcx = self.tcx;
        match &t.kind {
            hir::TyKind::Slice(inner) => {
                let inner = self.lower_ty(inner);
                tcx.mk_ty(TyKind::Slice(inner))
            }
            hir::TyKind::Array(inner, len) => {
                let inner = self.lower_ty(inner);
                let len = self.lower_array_len(len);
                tcx.mk_ty(TyKind::Array(inner, len))
            }
            hir::TyKind::Ptr(mt) => {
                let inner = self.lower_ty(&mt.ty);
                tcx.mk_ty(TyKind::RawPtr(inner, mt.mutbl))
            }
            hir::TyKind::Ref(lifetime, mt) => {
                let r = self.lower_lifetime(lifetime);
                let inner = self.lower_ref_target(&mt.ty, mt.mutbl);
                tcx.mk_ref(r, inner, mt.mutbl)
            }
            hir::TyKind::FnPtr(f) => {
                let sig = self.lower_fn_ptr(&f.decl, if matches!(f.safety, hir::Safety::Unsafe(_)) { Safety::Unsafe } else { Safety::Safe }, f.abi);
                tcx.mk_ty(TyKind::FnPtr(sig))
            }
            hir::TyKind::FuncValue(f) => {
                // `func(T) -> U` is `Func<fn(T) -> U>` (exception 14).
                let sig = self.lower_fn_ptr(&f.decl, Safety::Safe, None);
                let fn_ptr = tcx.mk_ty(TyKind::FnPtr(sig));
                match tcx.require_lang_item(LangItem::Func, t.span) {
                    Some(def) => {
                        let adt = tcx.adt_def(def);
                        tcx.mk_adt(adt, tcx.mk_args(&[GenericArg::Type(fn_ptr)]))
                    }
                    None => tcx.types.error,
                }
            }
            hir::TyKind::Never => tcx.types.never,
            hir::TyKind::Tup(tys) => {
                let tys: Vec<_> = tys.iter().map(|t| self.lower_ty(t)).collect();
                tcx.mk_tup(&tys)
            }
            hir::TyKind::Path(q) => self.lower_qpath(q, t.span),
            hir::TyKind::TraitObject(traits, lifetime) => self.lower_trait_object(traits, lifetime.as_ref(), t.span),
            hir::TyKind::OpaqueDef(def, _) => {
                let def_id = def.to_def_id();
                let parent = tcx.opt_parent(def_id).unwrap_or(self.item_def);
                let args = tcx.identity_args(parent);
                tcx.mk_ty(TyKind::Opaque(def_id, args))
            }
            hir::TyKind::Infer if self.elided == ElidedRegions::Body => self.fresh_ty(),
            hir::TyKind::Infer => {
                tcx.dcx()
                    .struct_span_err(t.span, "the placeholder `_` is not allowed within types on item signatures")
                    .with_code(ErrCode(121))
                    .with_span_label(t.span, "not allowed in type signatures")
                    .emit();
                tcx.types.error
            }
            hir::TyKind::Err => tcx.types.error,
        }
    }

    /// The type behind `&`: `&string` is `&str`, `&mut string` is
    /// `&mut String`; `&cstring` is `&CStr` (exception 2).
    fn lower_ref_target(&mut self, t: &hir::Ty, mutbl: Mutability) -> Ty<'tcx> {
        if let hir::TyKind::Path(hir::QPath::Resolved(None, p)) = &t.kind {
            match p.res {
                Res::PrimTy(PrimTy::Str) if !mutbl.is_mut() => {
                    self.no_generic_args(p);
                    return self.tcx.types.str_;
                }
                Res::BuiltinTy(s) if s == sym::cstring && !mutbl.is_mut() => {
                    self.no_generic_args(p);
                    return self.lang_item_adt(LangItem::CStr, t.span);
                }
                _ => {}
            }
        }
        self.lower_ty(t)
    }

    fn lang_item_adt(&mut self, item: LangItem, span: Span) -> Ty<'tcx> {
        match self.tcx.require_lang_item(item, span) {
            Some(def) => {
                let adt = self.tcx.adt_def(def);
                let g = self.tcx.generics_of(def);
                // In a body (a desugared range, say), its arguments are
                // inferred.
                if g.count() != 0 && self.elided == ElidedRegions::Body {
                    let args: Vec<GenericArg<'tcx>> = (0..g.count())
                        .map(|i| match g.param_at(i, self.tcx).kind {
                            GenericParamDefKind::Lifetime => GenericArg::Region(self.tcx.regions.re_erased),
                            GenericParamDefKind::Type { .. } => GenericArg::Type(self.fresh_ty()),
                            GenericParamDefKind::Const { .. } => GenericArg::Const(self.tcx.mk_const(ConstKind::Error)),
                        })
                        .collect();
                    return self.tcx.mk_adt(adt, self.tcx.mk_args(&args));
                }
                if g.count() != 0 {
                    self.tcx
                        .dcx()
                        .struct_span_err(self.tcx.def_span(def), format!("the `{}` lang item must not be generic", item.name()))
                        .emit();
                    return self.tcx.types.error;
                }
                self.tcx.mk_adt(adt, &[])
            }
            None => self.tcx.types.error,
        }
    }

    /// An array's length: a literal, a const parameter, or an expression to
    /// evaluate later.
    fn fresh_ty(&mut self) -> Ty<'tcx> {
        self.fresh += 1;
        self.tcx.mk_ty(TyKind::Infer(InferTy::Fresh(self.fresh - 1)))
    }

    pub fn lower_array_len(&mut self, c: &hir::AnonConst) -> Const<'tcx> {
        let tcx = self.tcx;
        let body = tcx.hir_crate.body(c.body);
        let mut e = &body.value;
        while let hir::ExprKind::Block(b, None) = &e.kind {
            match (&b.stmts[..], &b.expr) {
                ([], Some(inner)) => e = inner,
                _ => break,
            }
        }
        match &e.kind {
            hir::ExprKind::Lit(lit) if lit.kind == vsc_ast::token::LitKind::Integer => {
                let text: String = lit.symbol.as_str().chars().filter(|&c| c != '_').collect();
                let value = if let Some(h) = text.strip_prefix("0x") {
                    u128::from_str_radix(h, 16).ok()
                } else if let Some(o) = text.strip_prefix("0o") {
                    u128::from_str_radix(o, 8).ok()
                } else if let Some(b) = text.strip_prefix("0b") {
                    u128::from_str_radix(b, 2).ok()
                } else {
                    text.parse().ok()
                };
                match value {
                    Some(v) => tcx.mk_const(ConstKind::Value(tcx.types.usize, v)),
                    None => tcx.mk_const(ConstKind::Error),
                }
            }
            hir::ExprKind::Path(hir::QPath::Resolved(None, p)) => match p.res {
                Res::Def(DefKind::ConstParam, def) => match self.param_index(def) {
                    Some(index) => tcx.mk_const(ConstKind::Param(ParamConst { index, name: tcx.item_name(def) })),
                    None => tcx.mk_const(ConstKind::Error),
                },
                _ => tcx.mk_const(ConstKind::Unevaluated(c.def_id.to_def_id(), &[])),
            },
            _ => tcx.mk_const(ConstKind::Unevaluated(c.def_id.to_def_id(), &[])),
        }
    }

    fn lower_const_arg(&mut self, c: &hir::ConstArg) -> Const<'tcx> {
        let tcx = self.tcx;
        match &c.kind {
            hir::ConstArgKind::Anon(a) => self.lower_array_len(a),
            hir::ConstArgKind::Path(hir::QPath::Resolved(None, p)) => match p.res {
                Res::Def(DefKind::ConstParam, def) => match self.param_index(def) {
                    Some(index) => tcx.mk_const(ConstKind::Param(ParamConst { index, name: tcx.item_name(def) })),
                    None => tcx.mk_const(ConstKind::Error),
                },
                Res::Def(DefKind::Const | DefKind::AssocConst, def) => tcx.mk_const(ConstKind::Unevaluated(def, &[])),
                _ => tcx.mk_const(ConstKind::Error),
            },
            hir::ConstArgKind::Path(_) => tcx.mk_const(ConstKind::Error),
        }
    }

    /// A function pointer or function value type: its own binder, inside
    /// whatever binders are outside it.
    fn lower_fn_ptr(&mut self, decl: &hir::FnDecl, safety: Safety, abi: Option<Symbol>) -> PolyFnSig<'tcx> {
        self.lower_binder(decl, safety, abi)
    }

    fn lower_trait_object(&mut self, traits: &[hir::PolyTraitRef], lifetime: Option<&hir::Lifetime>, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        // `Self` in a `dyn` type's trait arguments is erased, as in rustc.
        let dummy_self = tcx.types.error;
        let mut preds = Vec::new();
        for t in traits {
            let Some((tr, projections)) = self.lower_trait_ref(&t.trait_ref, dummy_self) else { continue };
            preds.push(ExistentialPredicate::Trait { def_id: tr.def_id, args: tcx.mk_args(&tr.args[1..]) });
            for (alias, term) in projections {
                preds.push(ExistentialPredicate::Projection { def_id: alias.def_id, args: tcx.mk_args(&alias.args[1..]), term });
            }
        }
        if preds.is_empty() {
            tcx.dcx().struct_span_err(span, "at least one trait is required for an object type").with_code(ErrCode(224)).emit();
            return tcx.types.error;
        }
        let r = match lifetime {
            Some(o) => self.lower_lifetime(o),
            None => match self.elided {
                ElidedRegions::Static => tcx.regions.re_static,
                _ => tcx.regions.re_erased,
            },
        };
        tcx.mk_ty(TyKind::Dynamic(tcx.mk_existentials(&preds), r))
    }

}

/// The regions of `ty` that belong to the binder it's directly in, or to
/// none: those of function types inside it are their own.
fn binder_regions<'tcx>(ty: Ty<'tcx>) -> Vec<Region<'tcx>> {
    fn walk<'tcx>(ty: Ty<'tcx>, depth: u32, out: &mut Vec<Region<'tcx>>) {
        let region = |r: Region<'tcx>, out: &mut Vec<Region<'tcx>>| match *r.kind() {
            RegionKind::ReBound(d, _) if d.0 != depth => {}
            _ => out.push(r),
        };
        let args = |args: &[GenericArg<'tcx>], out: &mut Vec<Region<'tcx>>| {
            for a in args {
                match *a {
                    GenericArg::Type(t) => walk(t, depth, out),
                    GenericArg::Region(r) => match *r.kind() {
                        RegionKind::ReBound(d, _) if d.0 != depth => {}
                        _ => out.push(r),
                    },
                    GenericArg::Const(_) => {}
                }
            }
        };
        match *ty.kind() {
            TyKind::Ref(r, t, _) => {
                region(r, out);
                walk(t, depth, out);
            }
            TyKind::Adt(_, a) | TyKind::FnDef(_, a) | TyKind::Closure(_, a) => args(a, out),
            TyKind::Alias(a) => args(a.args, out),
            TyKind::Array(t, _) | TyKind::Slice(t) | TyKind::RawPtr(t, _) => walk(t, depth, out),
            TyKind::Tuple(tys) => tys.iter().for_each(|t| walk(*t, depth, out)),
            TyKind::FnPtr(sig) => sig.value.inputs_and_output.iter().for_each(|t| walk(*t, depth + 1, out)),
            TyKind::Dynamic(preds, r) => {
                for p in preds {
                    match *p {
                        ExistentialPredicate::Trait { args: a, .. } => args(a, out),
                        ExistentialPredicate::Projection { args: a, term, .. } => {
                            args(a, out);
                            walk(term, depth, out);
                        }
                    }
                }
                region(r, out);
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(ty, 0, &mut out);
    out
}
