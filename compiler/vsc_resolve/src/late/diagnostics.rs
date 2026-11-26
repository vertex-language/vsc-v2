//! Diagnostic reporting for unresolved names, wrong kinds, and break-label mistakes.

use vsc_ast::*;
use vsc_errors::{Applicability, ErrCode};
use vsc_hir::def::{DefKind, LifetimeRes, Namespace, Res};
use vsc_span::{Ident, Symbol, kw};

use super::{LateResolutionVisitor, PathSource, RibKind};

impl<'r, 'a> LateResolutionVisitor<'r, 'a> {
    pub(super) fn report_not_found(&mut self, path: &Path, source: PathSource) {
        let ident = path.segments[0].ident;
        // Found in the other namespace: a type used as a value, or the reverse.
        let other = if source.ns() == Namespace::TypeNS { Namespace::ValueNS } else { Namespace::TypeNS };
        if let Some(res) = self.lookup(ident, other, true) {
            if res != Res::Err {
                self.report_wrong_kind(path, res, source);
            }
            return;
        }
        let (code, what) = match source {
            PathSource::Expr => (425, "value"),
            PathSource::Type => (412, "type"),
            PathSource::Trait => (405, "trait"),
            PathSource::Struct => (422, "struct, variant or union type"),
            PathSource::TupleStruct | PathSource::Pat => (531, "tuple struct or tuple variant"),
        };
        if ident.name == kw::SelfLower && source == PathSource::Expr {
            self.report_self_value(ident);
            return;
        }
        if ident.name == kw::SelfUpper {
            self.r
                .dcx()
                .struct_span_err(ident.span, "cannot find type `Self` in this scope")
                .with_code(ErrCode(411))
                .with_span_label(ident.span, "`Self` is only available in traits and `implement` blocks")
                .emit();
            return;
        }
        let mut err = self
            .r
            .dcx()
            .struct_span_err(ident.span, format!("cannot find {what} `{ident}` in this scope"))
            .with_code(ErrCode(code))
            .with_span_label(ident.span, "not found in this scope");
        if let Some(note) = std_name_note(ident.name) {
            err.note(note);
        }
        if source == PathSource::Type || source == PathSource::Trait {
            if let Some(hint) = rust_type_hint(ident.name) {
                err.span_suggestion(ident.span, hint.0, hint.1, Applicability::MachineApplicable);
            }
            // `<t>` declared a lifetime (exception 11: types are PascalCase).
            if matches!(self.lookup_lifetime(ident), Some(LifetimeRes::Param(_))) {
                let upper: String = ident.name.as_str().chars().take(1).flat_map(char::to_uppercase).chain(ident.name.as_str().chars().skip(1)).collect();
                err = err
                    .with_note(format!("`{ident}` is a lifetime parameter: lowercase generic parameters are lifetimes"))
                    .with_help(format!("type parameters are `PascalCase`: `{upper}`"));
            }
        }
        err.emit();
    }

    pub(super) fn report_self_value(&mut self, ident: Ident) {
        self.r
            .dcx()
            .struct_span_err(ident.span, "expected value, found package `self`")
            .with_code(ErrCode(424))
            .with_span_label(ident.span, "`self` value is only available in methods with a `self` parameter")
            .with_help("in a flat method, the receiver has the name you gave it: `func (t: &Task) m()` uses `t`")
            .emit();
    }

    pub(super) fn report_failed_to_resolve(&mut self, ident: Ident) {
        if ident.name == kw::SelfLower {
            return self.report_self_value(ident);
        }
        let mut err = self
            .r
            .dcx()
            .struct_span_err(ident.span, format!("failed to resolve: use of undeclared type or package `{ident}`"))
            .with_code(ErrCode(433))
            .with_span_label(ident.span, format!("use of undeclared type or package `{ident}`"));
        if let Some(note) = std_name_note(ident.name) {
            err.note(note);
        } else if ident.name.as_str().chars().next().is_some_and(char::is_lowercase) {
            err.help(format!("if `{ident}` is a package, import it: `import \"path/to/{ident}\"`"));
        }
        err.emit();
    }

