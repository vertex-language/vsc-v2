//! Looking names up in packages and files (rustc's `ident.rs`).

use vsc_hir::def::{Namespace, PrimTy, Res};
use vsc_span::def_id::LocalDefId;
use vsc_span::{Symbol, sym};

use crate::{Binding, Resolver};

impl Resolver<'_> {
    pub(crate) fn module_binding(&self, module: LocalDefId, name: Symbol, ns: Namespace) -> Option<Binding> {
        self.modules.get(&module)?.bindings.get(&(name, ns)).copied()
    }

    pub(crate) fn import_res(&self, package: LocalDefId, file: usize, name: Symbol) -> Option<Res> {
        self.modules.get(&package)?.files.get(file)?.imports.get(&name).map(|&(res, _)| res)
    }
}

/// Built-in calls (exception 12).
pub(crate) fn builtin_value(name: Symbol) -> Option<Res> {
    let builtins = [
        sym::print,
        sym::println,
        sym::eprint,
        sym::eprintln,
        sym::format,
        sym::vec,
        sym::assert,
        sym::assertEq,
        sym::panic,
        sym::todo,
        sym::unreachable,
        sym::asm,
        sym::nakedAsm,
        sym::globalAsm,
    ];
    builtins.contains(&name).then_some(Res::Builtin(name))
}

/// Built-in types: the primitives (exception 2), `cstring`, and C's types.
pub(crate) fn builtin_type(name: Symbol) -> Option<Res> {
    if let Some(p) = PrimTy::from_name(name) {
        return Some(Res::PrimTy(p));
    }
    let c_types = [
        sym::cstring,
        sym::c_char,
        sym::c_schar,
        sym::c_uchar,
        sym::c_short,
        sym::c_ushort,
        sym::c_int,
        sym::c_uint,
        sym::c_long,
        sym::c_ulong,
        sym::c_longlong,
        sym::c_ulonglong,
        sym::c_float,
        sym::c_double,
        sym::c_void,
    ];
    c_types.contains(&name).then_some(Res::BuiltinTy(name))
}
