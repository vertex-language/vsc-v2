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

use vsc_errors::ErrCode;
use vsc_hir::def::{DefKind, IntTy, LifetimeRes, PrimTy, Res, UintTy};
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
            hir::TyKind::OpaqueDef(_) => {
                tcx.dcx()
                    .struct_span_err(t.span, "`impl Trait` types aren't supported yet")
                    .with_span_label(t.span, "not supported yet")
                    .with_help("use a generic parameter with a bound instead: `func f<T: Trait>(x: T)`")
                    .emit();
                tcx.types.error
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

    // -----------------------------------------------------------------------
    // Paths
    // -----------------------------------------------------------------------

    pub fn lower_qpath(&mut self, q: &hir::QPath, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        match q {
            hir::QPath::Resolved(None, path) => self.lower_resolved_path(path, span),
            hir::QPath::Resolved(Some(qself), path) => {
                // `<T as Trait>.Assoc`
                let self_ty = self.lower_ty(qself);
                let Res::Def(DefKind::AssocTy, assoc) = path.res else { return tcx.types.error };
                let trait_def = tcx.parent(assoc);
                let n = path.segments.len();
                let trait_seg = n.checked_sub(2).map(|i| &path.segments[i]);
                let args = self.lower_generic_args(trait_def, trait_seg.and_then(|s| s.args.as_deref()), Some(self_ty), span);
                // The trait's segment takes its arguments; no other does.
                self.no_generic_args_except(path, n.checked_sub(2));
                tcx.mk_ty(TyKind::Alias(AliasTy { def_id: assoc, args }))
            }
            hir::QPath::TypeRelative(base, seg) => {
                let base_ty = self.lower_ty(base);
                self.lower_type_relative(base_ty, base, seg, span)
            }
            hir::QPath::LangItem(item, span) => self.lang_item_adt(*item, *span),
        }
    }

    fn lower_resolved_path(&mut self, path: &hir::Path, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        let last_args = path.segments.last().and_then(|s| s.args.as_deref());
        match path.res {
            Res::Def(DefKind::Struct | DefKind::Enum | DefKind::Union | DefKind::Class, def) => {
                let args = self.lower_generic_args(def, last_args, None, span);
                let adt = tcx.adt_def(def);
                let ty = tcx.mk_adt(adt, args);
                if adt.is_class() {
                    // A use of a class is `Class<C>` (exception 3).
                    match tcx.require_lang_item(LangItem::Class, span) {
                        Some(class) => tcx.mk_adt(tcx.adt_def(class), tcx.mk_args(&[GenericArg::Type(ty)])),
                        None => tcx.types.error,
                    }
                } else {
                    ty
                }
            }
            Res::Def(DefKind::TyAlias, def) => {
                let args = self.lower_generic_args(def, last_args, None, span);
                tcx.instantiate(tcx.type_of(def), args)
            }
            Res::Def(DefKind::TyParam, def) => {
                self.no_generic_args(path);
                self.param_ty(def, span)
            }
            Res::Def(DefKind::ForeignTy, def) => {
                self.no_generic_args(path);
                tcx.mk_ty(TyKind::Foreign(def))
            }
            Res::Def(DefKind::AssocTy, _) => {
                tcx.dcx()
                    .struct_span_err(span, "ambiguous associated type")
                    .with_code(ErrCode(223))
                    .with_help("name the type it belongs to: `<Type as Trait>.Name`")
                    .emit();
                tcx.types.error
            }
            Res::PrimTy(p) => {
                self.no_generic_args(path);
                match p {
                    PrimTy::Bool => tcx.types.bool,
                    PrimTy::Char => tcx.types.char,
                    PrimTy::Int(i) => tcx.mk_ty(TyKind::Int(i)),
                    PrimTy::Uint(u) => tcx.mk_ty(TyKind::Uint(u)),
                    PrimTy::Float(f) => tcx.mk_ty(TyKind::Float(f)),
                    // `string` alone is `String` (exception 2).
                    PrimTy::Str => self.lang_item_adt(LangItem::String, span),
                }
            }
            Res::BuiltinTy(s) => {
                self.no_generic_args(path);
                self.lower_builtin_ty(s, span)
            }
            Res::SelfTyParam { .. } => {
                self.no_generic_args(path);
                tcx.types.self_param
            }
            Res::SelfTyAlias { alias_to } => {
                self.no_generic_args(path);
                tcx.type_of(alias_to)
            }
            Res::Err => tcx.types.error,
            other => {
                tcx.dcx()
                    .struct_span_err(span, format!("expected type, found {}", other.descr()))
                    .with_code(ErrCode(573))
                    .emit();
                tcx.types.error
            }
        }
    }

    /// `cstring` and the C types, for the target.
    fn lower_builtin_ty(&mut self, s: Symbol, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        let t = tcx.target;
        let windows = t.os == "windows";
        let lp64 = t.pointer_width == 64 && !windows;
        // `char` is unsigned on ARM and RISC-V Linux and Android, signed on
        // Apple and Windows platforms and x86.
        let char_unsigned = matches!(t.arch, "aarch64" | "arm" | "riscv64" | "powerpc64" | "s390x") && !matches!(t.os, "macos" | "ios" | "windows");
        let int = |i: IntTy| tcx.mk_ty(TyKind::Int(i));
        let uint = |u: UintTy| tcx.mk_ty(TyKind::Uint(u));
        match s.as_str() {
            "cstring" => self.lang_item_adt(LangItem::CString, span),
            "c_char" if char_unsigned => uint(UintTy::U8),
            "c_char" | "c_schar" => int(IntTy::I8),
            "c_uchar" => uint(UintTy::U8),
            "c_short" => int(IntTy::I16),
            "c_ushort" => uint(UintTy::U16),
            "c_int" => int(IntTy::I32),
            "c_uint" => uint(UintTy::U32),
            "c_long" if lp64 => int(IntTy::I64),
            "c_long" => int(IntTy::I32),
            "c_ulong" if lp64 => uint(UintTy::U64),
            "c_ulong" => uint(UintTy::U32),
            "c_longlong" => int(IntTy::I64),
            "c_ulonglong" => uint(UintTy::U64),
            "c_float" => tcx.mk_ty(TyKind::Float(vsc_hir::def::FloatTy::F32)),
            "c_double" => tcx.mk_ty(TyKind::Float(vsc_hir::def::FloatTy::F64)),
            "c_void" => self.lang_item_adt(LangItem::CVoid, span),
            other => panic!("unknown built-in type `{other}`"),
        }
    }

    /// The associated type `name` of `tr`'s trait or, failing that, of its
    /// supertraits, with the trait reference that declares it.
    fn find_assoc_ty(&self, tr: TraitRef<'tcx>, name: Symbol) -> Option<(TraitRef<'tcx>, DefId)> {
        let tcx = self.tcx;
        let mut stack = vec![tr];
        let mut seen = Vec::new();
        while let Some(t) = stack.pop() {
            if seen.contains(&t.def_id) {
                continue;
            }
            seen.push(t.def_id);
            if let Some(item) = tcx.associated_items(t.def_id).iter().find(|i| i.kind == AssocKind::Type && i.name == name) {
                return Some((t, item.def_id));
            }
            for (c, _) in tcx.predicates_of(t.def_id).predicates.iter().skip(1) {
                if let Clause::Trait(s) = tcx.instantiate_clause(*c, t.args) {
                    if s.self_ty() == t.self_ty() {
                        stack.push(s);
                    }
                }
            }
        }
        None
    }

    /// `T.Assoc`: an associated type of a trait `T` is bounded by, or of the
    /// trait itself for `Self`.
    fn lower_type_relative(&mut self, base_ty: Ty<'tcx>, base: &hir::Ty, seg: &hir::PathSegment, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        if base_ty == tcx.types.error {
            return base_ty;
        }
        let candidates: Vec<TraitRef<'tcx>> = match *base_ty.kind() {
            TyKind::Param(p) => self.param_bounds(p, base),
            _ => Vec::new(),
        };
        let mut found: Vec<(TraitRef<'tcx>, DefId)> = Vec::new();
        for tr in candidates {
            if let Some((owner, item)) = self.find_assoc_ty(tr, seg.ident.name) {
                if !found.iter().any(|(t, _)| t.def_id == owner.def_id) {
                    found.push((owner, item));
                }
            }
        }
        if seg.args.is_some() {
            tcx.dcx()
                .struct_span_err(span, "generic associated types aren't supported yet")
                .with_span_label(seg.ident.span, "takes no generic arguments")
                .emit();
        }
        match &found[..] {
            [(tr, assoc)] => tcx.mk_ty(TyKind::Alias(AliasTy { def_id: *assoc, args: tr.args })),
            [] if matches!(base_ty.kind(), TyKind::Param(_)) => {
                tcx.dcx()
                    .struct_span_err(seg.ident.span, format!("associated type `{}` not found for `{}`", seg.ident, tcx.ty_to_string(base_ty)))
                    .with_code(ErrCode(220))
                    .with_span_label(seg.ident.span, format!("associated type `{}` not found", seg.ident))
                    .emit();
                tcx.types.error
            }
            [] => {
                tcx.dcx()
                    .struct_span_err(span, "ambiguous associated type")
                    .with_code(ErrCode(223))
                    .with_help(format!(
                        "if there's a trait with an associated type `{}`, name it: `<{} as Trait>.{}`",
                        seg.ident,
                        tcx.ty_to_string(base_ty),
                        seg.ident
                    ))
                    .emit();
                tcx.types.error
            }
            _ => {
                let names: Vec<String> = found.iter().map(|(t, _)| format!("`{}`", tcx.def_path_str(t.def_id))).collect();
                tcx.dcx()
                    .struct_span_err(seg.ident.span, format!("ambiguous associated type `{}` in bounds of `{}`", seg.ident, tcx.ty_to_string(base_ty)))
                    .with_code(ErrCode(221))
                    .with_note(format!("it's an associated type of {}", names.join(" and ")))
                    .with_help(format!("name the trait: `<{} as Trait>.{}`", tcx.ty_to_string(base_ty), seg.ident))
                    .emit();
                tcx.types.error
            }
        }
    }

    /// The traits a type parameter is bounded by, read from the HIR of the
    /// item and its parents (asking for their predicates here could be a
    /// cycle, as in rustc's `type_param_predicates`). A trait's `Self` is
    /// bounded by the trait.
    fn param_bounds(&mut self, p: ParamTy, base: &hir::Ty) -> Vec<TraitRef<'tcx>> {
        let tcx = self.tcx;
        // Which definition is the parameter?
        let param_def = match &base.kind {
            hir::TyKind::Path(hir::QPath::Resolved(None, path)) => match path.res {
                Res::Def(DefKind::TyParam, d) => Some(d),
                Res::SelfTyParam { trait_ } => {
                    return vec![TraitRef { def_id: trait_, args: tcx.identity_args(trait_) }];
                }
                _ => None,
            },
            _ => None,
        };
        let Some(param_def) = param_def else { return Vec::new() };
        let mut out = Vec::new();
        let mut def = Some(self.item_def);
        while let Some(d) = def {
            if let Some(generics) = crate::collect::hir_generics(tcx, d) {
                for pred in &generics.predicates {
                    let hir::WherePredicate::Bound { bounded_ty, bounds, .. } = pred else { continue };
                    let hir::TyKind::Path(hir::QPath::Resolved(None, bp)) = &bounded_ty.kind else { continue };
                    if bp.res != Res::Def(DefKind::TyParam, param_def) {
                        continue;
                    }
                    for b in bounds {
                        if let hir::GenericBound::Trait(poly) = b {
                            let self_ty = tcx.mk_param(p.index, p.name);
                            if let Some((tr, _)) = self.lower_trait_ref(&poly.trait_ref, self_ty) {
                                out.push(tr);
                            }
                        }
                    }
                }
            }
            def = tcx.generics_of(d).parent;
        }
        out
    }

    fn no_generic_args(&self, path: &hir::Path) {
        self.no_generic_args_except(path, None);
    }

    fn no_generic_args_except(&self, path: &hir::Path, except: Option<usize>) {
        for (i, seg) in path.segments.iter().enumerate() {
            if Some(i) == except {
                continue;
            }
            if let Some(args) = &seg.args {
                if !args.args.is_empty() || !args.constraints.is_empty() {
                    self.tcx
                        .dcx()
                        .struct_span_err(args.span, format!("generic arguments are not allowed on {} `{}`", path.res.descr(), seg.ident))
                        .with_code(ErrCode(109))
                        .with_span_label(args.span, "remove these arguments")
                        .emit();
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Generic arguments
    // -----------------------------------------------------------------------

    /// The arguments for `def`'s parameters (its parents' are `def`'s own
    /// identity, as for an associated item named from inside its
    /// container). Missing lifetimes are elided; missing types with defaults
    /// take their defaults.
    pub fn lower_generic_args(
        &mut self,
        def: DefId,
        args: Option<&hir::GenericArgs>,
        self_ty: Option<Ty<'tcx>>,
        span: Span,
    ) -> GenericArgsRef<'tcx> {
        let tcx = self.tcx;
        let generics = tcx.generics_of(def);
        let mut out: Vec<GenericArg<'tcx>> = Vec::with_capacity(generics.count());
        if generics.parent_count > 0 {
            out.extend_from_slice(&tcx.identity_args(generics.parent.unwrap())[..generics.parent_count]);
        }

        let (mut lifetimes, mut types, mut consts) = (Vec::new(), Vec::new(), Vec::new());
        if let Some(a) = args {
            for arg in &a.args {
                match arg {
                    hir::GenericArg::Lifetime(o) => lifetimes.push(o),
                    hir::GenericArg::Type(_) | hir::GenericArg::Infer(_) => types.push(arg),
                    hir::GenericArg::Const(_) => consts.push(arg),
                }
            }
        }
        let own = &generics.own_params;
        let want_lifetimes = own.iter().filter(|p| p.kind == GenericParamDefKind::Lifetime).count();
        let want_types = own.iter().filter(|p| matches!(p.kind, GenericParamDefKind::Type { synthetic: false, .. })).count();
        let min_types =
            own.iter().filter(|p| matches!(p.kind, GenericParamDefKind::Type { synthetic: false, has_default: false })).count();
        let want_consts = own.iter().filter(|p| matches!(p.kind, GenericParamDefKind::Const { .. })).count();
        let descr = tcx.def_kind(def).descr();
        let name = tcx.def_path_str(def);
        let arg_span = args.map_or(span, |a| a.span);
        let plural = |n: usize, what: &str| format!("{n} {what} argument{}", if n == 1 { "" } else { "s" });

        if !lifetimes.is_empty() && lifetimes.len() != want_lifetimes {
            tcx.dcx()
                .struct_span_err(arg_span, format!("{descr} takes {} but {} {} supplied", plural(want_lifetimes, "lifetime"), plural(lifetimes.len(), "lifetime"), if lifetimes.len() == 1 { "was" } else { "were" }))
                .with_code(ErrCode(107))
                .emit();
        }
        // In a body, leaving out the type arguments infers them.
        let infer_types = self.elided == ElidedRegions::Body && types.is_empty();
        if !infer_types && (types.len() < min_types || types.len() > want_types) {
            let mut err = if types.is_empty() {
                tcx.dcx().struct_span_err(span, format!("missing generics for {descr} `{name}`"))
            } else {
                let expected = if min_types == want_types { plural(want_types, "generic") } else { format!("at least {}", plural(min_types, "generic")) };
                tcx.dcx().struct_span_err(arg_span, format!(
                    "{descr} takes {expected} but {} {} supplied",
                    plural(types.len(), "generic"),
                    if types.len() == 1 { "was" } else { "were" }
                ))
            };
            err = err.with_code(ErrCode(107));
            if types.is_empty() {
                let params: Vec<&str> = own.iter().filter(|p| matches!(p.kind, GenericParamDefKind::Type { synthetic: false, .. })).map(|p| p.name.as_str()).collect();
                err = err.with_help(format!("add the missing generic arguments: `{name}<{}>`", params.join(", ")));
            }
            err.emit();
        }
        if consts.len() > want_consts || (consts.len() < want_consts && !types.is_empty()) {
            tcx.dcx()
                .struct_span_err(arg_span, format!("{descr} takes {} but {} {} supplied", plural(want_consts, "const"), plural(consts.len(), "const"), if consts.len() == 1 { "was" } else { "were" }))
                .with_code(ErrCode(107))
                .emit();
        }

        let (mut oi, mut ti, mut ci) = (lifetimes.into_iter(), std::mem::take(&mut types).into_iter(), std::mem::take(&mut consts).into_iter());
        for p in own {
            let arg = match p.kind {
                GenericParamDefKind::Type { synthetic: true, .. } if p.name == kw::SelfUpper => {
                    GenericArg::Type(self_ty.unwrap_or(tcx.types.error))
                }
                GenericParamDefKind::Lifetime => match oi.next() {
                    Some(o) => GenericArg::Region(self.lower_lifetime(o)),
                    None if want_lifetimes > 0 => GenericArg::Region(self.elided_region(arg_span)),
                    None => GenericArg::Region(tcx.regions.re_error),
                },
                GenericParamDefKind::Type { has_default, .. } => match ti.next() {
                    Some(hir::GenericArg::Type(t)) => GenericArg::Type(self.lower_ty(t)),
                    Some(hir::GenericArg::Infer(_)) if self.elided == ElidedRegions::Body => GenericArg::Type(self.fresh_ty()),
                    Some(hir::GenericArg::Infer(s)) => {
                        tcx.dcx()
                            .struct_span_err(*s, "the placeholder `_` is not allowed within types on item signatures")
                            .with_code(ErrCode(121))
                            .emit();
                        GenericArg::Type(tcx.types.error)
                    }
                    Some(_) => GenericArg::Type(tcx.types.error),
                    None if infer_types => GenericArg::Type(self.fresh_ty()),
                    None if has_default => {
                        let default = tcx.type_of(p.def_id);
                        GenericArg::Type(tcx.instantiate(default, &out))
                    }
                    None => GenericArg::Type(tcx.types.error),
                },
                GenericParamDefKind::Const { .. } => match ci.next() {
                    Some(hir::GenericArg::Const(c)) => GenericArg::Const(self.lower_const_arg(c)),
                    _ => GenericArg::Const(tcx.mk_const(ConstKind::Error)),
                },
            };
            out.push(arg);
        }
        if let Some(a) = args {
            if !a.constraints.is_empty() && tcx.def_kind(def) != DefKind::Trait {
                tcx.dcx()
                    .struct_span_err(a.constraints[0].span, "associated item constraints are not allowed here")
                    .with_code(ErrCode(229))
                    .emit();
            }
        }
        tcx.mk_args(&out)
    }

    // -----------------------------------------------------------------------
    // Traits
    // -----------------------------------------------------------------------

    /// `Trait<A, Item = B>` applied to `self_ty`: the trait reference and the
    /// `Item = B` projections. `None` if the path isn't a trait.
    pub fn lower_trait_ref(&mut self, t: &hir::TraitRef, self_ty: Ty<'tcx>) -> Option<(TraitRef<'tcx>, Vec<(AliasTy<'tcx>, Ty<'tcx>)>)> {
        let tcx = self.tcx;
        let Res::Def(DefKind::Trait, def) = t.path.res else { return None };
        let seg = t.path.segments.last().unwrap();
        for s in &t.path.segments[..t.path.segments.len() - 1] {
            if s.args.is_some() {
                self.no_generic_args(&hir::Path { span: t.path.span, res: Res::Def(DefKind::Mod, def), segments: Vec::new() });
            }
        }
        let args = self.lower_generic_args(def, seg.args.as_deref(), Some(self_ty), t.path.span);
        let tr = TraitRef { def_id: def, args };
        let mut projections = Vec::new();
        if let Some(a) = &seg.args {
            for c in &a.constraints {
                // `Fn(A) -> B` constrains `Output`, which `FnOnce` declares.
                let Some((owner, item)) = self.find_assoc_ty(tr, c.ident.name) else {
                    tcx.dcx()
                        .struct_span_err(c.ident.span, format!("associated type `{}` not found for `{}`", c.ident, tcx.def_path_str(def)))
                        .with_code(ErrCode(220))
                        .emit();
                    continue;
                };
                let alias = AliasTy { def_id: item, args: owner.args };
                match &c.kind {
                    hir::AssocItemConstraintKind::Equality(ty) => {
                        let ty = self.lower_ty(ty);
                        projections.push((alias, ty));
                    }
                    hir::AssocItemConstraintKind::Bound(_) => {
                        tcx.dcx()
                            .struct_span_err(c.span, "bounds on associated types in trait arguments aren't supported yet")
                            .with_help(format!("use a where clause: `where <T as {}>.{}: Bound`", tcx.def_path_str(def), c.ident))
                            .emit();
                    }
                }
            }
        }
        Some((tr, projections))
    }

    /// The predicates of `bounds` on `self_ty`.
    pub fn lower_bounds(&mut self, self_ty: Ty<'tcx>, bounds: &[hir::GenericBound], out: &mut Vec<(Clause<'tcx>, Span)>) {
        for b in bounds {
            match b {
                hir::GenericBound::Trait(poly) => {
                    // `?Sized` relaxes a default bound vsc-v2 doesn't add yet.
                    if poly.maybe {
                        continue;
                    }
                    if let Some((tr, projections)) = self.lower_trait_ref(&poly.trait_ref, self_ty) {
                        out.push((Clause::Trait(tr), poly.span));
                        for (alias, term) in projections {
                            out.push((Clause::Projection(alias, term), poly.span));
                        }
                    }
                }
                hir::GenericBound::Outlives(o) => {
                    let r = self.lower_lifetime(o);
                    out.push((Clause::TypeOutlives(self_ty, r), o.span));
                }
                hir::GenericBound::Use(..) => {}
            }
        }
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
