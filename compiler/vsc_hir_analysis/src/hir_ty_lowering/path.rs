//! Path, associated-type, generic-argument, and trait-bound lowering from HIR to `Ty`.

use vsc_errors::ErrCode;
use vsc_hir::def::{DefKind, PrimTy, Res};
use vsc_hir::{self as hir, LangItem};
use vsc_middle::ty::*;
use vsc_span::def_id::DefId;
use vsc_span::{Span, Symbol, kw};

use super::{ElidedRegions, ItemCtxt};

impl<'tcx> ItemCtxt<'tcx> {
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

    pub(super) fn lower_resolved_path(&mut self, path: &hir::Path, span: Span) -> Ty<'tcx> {
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
    pub(super) fn lower_builtin_ty(&mut self, s: Symbol, span: Span) -> Ty<'tcx> {
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
    pub(super) fn find_assoc_ty(&self, tr: TraitRef<'tcx>, name: Symbol) -> Option<(TraitRef<'tcx>, DefId)> {
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
    pub(super) fn lower_type_relative(&mut self, base_ty: Ty<'tcx>, base: &hir::Ty, seg: &hir::PathSegment, span: Span) -> Ty<'tcx> {
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
    pub(super) fn param_bounds(&mut self, p: ParamTy, base: &hir::Ty) -> Vec<TraitRef<'tcx>> {
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

    pub(super) fn no_generic_args(&self, path: &hir::Path) {
        self.no_generic_args_except(path, None);
    }

    pub(super) fn no_generic_args_except(&self, path: &hir::Path, except: Option<usize>) {
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
