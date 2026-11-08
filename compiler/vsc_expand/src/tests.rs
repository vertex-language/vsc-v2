use std::path::{Path, PathBuf};

use vsc_ast_pretty_shim::print;
use vsc_errors::SharedBuffer;
use vsc_session::ParseSess;
use vsc_session::config::{Cfg, TargetInfo, default_configuration};

use crate::{ExpansionConfig, expand_crate};

mod vsc_ast_pretty_shim {
    /// Item names of a package, for checking what `@cfg` kept.
    pub fn print(pkg: &vsc_ast::Package) -> Vec<String> {
        pkg.items().filter_map(|i| i.kind.ident()).map(|i| i.to_string()).collect()
    }
}

/// A scratch folder with the given files.
fn tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vsc-expand-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

fn expand(root: &Path, cfg: &Cfg) -> (vsc_ast::Crate, String) {
    let buf = SharedBuffer::new();
    let psess = ParseSess::with_buffer(buf.clone());
    let krate = expand_crate(&psess, root, &ExpansionConfig { crate_name: None, cfg }).unwrap();
    (krate, buf.contents())
}

fn linux() -> Cfg {
    default_configuration(&TargetInfo::from_triple("x86_64-unknown-linux-gnu").unwrap(), true, false)
}

#[test]
fn folders_are_packages() {
    let dir = tree("pkgs", &[
        ("src/main.vs", "package main\nfunc main() {}\n"),
        ("src/util.vs", "package main\nfunc helper() {}\n"),
        ("src/geometry/point.vs", "package geometry\npublic struct Point {}\n"),
        ("src/geometry/shapes/circle.vs", "package shapes\npublic struct Circle {}\n"),
        ("src/empty/inner/x.vs", "package inner\nfunc x() {}\n"),
        ("src/bin/tool/main.vs", "package main\nfunc main() {}\n"),
        ("src/notes/readme.txt", "not vertex"),
    ]);
    let (krate, errors) = expand(&dir.join("src/main.vs"), &linux());
    assert_eq!(errors, "");
    let root = &krate.root;
    assert_eq!(root.name.to_string(), "main");
    assert_eq!(print(root), vec!["main", "helper"]);
    let names: Vec<String> = root.children.iter().map(|c| c.name.to_string()).collect();
    assert_eq!(names, vec!["empty", "geometry"], "bin/ and folders without .vs files are skipped");
    let geometry = &root.children[1];
    assert_eq!(geometry.children[0].name.to_string(), "shapes");
    assert_eq!(root.children[0].files.len(), 0);
    assert_eq!(root.children[0].children[0].name.to_string(), "inner");
}

#[test]
fn package_names_must_match_folders() {
    let dir = tree("names", &[
        ("main.vs", "package main\n"),
        ("other.vs", "package other\n"),
        ("geo/a.vs", "package geometry\n"),
    ]);
    let (_, errors) = expand(&dir.join("main.vs"), &linux());
    assert!(errors.contains("found package `other`, but this folder is package `main`"), "{errors}");
    assert!(errors.contains("package `geometry` is in folder `geo`"), "{errors}");
}

#[test]
fn cfg_strips_items_fields_and_statements() {
    let dir = tree("cfg", &[(
        "main.vs",
        "@cfg(target_os = \"linux\")\nfunc onLinux() {}\n\
         @cfg(target_os = \"macos\")\nfunc onMac() {}\n\
         @cfg(all(unix, not(windows)))\nfunc onUnix() {}\n\
         @cfg(any(windows, feature = \"extra\"))\nfunc extra() {}\n\
         struct S {\n    a: int64\n    @cfg(false)\n    b: int64\n}\n\
         func f() {\n    @cfg(windows)\n    let x = 1\n    let y = 2\n}\n",
    )]);
    let (krate, errors) = expand(&dir.join("main.vs"), &linux());
    assert_eq!(errors, "");
    assert_eq!(print(&krate.root), vec!["onLinux", "onUnix", "S", "f"]);
    let s = krate.root.items().find(|i| i.kind.ident().unwrap().as_str() == "S").unwrap();
    let vsc_ast::ItemKind::Struct(_, _, data) = &s.kind else { panic!() };
    assert_eq!(data.fields().len(), 1);
    let f = krate.root.items().find(|i| i.kind.ident().unwrap().as_str() == "f").unwrap();
    let vsc_ast::ItemKind::Fn(func) = &f.kind else { panic!() };
    assert_eq!(func.body.as_ref().unwrap().stmts.len(), 1);

    let mut with_feature = linux();
    with_feature.insert("feature", Some("extra"));
    let (krate, _) = expand(&dir.join("main.vs"), &with_feature);
    assert!(print(&krate.root).contains(&"extra".to_string()));
}

#[test]
fn cfg_attr_adds_attributes() {
    let dir = tree("cfgattr", &[(
        "main.vs",
        "@cfgAttr(unix, derive(Debug), inline)\n@cfgAttr(windows, derive(Clone))\nstruct S\n",
    )]);
    let (krate, errors) = expand(&dir.join("main.vs"), &linux());
    assert_eq!(errors, "");
    let item = krate.root.items().next().unwrap();
    let names: Vec<String> = item.attrs.iter().filter_map(|a| a.name()).map(|n| n.to_string()).collect();
    assert_eq!(names, vec!["derive", "inline"]);
}

#[test]
fn malformed_cfg_is_an_error() {
    let dir = tree("badcfg", &[("main.vs", "@cfg(maybe(x))\nfunc f() {}\n@cfg(a, b)\nfunc g() {}\n")]);
    let (_, errors) = expand(&dir.join("main.vs"), &linux());
    assert!(errors.contains("invalid predicate `maybe`"), "{errors}");
    assert!(errors.contains("`cfg` takes one predicate"), "{errors}");
}

#[test]
fn node_ids_are_assigned() {
    let dir = tree("ids", &[("main.vs", "func f(x: int64) -> int64 { x + 1 }\n")]);
    let (krate, _) = expand(&dir.join("main.vs"), &linux());
    let item = krate.root.items().next().unwrap();
    assert_ne!(item.id, vsc_ast::DUMMY_NODE_ID);
    assert_eq!(krate.id.as_u32(), 0);
    assert_eq!(krate.root.id.as_u32(), 1);
}