    pub(super) fn report_wrong_kind(&mut self, path: &Path, res: Res, source: PathSource) {
        let span = path.span;
        let shown = path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join(".");
        let code = match source {
            PathSource::Expr => 423,
            PathSource::Type => 573,
            PathSource::Trait => 404,
            PathSource::Struct => 574,
            PathSource::TupleStruct | PathSource::Pat => 532,
        };
        let mut err = self
            .r
            .dcx()
            .struct_span_err(span, format!("expected {}, found {} `{shown}`", source.expected(), res.descr()))
            .with_code(ErrCode(code))
            .with_span_label(span, format!("not a {}", source.expected()));
        if source == PathSource::Expr && matches!(res, Res::Def(DefKind::Struct | DefKind::Class, _)) {
            err.help(format!("use struct literal syntax: `{shown} {{ … }}`"));
        }
        if source == PathSource::Type && matches!(res, Res::Def(DefKind::Trait, _)) {
            err.span_suggestion(span.shrink_to_lo(), "trait objects are written with `dyn`", "dyn ", Applicability::MaybeIncorrect);
        }
        err.emit();
    }

    // -----------------------------------------------------------------------
    // Lifetimes (exception 13)
    // -----------------------------------------------------------------------


    pub(super) fn report_break_name(&mut self, v: &Expr) -> bool {
        let ExprKind::Path(None, p) = &v.kind else { return false };
        let Some(ident) = p.as_ident() else { return false };
        if self.lookup(ident, Namespace::ValueNS, true).is_some() {
            return false;
        }
        let mut crossed = false;
        for rib in self.label_ribs.iter().rev() {
            if rib.bindings.contains_key(&ident.name) {
                if crossed {
                    self.r
                        .dcx()
                        .struct_span_err(ident.span, format!("use of unreachable label `{ident}`"))
                        .with_code(ErrCode(767))
                        .with_span_label(ident.span, format!("unreachable label `{ident}`"))
                        .with_note("labels are unreachable through functions and function literals")
                        .emit();
                    return true;
                }
                break;
            }
            match rib.kind {
                RibKind::Item => break,
                RibKind::FnLiteral => crossed = true,
                _ => {}
            }
        }
        false
    }

}

/// A note for Rust standard library names, which can't resolve until
/// vsc-v2 can build `library/`.
fn std_name_note(name: Symbol) -> Option<String> {
    const STD_NAMES: &[&str] = &[
        "Option", "Some", "None", "Result", "Ok", "Err", "Vec", "Box", "String", "Rc", "Arc", "Weak", "Default", "Clone",
        "Copy", "Debug", "Display", "PartialEq", "Eq", "PartialOrd", "Ord", "Hash", "Iterator", "IntoIterator", "Fn",
        "FnMut", "FnOnce", "Drop", "From", "Into", "Send", "Sync", "Sized", "ToString", "AsRef", "Deref", "HashMap",
    ];
    STD_NAMES
        .contains(&name.as_str())
        .then(|| format!("`{name}` comes from the standard library, which vsc-v2 can't build yet"))
}

/// Rust's names for Vertex's primitive types (exception 2).
fn rust_type_hint(name: Symbol) -> Option<(&'static str, &'static str)> {
    Some(match name.as_str() {
        "i8" => ("Vertex writes `i8` as `int8`", "int8"),
        "i16" => ("Vertex writes `i16` as `int16`", "int16"),
        "i32" => ("Vertex writes `i32` as `int32`", "int32"),
        "i64" => ("Vertex writes `i64` as `int64`", "int64"),
        "i128" => ("Vertex writes `i128` as `int128`", "int128"),
        "isize" => ("Vertex writes `isize` as `int`", "int"),
        "u8" => ("Vertex writes `u8` as `uint8`", "uint8"),
        "u16" => ("Vertex writes `u16` as `uint16`", "uint16"),
        "u32" => ("Vertex writes `u32` as `uint32`", "uint32"),
        "u64" => ("Vertex writes `u64` as `uint64`", "uint64"),
        "u128" => ("Vertex writes `u128` as `uint128`", "uint128"),
        "usize" => ("Vertex writes `usize` as `uint`", "uint"),
        "f32" => ("Vertex writes `f32` as `float32`", "float32"),
        "f64" => ("Vertex writes `f64` as `float64`", "float64"),
        "str" => ("Vertex writes `str` as `string`", "string"),
        "String" => ("Vertex writes `String` as `string`", "string"),
        _ => return None,
    })
}
