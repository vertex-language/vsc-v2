//! Monomorphized function and static instances (`rustc_middle::ty::instance`).

use std::fmt;

use vsc_hir::LangItem;
use vsc_hir::def::DefKind;
use vsc_span::def_id::{CRATE_DEF_ID, DefId};

use crate::ty::{
    Binder, ClosureKind, Const, FnSig, GenericArg, GenericArgsRef, Ty, TyCtxt, TyKind,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum InstanceKind<'tcx> {
    Item(DefId),
    Intrinsic(DefId),
    VTableShim(DefId),
    ReifyShim(DefId),
    FnPtrShim(DefId, Ty<'tcx>),
    Virtual(DefId, usize),
    ClosureOnceShim { call_once: DefId, closure: DefId, track_caller: bool },
    DropGlue(DefId, Option<Ty<'tcx>>),
}

impl<'tcx> InstanceKind<'tcx> {
    #[inline]
    pub fn def_id(self) -> DefId {
        match self {
            InstanceKind::Item(def_id)
            | InstanceKind::Intrinsic(def_id)
            | InstanceKind::VTableShim(def_id)
            | InstanceKind::ReifyShim(def_id)
            | InstanceKind::FnPtrShim(def_id, _)
            | InstanceKind::Virtual(def_id, _)
            | InstanceKind::DropGlue(def_id, _) => def_id,
            InstanceKind::ClosureOnceShim { call_once, .. } => call_once,
        }
    }

    pub fn requires_monomorphization(self, tcx: TyCtxt<'tcx>) -> bool {
        match self {
            InstanceKind::Item(def_id) => {
                let g = tcx.generics_of(def_id);
                g.parent_count > 0 || !g.own_params.is_empty()
            }
            InstanceKind::ClosureOnceShim { .. }
            | InstanceKind::DropGlue(..)
            | InstanceKind::FnPtrShim(..)
            | InstanceKind::ReifyShim(..)
            | InstanceKind::VTableShim(..) => true,
            InstanceKind::Intrinsic(_) | InstanceKind::Virtual(..) => false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Instance<'tcx> {
    pub def: InstanceKind<'tcx>,
    pub args: GenericArgsRef<'tcx>,
}

impl fmt::Display for Instance<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.def)
    }
}

impl<'tcx> Instance<'tcx> {
    #[inline]
    pub fn new_raw(def_id: DefId, args: GenericArgsRef<'tcx>) -> Self {
        Instance { def: InstanceKind::Item(def_id), args }
    }

    #[inline]
    pub fn new(def_id: DefId, args: GenericArgsRef<'tcx>) -> Self {
        Instance { def: InstanceKind::Item(def_id), args }
    }

    #[inline]
    pub fn mono(tcx: TyCtxt<'tcx>, def_id: DefId) -> Self {
        let args = tcx.erase_regions_args(tcx.identity_args(def_id));
        Instance { def: InstanceKind::Item(def_id), args }
    }

    #[inline]
    pub fn def_id(&self) -> DefId {
        self.def.def_id()
    }

    #[inline]
    pub fn instantiate_ty(&self, tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Ty<'tcx> {
        let substituted = tcx.instantiate(ty, self.args);
        tcx.reveal_opaque(tcx.erase_regions(substituted))
    }

    #[inline]
    pub fn instantiate_args(
        &self,
        tcx: TyCtxt<'tcx>,
        args: GenericArgsRef<'tcx>,
    ) -> GenericArgsRef<'tcx> {
        let substituted = tcx.instantiate_args(args, self.args);
        tcx.erase_regions_args(substituted)
    }

    #[inline]
    pub fn instantiate_const(&self, tcx: TyCtxt<'tcx>, c: Const<'tcx>) -> Const<'tcx> {
        tcx.instantiate_const(c, self.args)
    }

    pub fn ty(&self, tcx: TyCtxt<'tcx>) -> Ty<'tcx> {
        match self.def {
            InstanceKind::DropGlue(_, inner_ty) => {
                let target_ty = inner_ty.unwrap_or_else(|| {
                    self.args.first().map(|a| a.expect_ty()).unwrap_or(tcx.types.unit)
                });
                let ptr_ty = tcx.mk_ty(TyKind::RawPtr(target_ty, vsc_ast::Mutability::Mut));
                let sig = FnSig {
                    inputs_and_output: tcx.mk_type_list(&[ptr_ty, tcx.types.unit]),
                    c_variadic: false,
                    safety: crate::ty::Safety::Unsafe,
                    abi: None,
                };
                tcx.mk_ty(TyKind::FnPtr(Binder::dummy(sig)))
            }
            InstanceKind::Item(def_id) if tcx.def_kind(def_id) == DefKind::Closure => {
                tcx.mk_ty(TyKind::Closure(def_id, self.args))
            }
            InstanceKind::ClosureOnceShim { closure, .. } => {
                tcx.mk_ty(TyKind::Closure(closure, self.args))
            }
            InstanceKind::Item(def_id)
            | InstanceKind::Intrinsic(def_id)
            | InstanceKind::VTableShim(def_id)
            | InstanceKind::ReifyShim(def_id)
            | InstanceKind::FnPtrShim(def_id, _)
            | InstanceKind::Virtual(def_id, _) => {
                let base = tcx.type_of(def_id);
                self.instantiate_ty(tcx, base)
            }
        }
    }

    pub fn try_resolve(
        tcx: TyCtxt<'tcx>,
        def_id: DefId,
        args: GenericArgsRef<'tcx>,
    ) -> Option<Self> {
        let erased = tcx.erase_regions_args(args);
        tcx.resolve_instance_raw((def_id, erased))
    }

    pub fn expect_resolve(
        tcx: TyCtxt<'tcx>,
        def_id: DefId,
        args: GenericArgsRef<'tcx>,
    ) -> Self {
        Self::try_resolve(tcx, def_id, args).unwrap_or_else(|| {
            Instance { def: InstanceKind::Item(def_id), args: tcx.erase_regions_args(args) }
        })
    }

    pub fn resolve_drop_in_place(tcx: TyCtxt<'tcx>, ty: Ty<'tcx>) -> Self {
        let ty = tcx.erase_regions(ty);
        let drop_in_place =
            tcx.lang_items().get(LangItem::DropInPlace).unwrap_or_else(|| CRATE_DEF_ID.to_def_id());
        let needs_drop = tcx.needs_drop_raw(ty);
        let args = tcx.mk_args(&[GenericArg::Type(ty)]);
        let inner = needs_drop.then_some(ty);
        Instance { def: InstanceKind::DropGlue(drop_in_place, inner), args }
    }

    pub fn resolve_closure(
        tcx: TyCtxt<'tcx>,
        def_id: DefId,
        args: GenericArgsRef<'tcx>,
        requested_kind: ClosureKind,
    ) -> Self {
        let erased = tcx.erase_regions_args(args);
        let closure_kind = tcx
            .typeck(def_id)
            .closure_kinds
            .get(&def_id.expect_local())
            .copied()
            .unwrap_or(ClosureKind::Fn);
        if closure_kind == ClosureKind::FnOnce || closure_kind == requested_kind {
            return Instance::new_raw(def_id, erased);
        }
        let call_once = tcx
            .lang_items()
            .get(LangItem::FnOnce)
            .and_then(|tr| tcx.associated_items(tr).first().map(|a| a.def_id))
            .unwrap_or(def_id);
        Instance {
            def: InstanceKind::ClosureOnceShim {
                call_once,
                closure: def_id,
                track_caller: false,
            },
            args: erased,
        }
    }

    pub fn resolve_for_fn_ptr(
        tcx: TyCtxt<'tcx>,
        def_id: DefId,
        args: GenericArgsRef<'tcx>,
    ) -> Option<Self> {
        let inst = Self::try_resolve(tcx, def_id, args)?;
        match tcx.def_kind(inst.def_id()) {
            DefKind::AssocFn | DefKind::Fn => Some(inst),
            _ => Some(inst),
        }
    }

    pub fn is_user_defined(&self) -> bool {
        matches!(self.def, InstanceKind::Item(..))
    }

    pub fn name(&self, tcx: TyCtxt<'tcx>) -> String {
        tcx.print_instance_name(*self)
    }
}
