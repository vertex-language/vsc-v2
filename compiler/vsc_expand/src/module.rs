//! Loading packages from folders (rustc's `expand/module.rs`, which loads
//! `mod` files).
//!
//! Exception 1: a folder is a package, and every `.vs` file in it belongs to
//! it. Its subfolders that hold Vertex files, at any depth, are its child
//! packages, named after the folder. At the crate root, `bin/` is skipped:
//! that is where cargo keeps other binary crates.

use std::path::{Path, PathBuf};

use vsc_ast::{Crate, DUMMY_NODE_ID, Package, SourceFile};
use vsc_session::ParseSess;
use vsc_span::{DUMMY_SP, Ident, Span, Symbol};

pub fn load_crate(psess: &ParseSess, root: &Path, crate_name: Option<&str>) -> std::io::Result<Crate> {
    std::fs::metadata(root)?;
    let dir = root.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let mut files = vec![root.to_path_buf()];
    files.extend(vs_files(&dir)?.into_iter().filter(|p| !same_file(p, root)));

    let parsed: Vec<SourceFile> = files.iter().filter_map(|p| parse(psess, p)).collect();
    let declared = check_package_decls(psess, &parsed, None);
    let name = crate_name
        .map(Symbol::intern)
        .or(declared)
        .unwrap_or_else(|| Symbol::intern(&default_crate_name(root)));
    let span = parsed.first().map(|f| f.span.shrink_to_lo()).unwrap_or(DUMMY_SP);
    let children = load_children(psess, &dir, true)?;
    let root_pkg = Package {
        name: Ident::new(name, span),
        id: DUMMY_NODE_ID,
        files: parsed,
        children,
        dir,
        span,
    };
    Ok(Crate { root: root_pkg, id: DUMMY_NODE_ID, span })
}

/// The crate name when none is given: the root file's name, or its folder's
/// for `main.vs` and `lib.vs`.
fn default_crate_name(root: &Path) -> String {
    let stem = root.file_stem().and_then(|s| s.to_str()).unwrap_or("main");
    if matches!(stem, "main" | "lib") {
        let parent = root.canonicalize().ok().and_then(|p| p.parent().map(Path::to_path_buf));
        let dir = parent.as_deref().and_then(|p| p.file_name()).and_then(|n| n.to_str());
        // `src/main.vs` belongs to the folder above `src`.
        let dir = match dir {
            Some("src") => parent.as_deref().and_then(|p| p.parent()).and_then(|p| p.file_name()).and_then(|n| n.to_str()),
            other => other,
        };
        if let Some(d) = dir.filter(|d| vsc_lexer::is_ident(d)) {
            return d.to_string();
        }
    }
    stem.replace('-', "_")
}

fn load_children(psess: &ParseSess, dir: &Path, at_crate_root: bool) -> std::io::Result<Vec<Package>> {
    let mut children = Vec::new();
    for sub in subdirs(dir)? {
        let name = sub.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "target" || (at_crate_root && name == "bin") {
            continue;
        }
        if !contains_vertex(&sub) {
            continue;
        }
        if let Some(pkg) = load_package(psess, &sub, &name)? {
            children.push(pkg);
        }
    }
    Ok(children)
}

fn load_package(psess: &ParseSess, dir: &Path, folder_name: &str) -> std::io::Result<Option<Package>> {
    let parsed: Vec<SourceFile> = vs_files(dir)?.iter().filter_map(|p| parse(psess, p)).collect();
    let span = parsed.first().map(|f| f.span.shrink_to_lo()).unwrap_or(DUMMY_SP);
    if !vsc_lexer::is_ident(folder_name) || Symbol::intern(folder_name).is_used_or_reserved_keyword() {
        psess
            .dcx
            .struct_span_err(span, format!("folder `{}` can't be a package: `{folder_name}` isn't a name", dir.display()))
            .with_note("a package is named after its folder, so the folder's name must be an identifier")
            .emit();
        return Ok(None);
    }
    let name = Symbol::intern(folder_name);
    check_package_decls(psess, &parsed, Some(name));
    let children = load_children(psess, dir, false)?;
    Ok(Some(Package { name: Ident::new(name, span), id: DUMMY_NODE_ID, files: parsed, children, dir: dir.to_path_buf(), span }))
}

/// The `package` declarations of one folder's files must agree, and in a
/// child package they must name the folder. Returns the declared name.
fn check_package_decls(psess: &ParseSess, files: &[SourceFile], folder: Option<Symbol>) -> Option<Symbol> {
    let mut first: Option<(Symbol, Span)> = None;
    for file in files {
        let Some(decl) = &file.package else { continue };
        match first {
            None => first = Some((decl.name.name, decl.name.span)),
            Some((name, span)) if name != decl.name.name => {
                psess
                    .dcx
                    .struct_span_err(decl.name.span, format!("found package `{}`, but this folder is package `{name}`", decl.name))
                    .with_span_note(span, format!("package `{name}` is declared here"))
                    .with_note("every file in a folder belongs to the same package")
                    .emit();
            }
            Some(_) => {}
        }
        if let Some(folder) = folder {
            if decl.name.name != folder {
                psess
                    .dcx
                    .struct_span_err(decl.name.span, format!("package `{}` is in folder `{folder}`", decl.name))
                    .with_note("a package is named after its folder")
                    .with_span_suggestion(
                        decl.name.span,
                        "use the folder's name",
                        folder.as_str(),
                        vsc_errors::Applicability::MachineApplicable,
                    )
                    .emit();
            }
        }
    }
    first.map(|(n, _)| n)
}

fn parse(psess: &ParseSess, path: &Path) -> Option<SourceFile> {
    match vsc_parse::parse_file(psess, path) {
        Ok(f) => Some(f),
        Err(err) => {
            psess.dcx.err(format!("couldn't read `{}`: {err}", path.display()));
            None
        }
    }
}

fn vs_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "vs") && p.is_file())
        .collect();
    files.sort();
    Ok(files)
}

fn subdirs(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut dirs: Vec<PathBuf> =
        std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_dir()).collect();
    dirs.sort();
    Ok(dirs)
}

/// Does the folder, or any folder under it, hold a `.vs` file?
fn contains_vertex(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().any(|e| {
        let p = e.path();
        if p.is_dir() {
            !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) && contains_vertex(&p)
        } else {
            p.extension().is_some_and(|e| e == "vs")
        }
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
