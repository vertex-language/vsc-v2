//! Expansion (rustc's `rustc_expand`): from a root file to the crate's AST.
//!
//! 1. `module`: the root file's folder is the root package; its subfolders
//!    with Vertex files in them are child packages (exception 1).
//! 2. `config`: `@cfg` removes what doesn't apply to this compilation, and
//!    `@cfgAttr` adds attributes where it does.
//! 3. `vsc_builtin_macros`: built-in calls (`println`, `format`, `assert`, …)
//!    are checked and become `ExprKind::Builtin`. (`@derive` and `@test` wait
//!    for the standard library.)
//! 4. Every node gets its `NodeId`.

pub mod config;
pub mod module;

use std::path::{Path, PathBuf};

use vsc_ast::mut_visit::MutVisitor;
use vsc_ast::node_id::{CRATE_NODE_ID, NodeId};
use vsc_ast::{Crate, DUMMY_NODE_ID, Package};
use vsc_session::ParseSess;
use vsc_session::config::Cfg;

/// What expansion needs to know about the compilation.
pub struct ExpansionConfig<'a> {
    pub crate_name: Option<&'a str>,
    pub cfg: &'a Cfg,
}

/// Loads, configures and numbers the crate whose root file is `root`.
pub fn expand_crate(psess: &ParseSess, root: &Path, ecfg: &ExpansionConfig<'_>) -> std::io::Result<Crate> {
    expand_crate_with_externs(psess, root, ecfg, &[], None)
}

/// Loads, configures and numbers the crate whose root file is `root`, along
/// with explicit source-level `--extern` crates and an optional auto-loaded
/// `core` library root (loaded automatically when the crate imports `core`).
pub fn expand_crate_with_externs(
    psess: &ParseSess,
    root: &Path,
    ecfg: &ExpansionConfig<'_>,
    externs: &[(String, PathBuf)],
    auto_core_path: Option<&Path>,
) -> std::io::Result<Crate> {
    let mut krate = module::load_crate(psess, root, ecfg.crate_name)?;
    let root_name = krate.root.name.name.as_str();
    let mut loaded_core = root_name == "core";
    for (name, path) in externs {
        if name == root_name {
            continue;
        }
        if name == "core" {
            loaded_core = true;
        }
        let ext = module::load_extern_crate(psess, name, path)?;
        krate.extern_crates.push(ext);
    }
    if !loaded_core
        && let Some(core_path) = auto_core_path
        && (package_imports_crate(&krate.root, "core")
            || krate.extern_crates.iter().any(|e| package_imports_crate(e, "core")))
    {
        let ext = module::load_extern_crate(psess, "core", core_path)?;
        krate.extern_crates.push(ext);
    }
    config::StripUnconfigured { psess, cfg: ecfg.cfg }.visit_crate(&mut krate);
    vsc_builtin_macros::expand_builtins(psess, &mut krate);
    assign_node_ids(&mut krate);
    Ok(krate)
}

fn package_imports_crate(pkg: &Package, crate_name: &str) -> bool {
    for file in &pkg.files {
        for import in &file.imports {
            let p = import.path.as_str();
            if p == crate_name || p.starts_with(&format!("{crate_name}/")) {
                return true;
            }
        }
    }
    pkg.children.iter().any(|c| package_imports_crate(c, crate_name))
}

/// Numbers every node in the crate: the crate is 0, the rest count up in
/// tree order.
pub fn assign_node_ids(krate: &mut Crate) {
    struct Numberer {
        next: u32,
    }
    impl MutVisitor for Numberer {
        fn visit_id(&mut self, id: &mut NodeId) {
            if *id == DUMMY_NODE_ID {
                *id = NodeId::from_u32(self.next);
                self.next += 1;
            }
        }
    }
    krate.id = CRATE_NODE_ID;
    let mut numberer = Numberer { next: 1 };
    numberer.visit_package(&mut krate.root);
    for ext in &mut krate.extern_crates {
        numberer.visit_package(ext);
    }
}

#[cfg(test)]
mod tests;
