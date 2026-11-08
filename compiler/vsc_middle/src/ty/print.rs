//! Printing types in Vertex's spelling, for diagnostics (rustc's
//! `ty/print`): `int64`, `string` and `&string`, `never`, `&a T`,
//! `func(T) -> U`, `shapes.Circle`.

use vsc_hir::LangItem;
use vsc_hir::def::PrimTy;
use vsc_span::kw;

use super::*;

/// The names of the bound variables of the binders being printed,
/// innermost last.
type Binders<'tcx> = Vec<&'tcx [BoundVar]>;

impl<'tcx> TyCtxt<'tcx> {
    pub fn ty_to_string(self, ty: Ty<'tcx>) -> String {
        let mut s = String::new();
        self.write_ty(&mut s, ty, &mut Vec::new());
        s
    }

    pub fn region_to_string(self, r: Region<'tcx>) -> String {
        self.region_str(r, &[])
    }

    fn region_str(self, r: Region<'tcx>, binders: &[&'tcx [BoundVar]]) -> String {
        match *r.kind() {
            RegionKind::ReEarlyParam(p) => p.name.to_string(),
            RegionKind::ReBound(d, b) => {
                let name = binders
                    .len()
                    .checked_sub(1 + d.0 as usize)
                    .and_then(|i| binders[i].get(b.var as usize))
                    .map_or(kw::Empty, |v| v.name);
                if name == kw::Empty { "_".to_string() } else { name.to_string() }
            }
            RegionKind::ReErased => "_".to_string(),
            RegionKind::ReStatic => "static".to_string(),
            RegionKind::ReError => "{error}".to_string(),
        }
    }

    pub fn fn_sig_to_string(self, sig: PolyFnSig<'tcx>) -> String {
        let mut s = String::new();
        self.write_fn_sig(&mut s, sig, &mut Vec::new());
        s
    }

    pub fn trait_ref_to_string(self, t: TraitRef<'tcx>) -> String {
        let mut s = self.def_path_str(t.def_id);
        self.write_args(&mut s, &t.args[1..], &mut Vec::new());
        s
    }

    pub fn clause_to_string(self, c: Clause<'tcx>) -> String {
        match c {
            Clause::Trait(t) => format!("{}: {}", self.ty_to_string(t.self_ty()), self.trait_ref_to_string(t)),
            Clause::Projection(alias, term) => {
                format!("{} == {}", self.ty_to_string(self.mk_ty(TyKind::Alias(alias))), self.ty_to_string(term))
            }
            Clause::TypeOutlives(t, r) => format!("{}: {}", self.ty_to_string(t), self.region_to_string(r)),
            Clause::RegionOutlives(a, b) => format!("{}: {}", self.region_to_string(a), self.region_to_string(b)),
        }
    }

    fn write_args(self, s: &mut String, args: &[GenericArg<'tcx>], b: &mut Binders<'tcx>) {
        let mut shown: Vec<String> = Vec::new();
        for a in args {
            match a {
                GenericArg::Type(t) => {
                    let mut x = String::new();
                    self.write_ty(&mut x, *t, b);
                    shown.push(x);
                }
                GenericArg::Region(r) => {
                    if !matches!(r.kind(), RegionKind::ReErased) {
                        shown.push(self.region_str(*r, b));
                    }
                }
                GenericArg::Const(c) => shown.push(self.const_to_string(*c)),
            }
        }
        if !shown.is_empty() {
            s.push('<');
            s.push_str(&shown.join(", "));
            s.push('>');
        }
    }

    pub fn const_to_string(self, c: Const<'tcx>) -> String {
        match *c.kind() {
            ConstKind::Param(p) => p.name.to_string(),
            ConstKind::Value(_, v) => v.to_string(),
            ConstKind::Unevaluated(..) => "_".to_string(),
            ConstKind::Error => "{error}".to_string(),
        }
    }

    fn write_fn_sig(self, s: &mut String, sig: PolyFnSig<'tcx>, b: &mut Binders<'tcx>) {
        b.push(sig.bound_vars);
        let sig_v = sig.value;
        if sig_v.safety == Safety::Unsafe {
            s.push_str("unsafe ");
        }
        if let Some(abi) = sig_v.abi {
            s.push_str(&format!("extern \"{abi}\" "));
        }
        s.push_str("func(");
        for (i, t) in sig_v.inputs().iter().enumerate() {
            if i > 0 {
                s.push_str(", ");
            }
            self.write_ty(s, *t, b);
        }
        if sig_v.c_variadic {
            s.push_str(if sig_v.inputs().is_empty() { "..." } else { ", ..." });
        }
        s.push(')');
        let out = sig_v.output();
        if out != self.types.unit {
            s.push_str(" -> ");
            self.write_ty(s, out, b);
        }
        b.pop();
    }

    fn write_ty(self, s: &mut String, ty: Ty<'tcx>, b: &mut Binders<'tcx>) {
        match *ty.kind() {
            TyKind::Bool => s.push_str("bool"),
            TyKind::Char => s.push_str("char"),
            TyKind::Int(i) => s.push_str(PrimTy::Int(i).name().as_str()),
            TyKind::Uint(u) => s.push_str(PrimTy::Uint(u).name().as_str()),
            TyKind::Float(f) => s.push_str(PrimTy::Float(f).name().as_str()),
            TyKind::Str => s.push_str("str"),
            TyKind::Adt(def, args) => {
                let li = self.lang_items();
                match li.from_def_id(def.did()) {
                    Some(LangItem::String) => s.push_str("string"),
                    Some(LangItem::CString) => s.push_str("cstring"),
                    // `Class<T>` is how a use of class `T` is typed
                    // (exception 3); it prints as the class.
                    Some(LangItem::Class) if args.len() == 1 => self.write_ty(s, args[0].expect_ty(), b),
                    // `Func<fn(T) -> U>` is `func(T) -> U` (exception 14).
                    Some(LangItem::Func) if args.len() == 1 => match *args[0].expect_ty().kind() {
                        TyKind::FnPtr(sig) => self.write_fn_sig(s, sig, b),
                        _ => s.push_str("Func"),
                    },
                    _ => {
                        s.push_str(&self.def_path_str(def.did()));
                        self.write_args(s, args, b);
                    }
                }
            }
            TyKind::Foreign(d) => s.push_str(&self.def_path_str(d)),
            TyKind::Array(t, n) => {
                s.push('[');
                self.write_ty(s, t, b);
                s.push_str("; ");
                s.push_str(&self.const_to_string(n));
                s.push(']');
            }
            TyKind::Slice(t) => {
                s.push('[');
                self.write_ty(s, t, b);
                s.push(']');
            }
            TyKind::RawPtr(t, m) => {
                s.push_str(if m.is_mut() { "*mut " } else { "*const " });
                self.write_ty(s, t, b);
            }
            TyKind::Ref(r, t, m) => {
                s.push('&');
                let name = self.region_str(r, b);
                if name != "_" {
                    s.push_str(&name);
                    s.push(' ');
                }
                if m.is_mut() {
                    s.push_str("mut ");
                }
                // `&str` is `&string`, `&CStr` is `&cstring` (exception 2).
                match *t.kind() {
                    TyKind::Str if !m.is_mut() => s.push_str("string"),
                    TyKind::Adt(d, _) if !m.is_mut() && self.is_lang_item(d.did(), LangItem::CStr) => s.push_str("cstring"),
                    _ => self.write_ty(s, t, b),
                }
            }
            TyKind::FnDef(d, args) => {
                let sig = self.fn_sig(d);
                let io: Vec<Ty<'tcx>> = sig.value.inputs_and_output.iter().map(|t| self.instantiate(*t, args)).collect();
                let sig = Binder { value: FnSig { inputs_and_output: self.mk_type_list(&io), ..sig.value }, bound_vars: sig.bound_vars };
                self.write_fn_sig(s, sig, b);
                s.push_str(&format!(" {{{}}}", self.def_path_str(d)));
            }
            TyKind::FnPtr(sig) => self.write_fn_sig(s, sig, b),
            TyKind::Dynamic(preds, r) => {
                s.push_str("dyn ");
                for (i, p) in preds.iter().enumerate() {
                    if i > 0 {
                        s.push_str(" + ");
                    }
                    match *p {
                        ExistentialPredicate::Trait { def_id, args } => {
                            s.push_str(&self.def_path_str(def_id));
                            self.write_args(s, args, b);
                        }
                        ExistentialPredicate::Projection { def_id, term, .. } => {
                            s.push_str(&format!("{} = ", self.item_name(def_id)));
                            self.write_ty(s, term, b);
                        }
                    }
                }
                if !matches!(r.kind(), RegionKind::ReErased) {
                    s.push_str(&format!(" + {}", self.region_str(r, b)));
                }
            }
            TyKind::Closure(..) => s.push_str("{function literal}"),
            TyKind::Never => s.push_str("never"),
            TyKind::Tuple(tys) => {
                s.push('(');
                for (i, t) in tys.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    self.write_ty(s, *t, b);
                }
                if tys.len() == 1 {
                    s.push(',');
                }
                s.push(')');
            }
            TyKind::Alias(a) => {
                let trait_def = self.parent(a.def_id);
                s.push('<');
                self.write_ty(s, a.self_ty(), b);
                s.push_str(" as ");
                s.push_str(&self.def_path_str(trait_def));
                let trait_args = self.generics_of(trait_def).count();
                self.write_args(s, &a.args[1..trait_args.min(a.args.len())], b);
                s.push_str(">.");
                s.push_str(self.item_name(a.def_id).as_str());
            }
            TyKind::Param(p) => s.push_str(p.name.as_str()),
            TyKind::Infer(InferTy::IntVar(_)) => s.push_str("{integer}"),
            TyKind::Infer(InferTy::FloatVar(_)) => s.push_str("{float}"),
            TyKind::Infer(_) => s.push('_'),
            TyKind::Error => s.push_str("{error}"),
        }
    }
}
