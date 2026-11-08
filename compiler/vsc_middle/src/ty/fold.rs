//! Rewriting types (rustc's `ty/fold.rs` and `instantiate`).

use super::*;

/// Rebuilds a type bottom-up, letting the implementor replace types,
/// regions and constants. The default methods recurse.
pub trait TypeFolder<'tcx>: Sized {
    fn tcx(&self) -> TyCtxt<'tcx>;

    fn fold_ty(&mut self, t: Ty<'tcx>) -> Ty<'tcx> {
        super_fold_ty(self, t)
    }
    fn fold_region(&mut self, r: Region<'tcx>) -> Region<'tcx> {
        r
    }
    fn fold_const(&mut self, c: Const<'tcx>) -> Const<'tcx> {
        super_fold_const(self, c)
    }
}

pub fn fold_args<'tcx, F: TypeFolder<'tcx>>(f: &mut F, args: GenericArgsRef<'tcx>) -> GenericArgsRef<'tcx> {
    let folded: Vec<_> = args
        .iter()
        .map(|a| match *a {
            GenericArg::Type(t) => GenericArg::Type(f.fold_ty(t)),
            GenericArg::Region(r) => GenericArg::Region(f.fold_region(r)),
            GenericArg::Const(c) => GenericArg::Const(f.fold_const(c)),
        })
        .collect();
    f.tcx().mk_args(&folded)
}

pub fn fold_fn_sig<'tcx, F: TypeFolder<'tcx>>(f: &mut F, sig: FnSig<'tcx>) -> FnSig<'tcx> {
    let io: Vec<_> = sig.inputs_and_output.iter().map(|t| f.fold_ty(*t)).collect();
    FnSig { inputs_and_output: f.tcx().mk_type_list(&io), ..sig }
}

pub fn super_fold_ty<'tcx, F: TypeFolder<'tcx>>(f: &mut F, t: Ty<'tcx>) -> Ty<'tcx> {
    let tcx = f.tcx();
    let kind = match *t.kind() {
        TyKind::Adt(d, args) => TyKind::Adt(d, fold_args(f, args)),
        TyKind::Array(e, n) => TyKind::Array(f.fold_ty(e), f.fold_const(n)),
        TyKind::Slice(e) => TyKind::Slice(f.fold_ty(e)),
        TyKind::RawPtr(e, m) => TyKind::RawPtr(f.fold_ty(e), m),
        TyKind::Ref(r, e, m) => TyKind::Ref(f.fold_region(r), f.fold_ty(e), m),
        TyKind::FnDef(d, args) => TyKind::FnDef(d, fold_args(f, args)),
        TyKind::FnPtr(sig) => TyKind::FnPtr(Binder { value: fold_fn_sig(f, sig.value), bound_vars: sig.bound_vars }),
        TyKind::Dynamic(preds, r) => {
            let preds: Vec<_> = preds
                .iter()
                .map(|p| match *p {
                    ExistentialPredicate::Trait { def_id, args } => ExistentialPredicate::Trait { def_id, args: fold_args(f, args) },
                    ExistentialPredicate::Projection { def_id, args, term } => {
                        ExistentialPredicate::Projection { def_id, args: fold_args(f, args), term: f.fold_ty(term) }
                    }
                })
                .collect();
            TyKind::Dynamic(tcx.mk_existentials(&preds), f.fold_region(r))
        }
        TyKind::Closure(d, args) => TyKind::Closure(d, fold_args(f, args)),
        TyKind::Tuple(tys) => {
            let tys: Vec<_> = tys.iter().map(|t| f.fold_ty(*t)).collect();
            TyKind::Tuple(tcx.mk_type_list(&tys))
        }
        TyKind::Alias(a) => TyKind::Alias(AliasTy { def_id: a.def_id, args: fold_args(f, a.args) }),
        TyKind::Bool
        | TyKind::Char
        | TyKind::Int(_)
        | TyKind::Uint(_)
        | TyKind::Float(_)
        | TyKind::Str
        | TyKind::Foreign(_)
        | TyKind::Never
        | TyKind::Param(_)
        | TyKind::Infer(_)
        | TyKind::Error => return t,
    };
    tcx.mk_ty(kind)
}

pub fn super_fold_const<'tcx, F: TypeFolder<'tcx>>(f: &mut F, c: Const<'tcx>) -> Const<'tcx> {
    match *c.kind() {
        ConstKind::Unevaluated(d, args) => {
            let args = fold_args(f, args);
            f.tcx().mk_const(ConstKind::Unevaluated(d, args))
        }
        ConstKind::Value(t, v) => {
            let t = f.fold_ty(t);
            f.tcx().mk_const(ConstKind::Value(t, v))
        }
        ConstKind::Param(_) | ConstKind::Error => c,
    }
}

/// Replaces parameters with arguments (rustc's `EarlyBinder::instantiate`).
struct ArgFolder<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    args: &'a [GenericArg<'tcx>],
}

impl<'tcx> TypeFolder<'tcx> for ArgFolder<'_, 'tcx> {
    fn tcx(&self) -> TyCtxt<'tcx> {
        self.tcx
    }
    fn fold_ty(&mut self, t: Ty<'tcx>) -> Ty<'tcx> {
        match *t.kind() {
            TyKind::Param(p) => match self.args.get(p.index as usize) {
                Some(GenericArg::Type(t)) => *t,
                _ => t,
            },
            _ => super_fold_ty(self, t),
        }
    }
    fn fold_region(&mut self, r: Region<'tcx>) -> Region<'tcx> {
        match *r.kind() {
            RegionKind::ReEarlyParam(p) => match self.args.get(p.index as usize) {
                Some(GenericArg::Region(r)) => *r,
                _ => r,
            },
            _ => r,
        }
    }
    fn fold_const(&mut self, c: Const<'tcx>) -> Const<'tcx> {
        match *c.kind() {
            ConstKind::Param(p) => match self.args.get(p.index as usize) {
                Some(GenericArg::Const(c)) => *c,
                _ => c,
            },
            _ => super_fold_const(self, c),
        }
    }
}

impl<'tcx> TyCtxt<'tcx> {
    /// `ty` with each parameter replaced by its argument.
    pub fn instantiate(self, ty: Ty<'tcx>, args: &[GenericArg<'tcx>]) -> Ty<'tcx> {
        if args.is_empty() {
            return ty;
        }
        ArgFolder { tcx: self, args }.fold_ty(ty)
    }

    pub fn instantiate_clause(self, c: Clause<'tcx>, args: &[GenericArg<'tcx>]) -> Clause<'tcx> {
        let mut f = ArgFolder { tcx: self, args };
        match c {
            Clause::Trait(t) => Clause::Trait(TraitRef { def_id: t.def_id, args: fold_args(&mut f, t.args) }),
            Clause::Projection(a, term) => {
                Clause::Projection(AliasTy { def_id: a.def_id, args: fold_args(&mut f, a.args) }, f.fold_ty(term))
            }
            Clause::TypeOutlives(t, r) => Clause::TypeOutlives(f.fold_ty(t), f.fold_region(r)),
            Clause::RegionOutlives(a, b) => Clause::RegionOutlives(f.fold_region(a), f.fold_region(b)),
        }
    }

    pub fn instantiate_args(self, args: GenericArgsRef<'tcx>, with: &[GenericArg<'tcx>]) -> GenericArgsRef<'tcx> {
        if with.is_empty() {
            return args;
        }
        fold_args(&mut ArgFolder { tcx: self, args: with }, args)
    }

    pub fn instantiate_const(self, c: Const<'tcx>, with: &[GenericArg<'tcx>]) -> Const<'tcx> {
        ArgFolder { tcx: self, args: with }.fold_const(c)
    }

    pub fn erase_regions_args(self, args: GenericArgsRef<'tcx>) -> GenericArgsRef<'tcx> {
        let v: Vec<_> = args
            .iter()
            .map(|a| match *a {
                GenericArg::Type(t) => GenericArg::Type(self.erase_regions(t)),
                GenericArg::Region(_) => GenericArg::Region(self.regions.re_erased),
                c => c,
            })
            .collect();
        self.mk_args(&v)
    }

    /// `ty` with every region erased: type checking doesn't look at them
    /// (borrow checking does).
    pub fn erase_regions(self, ty: Ty<'tcx>) -> Ty<'tcx> {
        struct Erase<'tcx>(TyCtxt<'tcx>);
        impl<'tcx> TypeFolder<'tcx> for Erase<'tcx> {
            fn tcx(&self) -> TyCtxt<'tcx> {
                self.0
            }
            fn fold_region(&mut self, _: Region<'tcx>) -> Region<'tcx> {
                self.0.regions.re_erased
            }
        }
        Erase(self).fold_ty(ty)
    }

    pub fn erase_regions_clause(self, c: Clause<'tcx>) -> Clause<'tcx> {
        let e = |t| self.erase_regions(t);
        let ea = |args: GenericArgsRef<'tcx>| {
            let v: Vec<_> = args
                .iter()
                .map(|a| match *a {
                    GenericArg::Type(t) => GenericArg::Type(e(t)),
                    GenericArg::Region(_) => GenericArg::Region(self.regions.re_erased),
                    c => c,
                })
                .collect();
            self.mk_args(&v)
        };
        match c {
            Clause::Trait(t) => Clause::Trait(TraitRef { def_id: t.def_id, args: ea(t.args) }),
            Clause::Projection(a, t) => Clause::Projection(AliasTy { def_id: a.def_id, args: ea(a.args) }, e(t)),
            Clause::TypeOutlives(t, _) => Clause::TypeOutlives(e(t), self.regions.re_erased),
            Clause::RegionOutlives(..) => Clause::RegionOutlives(self.regions.re_erased, self.regions.re_erased),
        }
    }

    /// Every region in `ty`, outermost first.
    pub fn regions_in(self, ty: Ty<'tcx>) -> Vec<Region<'tcx>> {
        struct Collect<'tcx> {
            tcx: TyCtxt<'tcx>,
            out: Vec<Region<'tcx>>,
        }
        impl<'tcx> TypeFolder<'tcx> for Collect<'tcx> {
            fn tcx(&self) -> TyCtxt<'tcx> {
                self.tcx
            }
            fn fold_region(&mut self, r: Region<'tcx>) -> Region<'tcx> {
                self.out.push(r);
                r
            }
        }
        let mut c = Collect { tcx: self, out: Vec::new() };
        c.fold_ty(ty);
        c.out
    }
}
