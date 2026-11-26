//! Queries (rustc's `rustc_middle::query`): each is a cache plus a provider
//! function. Providers are installed by the crates that implement them,
//! before the `TyCtxt` is made.

use std::cell::RefCell;
use std::hash::Hash;

use vsc_data_structures::FxHashMap;
use vsc_span::def_id::DefId;

use crate::middle::lang_items::LanguageItems;
use crate::ty::{AdtDefData, AssocItem, GenericPredicates, Generics, PolyFnSig, TraitRef, Ty, TyCtxt, TypeckResults};

/// The functions that compute each query.
#[derive(Clone, Copy)]
pub struct Providers {
    pub lang_items: for<'tcx> fn(TyCtxt<'tcx>) -> LanguageItems,
    pub generics_of: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> Generics,
    pub type_of: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> Ty<'tcx>,
    pub fn_sig: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> PolyFnSig<'tcx>,
    pub predicates_of: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> GenericPredicates<'tcx>,
    pub adt_def: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> AdtDefData,
    pub impl_trait_ref: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> Option<TraitRef<'tcx>>,
    pub associated_items: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> Vec<AssocItem>,
    pub typeck: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> TypeckResults<'tcx>,
    pub thir_body: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> crate::thir::Thir<'tcx>,
    pub mir_built: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> crate::mir::Body<'tcx>,
    pub mir_borrowck: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> (),
    pub optimized_mir: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> crate::mir::Body<'tcx>,
    pub closure_upvars: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> &'tcx [Ty<'tcx>],
    pub coroutine_layout: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> crate::mir::CoroutineLayout<'tcx>,
    pub item_bounds: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> GenericPredicates<'tcx>,
    pub repr_options: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> vsc_abi::ReprOptions,
    pub adt_destructor: for<'tcx> fn(TyCtxt<'tcx>, DefId) -> Option<DefId>,
    pub layout_of: for<'tcx> fn(
        TyCtxt<'tcx>,
        Ty<'tcx>,
    ) -> Result<crate::ty::TyAndLayout<'tcx>, crate::ty::layout::LayoutError<'tcx>>,
    pub fn_abi_of_fn_ptr: for<'tcx> fn(
        TyCtxt<'tcx>,
        (PolyFnSig<'tcx>, &'tcx [Ty<'tcx>]),
    ) -> Result<
        &'tcx crate::ty::layout::FnAbi<'tcx, Ty<'tcx>>,
        crate::ty::layout::FnAbiError<'tcx>,
    >,
    pub fn_abi_of_instance: for<'tcx> fn(
        TyCtxt<'tcx>,
        (crate::ty::Instance<'tcx>, &'tcx [Ty<'tcx>]),
    ) -> Result<
        &'tcx crate::ty::layout::FnAbi<'tcx, Ty<'tcx>>,
        crate::ty::layout::FnAbiError<'tcx>,
    >,
    pub resolve_instance_raw: for<'tcx> fn(
        TyCtxt<'tcx>,
        (DefId, crate::ty::GenericArgsRef<'tcx>),
    ) -> Option<crate::ty::Instance<'tcx>>,
    pub needs_drop_raw: for<'tcx> fn(TyCtxt<'tcx>, Ty<'tcx>) -> bool,
    pub codegen_fn_attrs:
        for<'tcx> fn(TyCtxt<'tcx>, DefId) -> crate::middle::codegen_fn_attrs::CodegenFnAttrs,
    pub symbol_name:
        for<'tcx> fn(TyCtxt<'tcx>, crate::ty::Instance<'tcx>) -> crate::ty::SymbolName<'tcx>,
    pub entry_fn: for<'tcx> fn(TyCtxt<'tcx>, ()) -> Option<(DefId, crate::ty::EntryFnType)>,
    pub collect_and_partition_mono_items:
        for<'tcx> fn(TyCtxt<'tcx>, ()) -> &'tcx [crate::mono::CodegenUnit<'tcx>],
}

impl Default for Providers {
    fn default() -> Self {
        fn missing(name: &str) -> ! {
            panic!("no provider for the `{name}` query: is the crate that implements it installed?")
        }
        Providers {
            lang_items: |_| missing("lang_items"),
            generics_of: |_, _| missing("generics_of"),
            type_of: |_, _| missing("type_of"),
            fn_sig: |_, _| missing("fn_sig"),
            predicates_of: |_, _| missing("predicates_of"),
            adt_def: |_, _| missing("adt_def"),
            impl_trait_ref: |_, _| missing("impl_trait_ref"),
            associated_items: |_, _| missing("associated_items"),
            typeck: |_, _| missing("typeck"),
            thir_body: |_, _| missing("thir_body"),
            mir_built: |_, _| missing("mir_built"),
            mir_borrowck: |_, _| missing("mir_borrowck"),
            optimized_mir: |_, _| missing("optimized_mir"),
            closure_upvars: |_, _| &[],
            coroutine_layout: |_, _| crate::mir::CoroutineLayout::default(),
            item_bounds: |_, _| GenericPredicates { parent: None, predicates: &[] },
            repr_options: |_, _| missing("repr_options"),
            adt_destructor: |_, _| missing("adt_destructor"),
            layout_of: |_, _| missing("layout_of"),
            fn_abi_of_fn_ptr: |_, _| missing("fn_abi_of_fn_ptr"),
            fn_abi_of_instance: |_, _| missing("fn_abi_of_instance"),
            resolve_instance_raw: |_, _| missing("resolve_instance_raw"),
            needs_drop_raw: |_, _| missing("needs_drop_raw"),
            codegen_fn_attrs: |_, _| missing("codegen_fn_attrs"),
            symbol_name: |_, _| missing("symbol_name"),
            entry_fn: |_, _| missing("entry_fn"),
            collect_and_partition_mono_items: |_, _| missing("collect_and_partition_mono_items"),
        }
    }
}

/// One query's results. A key being computed is `None`, so asking for it
/// again from inside its own computation is a cycle.
pub struct QueryCache<K, V> {
    map: RefCell<FxHashMap<K, Option<V>>>,
}

impl<K, V> Default for QueryCache<K, V> {
    fn default() -> Self {
        QueryCache { map: RefCell::new(FxHashMap::default()) }
    }
}

impl<K: Copy + Eq + Hash, V: Copy> QueryCache<K, V> {
    pub fn get(&self, key: K, compute: impl FnOnce() -> V, on_cycle: impl FnOnce() -> V) -> V {
        match self.map.borrow().get(&key) {
            Some(Some(v)) => return *v,
            Some(None) => return on_cycle(),
            None => {}
        }
        self.map.borrow_mut().insert(key, None);
        let v = compute();
        self.map.borrow_mut().insert(key, Some(v));
        v
    }
}
