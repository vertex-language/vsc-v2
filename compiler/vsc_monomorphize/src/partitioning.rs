//! Partitioning collected [`MonoItem`]s into deterministic [`CodegenUnit`]s.

use vsc_data_structures::FxHashSet;
use vsc_middle::middle::codegen_fn_attrs::Linkage;
use vsc_middle::mono::{CodegenUnit, MonoItem, MonoItemData, Visibility};
use vsc_middle::ty::{Instance, TyCtxt};
use vsc_span::Symbol;

pub fn partition<'tcx>(
    tcx: TyCtxt<'tcx>,
    items: FxHashSet<MonoItem<'tcx>>,
) -> Vec<CodegenUnit<'tcx>> {
    let n_cgus = tcx.options.cg.codegen_units.unwrap_or(1).max(1);
    let mut cgus: Vec<CodegenUnit<'tcx>> = (0..n_cgus)
        .map(|i| {
            let mut cgu = CodegenUnit::new(Symbol::intern(&format!("{}.{i}", tcx.crate_name)));
            if i == 0 {
                cgu.set_is_primary(true);
            }
            cgu
        })
        .collect();

    let mut sorted: Vec<(String, MonoItem<'tcx>)> =
        items.into_iter().map(|item| (item.symbol_name(tcx).name.to_string(), item)).collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let entry_def = tcx.entry_fn(()).map(|(d, _)| d);

    for (_, item) in sorted {
        let def_id = match item {
            MonoItem::Fn(inst) => inst.def_id(),
            MonoItem::Static(d) => d,
            MonoItem::GlobalAsm(id) => id.owner_id.to_def_id(),
        };
        let attrs = match item {
            MonoItem::Fn(inst) => tcx.codegen_instance_attrs(inst.def).into_owned(),
            _ => tcx.codegen_fn_attrs(def_id).clone(),
        };
        let is_entry = entry_def == Some(def_id)
            && matches!(item, MonoItem::Fn(inst) if inst == Instance::mono(tcx, def_id));

        let linkage = if let Some(explicit) = attrs.linkage {
            explicit
        } else if attrs.contains_extern_indicator() || is_entry || n_cgus > 1 {
            Linkage::External
        } else {
            Linkage::Internal
        };

        let size_estimate = item.size_estimate(tcx);
        let target_cgu = cgus
            .iter()
            .enumerate()
            .min_by_key(|(_, c)| c.size_estimate())
            .map(|(i, _)| i)
            .unwrap_or(0);

        cgus[target_cgu].modify_size_estimate(size_estimate);
        cgus[target_cgu].items_mut().insert(
            item,
            MonoItemData {
                inlined: false,
                linkage,
                visibility: Visibility::Default,
                size_estimate,
            },
        );
    }

    cgus
}
