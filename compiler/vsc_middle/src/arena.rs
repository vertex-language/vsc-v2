//! The arenas a `TyCtxt` allocates in (rustc's `rustc_middle::arena`).

use vsc_arena::TypedArena;
use vsc_span::Span;

use crate::middle::lang_items::LanguageItems;
use crate::ty::*;

#[derive(Default)]
pub struct Arena<'tcx> {
    pub(crate) ty_kinds: TypedArena<TyKind<'tcx>>,
    pub(crate) region_kinds: TypedArena<RegionKind>,
    pub(crate) const_kinds: TypedArena<ConstKind<'tcx>>,
    pub(crate) generic_args: TypedArena<GenericArg<'tcx>>,
    pub(crate) tys: TypedArena<Ty<'tcx>>,
    pub(crate) existentials: TypedArena<ExistentialPredicate<'tcx>>,
    pub(crate) bound_vars: TypedArena<BoundVar>,
    pub(crate) clauses: TypedArena<(Clause<'tcx>, Span)>,
    pub(crate) generics: TypedArena<Generics>,
    pub(crate) adt_defs: TypedArena<AdtDefData>,
    pub(crate) lang_items: TypedArena<LanguageItems>,
    pub(crate) assoc_items: TypedArena<Vec<AssocItem>>,
    pub(crate) typeck_results: TypedArena<TypeckResults<'tcx>>,
    pub(crate) thir: TypedArena<crate::thir::Thir<'tcx>>,
    pub(crate) mir: TypedArena<crate::mir::Body<'tcx>>,
}
