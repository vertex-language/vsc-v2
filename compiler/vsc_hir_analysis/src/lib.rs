//! Item signatures and the checks on them (rustc's `rustc_hir_analysis`).
//!
//! - `collect`: the signature queries (`type_of`, `fn_sig`, `generics_of`,
//!   `predicates_of`, `adt_def`, …).
//! - `hir_ty_lowering`: HIR types to `ty::Ty`, where exceptions 2, 3, 13
//!   and 14 take effect.
//! - `check`: computes every item's signature, so its errors are reported,
//!   and checks that trait implementations are complete.
//! - `coherence`: inherent implementations, including flat receivers
//!   (exception 8): a method defined twice for a type is E0592.

mod check;
mod coherence;
pub mod collect;
pub mod dump;
pub mod hir_ty_lowering;

#[cfg(test)]
mod tests;

use vsc_middle::query::Providers;
use vsc_middle::ty::TyCtxt;

pub fn provide(providers: &mut Providers) {
    collect::provide(providers);
}

pub fn check_crate(tcx: TyCtxt<'_>) {
    check::check_items(tcx);
    coherence::check(tcx);
}
