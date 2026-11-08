//! Imports (rustc's `imports.rs`; exception 1).
//!
//! `import "a/b"` names a crate and the package path inside it; the crate is
//! this one (by its name) or a dependency. `import "./a/b"` starts at the
//! importing package and goes down its child packages. The import binds the
//! last segment, or the name given (`import geo "./geometry"`), in the
//! importing file only, as in Go. `import _ "path"` binds nothing.

use vsc_ast::{Crate, Import, Package};
use vsc_errors::ErrCode;
use vsc_hir::def::{DefKind, Namespace, Res};
use vsc_span::def_id::{CRATE_DEF_ID, LocalDefId};
use vsc_span::{Ident, Symbol, kw};

use crate::Resolver;

pub(crate) fn resolve_imports(r: &mut Resolver<'_>, krate: &Crate) {
    resolve_package(r, &krate.root);
}

fn resolve_package(r: &mut Resolver<'_>, pkg: &Package) {
    let module = r.local_def_id(pkg.id);
    for (file_index, file) in pkg.files.iter().enumerate() {
        for import in &file.imports {
            resolve_import(r, module, file_index, import);
        }
    }
    for child in &pkg.children {
        resolve_package(r, child);
    }
}

fn resolve_import(r: &mut Resolver<'_>, module: LocalDefId, file: usize, import: &Import) {
    let text = import.path.as_str();
    let (relative, rest) = match text.strip_prefix("./") {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let segments: Vec<&str> = rest.split('/').collect();
    if rest.is_empty() || segments.iter().any(|s| !vsc_lexer_is_ident(s)) {
        r.dcx()
            .struct_span_err(import.path_span, format!("invalid import path `{text}`"))
            .with_note("an import path is a crate name and the folders inside it, or `./` and folders of this package: `\"std/fs\"`, `\"./geometry\"`")
            .emit();
        return;
    }

    let res = if relative {
        walk_children(r, module, &segments, import)
    } else {
        let krate = Symbol::intern(segments[0]);
        if krate == r.crate_name {
            walk_children(r, CRATE_DEF_ID, &segments[1..], import)
        } else {
            report_missing_crate(r, krate, import);
            Res::Err
        }
    };

    if import.rename.is_some() || import.items.is_none() {
        let name = match import.rename {
            Some(rename) if rename.name == kw::Underscore => None,
            Some(rename) => Some(rename),
            None => {
                let last = segments.last().unwrap();
                let lo = vsc_span::BytePos(import.path_span.hi().0 - 1 - last.len() as u32);
                Some(Ident::new(Symbol::intern(last), vsc_span::Span::new(lo, vsc_span::BytePos(lo.0 + last.len() as u32))))
            }
        };
        if let Some(name) = name {
            bind_import(r, module, file, name, Namespace::TypeNS, res, import.span);
        }
    }

    if let Some(items) = &import.items {
        for item in items {
            let bound = item.rename.unwrap_or(item.ident);
            match res {
                Res::Def(DefKind::Mod, def) => {
                    let target = def.expect_local();
                    let ty_b = r.module_binding(target, item.ident.name, Namespace::TypeNS);
                    let val_b = r.module_binding(target, item.ident.name, Namespace::ValueNS);
                    if ty_b.is_none() && val_b.is_none() {
                        let pkg = r.modules[&target].name;
                        r.dcx()
                            .struct_span_err(item.span, format!("unresolved import `{}`", item.ident))
                            .with_code(ErrCode(432))
                            .with_span_label(item.span, format!("no `{}` in `{pkg}`", item.ident))
                            .emit();
                        if bound.name != kw::Underscore {
                            bind_import(r, module, file, bound, Namespace::TypeNS, Res::Err, item.span);
                            bind_import(r, module, file, bound, Namespace::ValueNS, Res::Err, item.span);
                        }
                        continue;
                    }
                    let mut reported_priv = false;
                    for (ns, b) in [(Namespace::TypeNS, ty_b), (Namespace::ValueNS, val_b)] {
                        let Some(b) = b else { continue };
                        if !reported_priv && !r.is_accessible_from(b.vis, module) {
                            reported_priv = true;
                            r.dcx()
                                .struct_span_err(item.ident.span, format!("{} `{}` is private", b.res.descr(), item.ident))
                                .with_code(ErrCode(603))
                                .with_span_label(item.ident.span, format!("private {}", b.res.descr()))
                                .with_span_note(b.span, format!("the {} `{}` is defined here", b.res.descr(), item.ident))
                                .with_help("make it `public`, or `package` to share it within the crate")
                                .emit();
                        }
                        if bound.name != kw::Underscore {
                            bind_import(r, module, file, bound, ns, b.res, item.span);
                        }
                    }
                }
                Res::Err => {
                    if bound.name != kw::Underscore {
                        bind_import(r, module, file, bound, Namespace::TypeNS, Res::Err, item.span);
                        bind_import(r, module, file, bound, Namespace::ValueNS, Res::Err, item.span);
                    }
                }
                _ => {}
            }
        }
    }
}

fn bind_import(
    r: &mut Resolver<'_>,
    module: LocalDefId,
    file: usize,
    name: Ident,
    ns: Namespace,
    res: Res,
    span: vsc_span::Span,
) {
    if let Some(item) = r.modules[&module].bindings.get(&(name.name, ns)).copied() {
        r.dcx()
            .struct_span_err(span, format!("the name `{name}` is defined multiple times"))
            .with_code(ErrCode(255))
            .with_span_label(item.span, format!("the {} `{name}` is defined here", item.res.descr()))
            .with_span_label(span, format!("`{name}` imported here"))
            .with_help("rename the import: `import other from \"path\"`")
            .emit();
        return;
    }
    let scope = &mut r.modules.get_mut(&module).unwrap().files[file];
    if let Some(&(_, prev)) = scope.imports.get(&(name.name, ns)) {
        r.dcx()
            .struct_span_err(span, format!("the name `{name}` is imported multiple times"))
            .with_code(ErrCode(252))
            .with_span_label(prev, format!("previous import of `{name}` here"))
            .with_span_label(span, format!("`{name}` reimported here"))
            .emit();
        return;
    }
    scope.imports.insert((name.name, ns), (res, span));
}

fn walk_children(r: &mut Resolver<'_>, start: LocalDefId, segments: &[&str], import: &Import) -> Res {
    let mut current = start;
    for seg in segments {
        let name = Symbol::intern(seg);
        match r.modules[&current].children.get(&name) {
            Some(&child) => current = child,
            None => {
                let parent = r.modules[&current].name;
                r.dcx()
                    .struct_span_err(import.path_span, format!("unresolved import `{}`", import.path))
                    .with_code(ErrCode(432))
                    .with_span_label(import.path_span, format!("no package `{seg}` in `{parent}`"))
                    .emit();
                return Res::Err;
            }
        }
    }
    Res::Def(DefKind::Mod, current.to_def_id())
}

fn report_missing_crate(r: &mut Resolver<'_>, krate: Symbol, import: &Import) {
    if !r.missing_crates.insert(krate) {
        return;
    }
    let mut err = r
        .dcx()
        .struct_span_err(import.path_span, format!("can't find crate for `{krate}`"))
        .with_code(ErrCode(463))
        .with_span_label(import.path_span, "can't find crate");
    if [vsc_span::sym::std, vsc_span::sym::core, vsc_span::sym::alloc].contains(&krate) {
        err.note("vsc-v2 can't build the standard library yet, so only this crate's own packages can be imported");
    } else {
        err.note(format!("add `{krate}` to `[dependencies]`, or import one of this crate's packages with `./`"));
    }
    err.emit();
}

fn vsc_lexer_is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_alphabetic()) && chars.all(|c| c == '_' || c.is_alphanumeric())
}
