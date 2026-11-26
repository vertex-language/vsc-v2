//! Shared resolution errors.

use vsc_span::Ident;

use crate::Resolver;

impl Resolver<'_> {
    /// Built-in names (exception 12) are reserved: unlike Go's, they can't be
    /// shadowed.
    pub(crate) fn report_builtin_redeclared(&self, ident: Ident) {
        self.dcx()
            .struct_span_err(ident.span, format!("`{ident}` is a built-in name and can't be declared"))
            .with_span_label(ident.span, "built-in name")
            .with_note("built-in names are reserved")
            .emit();
    }
}
