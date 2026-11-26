//! Type queries (`rustc_ty_utils`): layout computation, ABI classification,
//! instance resolution, drop checking, and attribute parsing.

mod abi;
mod attrs;
mod instance;
mod layout;
mod needs_drop;
#[cfg(test)]
mod tests;

use vsc_middle::query::Providers;

pub fn provide(providers: &mut Providers) {
    providers.repr_options = attrs::repr_options;
    providers.adt_destructor = attrs::adt_destructor;
    providers.codegen_fn_attrs = attrs::codegen_fn_attrs;
    providers.layout_of = layout::layout_of;
    providers.fn_abi_of_fn_ptr = abi::fn_abi_of_fn_ptr;
    providers.fn_abi_of_instance = abi::fn_abi_of_instance;
    providers.resolve_instance_raw = instance::resolve_instance_raw;
    providers.needs_drop_raw = needs_drop::needs_drop_raw;
}
