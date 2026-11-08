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
