//! Monomorphization item collection and CGU partitioning (`rustc_monomorphize`).

pub mod collector;
pub mod partitioning;

#[cfg(test)]
mod tests;

use vsc_middle::mono::CodegenUnit;
use vsc_middle::query::Providers;
use vsc_middle::ty::TyCtxt;

pub fn provide(providers: &mut Providers) {
    providers.collect_and_partition_mono_items = collect_and_partition_mono_items;
}

fn collect_and_partition_mono_items<'tcx>(
    tcx: TyCtxt<'tcx>,
    (): (),
) -> &'tcx [CodegenUnit<'tcx>] {
    let items = collector::collect_crate_mono_items(tcx);
    let units = partitioning::partition(tcx, items);
    tcx.alloc_codegen_units(units)
}
