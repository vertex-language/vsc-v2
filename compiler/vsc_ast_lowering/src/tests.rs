use std::path::PathBuf;

use vsc_data_structures::{FxHashMap, FxHashSet};
use vsc_errors::SharedBuffer;
use vsc_hir::def::{DefKind, Res};
use vsc_hir::intravisit::{self, Visitor};
use vsc_hir::*;
use vsc_session::ParseSess;
use vsc_session::config::{TargetInfo, default_configuration};
use vsc_span::Span;
use vsc_span::def_id::{CRATE_DEF_ID, LocalDefId};

use crate::lower_to_hir;

struct Lowered {
    krate: Crate,
    defs: Vec<(LocalDefId, DefKind)>,
    diagnostics: String,
}

/// Writes the files to a scratch crate, then expands, resolves and lowers it.
fn lower_files(name: &str, files: &[(&str, &str)]) -> Lowered {
    let dir: PathBuf = std::env::temp_dir().join(format!("vsc-lower-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let buf = SharedBuffer::new();
    let psess = ParseSess::with_buffer(buf.clone());
    let cfg = default_configuration(&TargetInfo::from_triple("x86_64-unknown-linux-gnu").unwrap(), true, false);
    let ecfg = vsc_expand::ExpansionConfig { crate_name: Some("app"), cfg: &cfg };
    let krate = vsc_expand::expand_crate(&psess, &dir.join(files[0].0), &ecfg).unwrap();
    let outputs = vsc_resolve::resolve_crate(&psess, &krate);
    let hir = lower_to_hir(&psess, &krate, &outputs);
    let _ = std::fs::remove_dir_all(&dir);
    let defs = outputs.defs.iter_enumerated().map(|(id, d)| (id, d.kind)).collect();
    Lowered { krate: hir, defs, diagnostics: buf.contents() }
}

fn lower(src: &str) -> Lowered {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    lower_files(&format!("t{:x}", h.finish()), &[("main.vs", src)])
}

// ---------------------------------------------------------------------------
// The validator (rustc's `hir_id_validator`)
// ---------------------------------------------------------------------------

/// Checks the invariants later passes rely on, and returns what broke:
///
/// - every node's id is unique and belongs to the owner it's in;
/// - every owner is reachable from the crate root, and every item-like
///   definition is an owner;
/// - every use of a local names a binding pattern, as does every lifetime
///   naming a parameter;
/// - every `break` and `continue` target is a loop or a labeled block.
fn validate(l: &Lowered) -> Vec<String> {
    let mut v = Validator {
        krate: &l.krate,
        owner: CRATE_DEF_ID,
        seen: FxHashMap::default(),
        visited_owners: FxHashSet::default(),
        bindings: FxHashSet::default(),
        local_uses: Vec::new(),
        targets: FxHashSet::default(),
        destinations: Vec::new(),
        errors: Vec::new(),
    };
    let root = l.krate.owner(CRATE_DEF_ID);
    v.visited_owners.insert(CRATE_DEF_ID);
    if let OwnerNode::Crate(m) = &root.node {
        v.visit_mod(m);
    }
    for (def, info) in l.krate.owners.iter_enumerated() {
        if info.is_some() && !v.visited_owners.contains(&def) {
            v.errors.push(format!("owner {def:?} isn't reachable from the crate root"));
        }
    }
    for &(def, kind) in &l.defs {
        let is_owner_kind = matches!(
            kind,
            DefKind::Mod
                | DefKind::Struct
                | DefKind::Class
                | DefKind::Union
                | DefKind::Enum
                | DefKind::Trait
                | DefKind::TyAlias
                | DefKind::ForeignTy
                | DefKind::Fn
                | DefKind::Const
                | DefKind::Static { .. }
                | DefKind::Impl { .. }
                | DefKind::ForeignMod
                | DefKind::GlobalAsm
                | DefKind::AssocFn
                | DefKind::AssocConst
                | DefKind::AssocTy
        );
        if is_owner_kind && l.krate.owners[def].is_none() {
            v.errors.push(format!("{kind:?} {def:?} has no HIR owner"));
        }
    }
    for (span, id) in std::mem::take(&mut v.local_uses) {
        if !v.bindings.contains(&id) {
            v.errors.push(format!("local use at {span:?} names {id:?}, which isn't a binding"));
        }
    }
    for (span, id) in std::mem::take(&mut v.destinations) {
        if !v.targets.contains(&id) {
            v.errors.push(format!("`break`/`continue` at {span:?} targets {id:?}, which isn't a loop or labeled block"));
        }
    }
    v.errors
}

struct Validator<'hir> {
    krate: &'hir Crate,
    owner: LocalDefId,
    seen: FxHashMap<HirId, ()>,
    visited_owners: FxHashSet<LocalDefId>,
    bindings: FxHashSet<HirId>,
    local_uses: Vec<(Span, HirId)>,
    targets: FxHashSet<HirId>,
    destinations: Vec<(Span, HirId)>,
    errors: Vec<String>,
}

impl<'hir> Validator<'hir> {
    fn in_owner(&mut self, owner: LocalDefId, f: impl FnOnce(&mut Self)) {
        if !self.visited_owners.insert(owner) {
            self.errors.push(format!("owner {owner:?} visited twice"));
        }
        let saved = std::mem::replace(&mut self.owner, owner);
        f(self);
        self.owner = saved;
    }

    fn check_res(&mut self, res: Res<HirId>, span: Span) {
        if let Res::Local(id) = res {
            self.local_uses.push((span, id));
        }
    }
}

impl<'hir> Visitor<'hir> for Validator<'hir> {
    fn krate(&self) -> Option<&'hir Crate> {
        Some(self.krate)
    }

    fn visit_item(&mut self, i: &'hir Item) {
        self.in_owner(i.owner_id, |this| intravisit::walk_item(this, i));
    }

    fn visit_assoc_item(&mut self, i: &'hir AssocItem) {
        self.in_owner(i.owner_id, |this| intravisit::walk_assoc_item(this, i));
    }

    fn visit_foreign_item(&mut self, i: &'hir ForeignItem) {
        self.in_owner(i.owner_id, |this| intravisit::walk_foreign_item(this, i));
    }

    fn visit_id(&mut self, id: HirId) {
        if id.owner != self.owner {
            self.errors.push(format!("{id:?} is inside owner {:?}", self.owner));
        }
        if self.seen.insert(id, ()).is_some() {
            self.errors.push(format!("{id:?} is used by more than one node"));
        }
    }

    fn visit_pat(&mut self, p: &'hir Pat) {
        if let PatKind::Binding(_, id, ..) = p.kind {
            if id != p.hir_id {
                self.errors.push(format!("binding {id:?} isn't its pattern's id {:?}", p.hir_id));
            }
            self.bindings.insert(id);
        }
        intravisit::walk_pat(self, p)
    }

    fn visit_path(&mut self, p: &'hir Path, _id: HirId) {
        self.check_res(p.res, p.span);
        intravisit::walk_path(self, p)
    }

    fn visit_lifetime(&mut self, o: &'hir Lifetime) {
        self.visit_id(o.hir_id)
    }

    fn visit_expr(&mut self, e: &'hir Expr) {
        match &e.kind {
            ExprKind::Loop(..) | ExprKind::Block(_, Some(_)) => {
                self.targets.insert(e.hir_id);
            }
            ExprKind::Break(d, _) | ExprKind::Continue(d) => {
                if let Some(t) = d.target_id {
                    self.destinations.push((e.span, t));
                }
            }
            _ => {}
        }
        intravisit::walk_expr(self, e)
    }
}

#[track_caller]
fn assert_valid(l: &Lowered) {
    let errors = validate(l);
    assert!(errors.is_empty(), "invalid HIR:\n{}\n\nHIR:\n{}", errors.join("\n"), vsc_hir_pretty_lite(&l.krate));
}

/// The HIR's debug form, trimmed, for failure messages.
fn vsc_hir_pretty_lite(k: &Crate) -> String {
    let s = format!("{k:#?}");
    s.lines().take(400).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------
// Whole programs
// ---------------------------------------------------------------------------

#[test]
fn ui_lowering_tests_are_valid() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/ui/lowering");
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "vs") {
            let src = std::fs::read_to_string(&path).unwrap();
            let l = lower(&src);
            let errors = validate(&l);
            assert!(errors.is_empty(), "{}: invalid HIR:\n{}", path.display(), errors.join("\n"));
            n += 1;
        }
    }
    assert!(n >= 6, "found only {n} tests");
}

/// The code blocks marked `// Vertex` in usage.md, as files: snippets that
/// are statements get wrapped in a function. Resolution errors (the
/// standard library isn't built yet) are expected; invalid HIR isn't.
#[test]
fn usage_snippets_lower_to_valid_hir() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/doc/reference/usage.md");
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines();
    let mut n = 0;
    while let Some(line) = lines.next() {
        if line.trim() != "```rust" {
            continue;
        }
        let body: Vec<&str> = lines.by_ref().take_while(|l| l.trim() != "```").collect();
        if !body.first().is_some_and(|l| l.trim_start().starts_with("// Vertex")) {
            continue;
        }
        let src = body[1..].join("\n");
        let (mut header, mut rest) = (String::new(), String::new());
        let mut in_header = true;
        for l in &body[1..] {
            let t = l.trim();
            if in_header && (t.is_empty() || t.starts_with("package ") || t.starts_with("import") || t == ")" || t.starts_with('"')) {
                header.push_str(l);
                header.push('\n');
            } else {
                in_header = false;
                rest.push_str(l);
                rest.push('\n');
            }
        }
        let wrapped = format!("{header}func snippetBody() {{\n{rest}}}\n");
        for candidate in [&src, &wrapped] {
            let l = lower(candidate);
            let errors = validate(&l);
            assert!(errors.is_empty(), "usage snippet:\n{candidate}\ninvalid HIR:\n{}", errors.join("\n"));
        }
        n += 1;
    }
    assert!(n >= 39, "found only {n} snippets");
}

// ---------------------------------------------------------------------------
// Loops and labels
// ---------------------------------------------------------------------------

/// Labels → loop ids, and each `break`/`continue`'s label and target.
#[derive(Default)]
struct Loops {
    labeled: FxHashMap<String, HirId>,
    unlabeled_loops: Vec<HirId>,
    jumps: Vec<(Option<String>, Option<HirId>)>,
}

struct LoopVisitor<'hir>(Loops, &'hir Crate);

impl<'hir> Visitor<'hir> for LoopVisitor<'hir> {
    fn krate(&self) -> Option<&'hir Crate> {
        Some(self.1)
    }
    fn visit_expr(&mut self, e: &'hir Expr) {
        match &e.kind {
            ExprKind::Loop(_, label, ..) | ExprKind::Block(_, label @ Some(_)) => match label {
                Some(l) => {
                    self.0.labeled.insert(l.ident.to_string(), e.hir_id);
                }
                None => self.0.unlabeled_loops.push(e.hir_id),
            },
            ExprKind::Break(d, _) | ExprKind::Continue(d) => {
                self.0.jumps.push((d.label.map(|l| l.ident.to_string()), d.target_id));
            }
            _ => {}
        }
        intravisit::walk_expr(self, e)
    }
}

fn loops(l: &Lowered) -> Loops {
    let mut v = LoopVisitor(Loops::default(), &l.krate);
    if let OwnerNode::Crate(m) = &l.krate.owner(CRATE_DEF_ID).node {
        v.visit_mod(m);
    }
    v.0
}

#[test]
fn labeled_jumps_target_their_loop() {
    let l = lower(
        "func main(xs: &[int64]) {
    outer: for x in xs {
        for y in xs {
            if *y > *x { continue outer }
            if *y == 0 { break outer }
        }
    }
    let v = b: { break b 1 }
}",
    );
    assert_valid(&l);
    assert!(l.diagnostics.is_empty(), "{}", l.diagnostics);
    let info = loops(&l);
    let outer = info.labeled["outer"];
    let b = info.labeled["b"];
    let labeled: Vec<_> = info.jumps.iter().filter(|(label, _)| label.is_some()).collect();
    assert_eq!(labeled.len(), 3);
    for (label, target) in labeled {
        let want = if label.as_deref() == Some("b") { b } else { outer };
        assert_eq!(*target, Some(want), "{label:?}");
    }
}

#[test]
fn unlabeled_break_targets_innermost_loop() {
    let l = lower(
        "func main(xs: &[int64]) {
    for x in xs {
        while true {
            break
        }
        break
    }
}",
    );
    assert_valid(&l);
    let info = loops(&l);
    // Desugared loops are unlabeled: the `for`'s, then the `while`'s. Each
    // also has the desugaring's own `break`.
    let [for_loop, while_loop] = info.unlabeled_loops[..] else { panic!("{:?}", info.unlabeled_loops) };
    let targets: Vec<_> = info.jumps.iter().map(|(_, t)| t.unwrap()).collect();
    assert_eq!(targets.iter().filter(|&&t| t == while_loop).count(), 2, "the `while`'s own break and the written one");
    assert_eq!(targets.iter().filter(|&&t| t == for_loop).count(), 2, "the `for`'s `None => break` and the written one");
}

#[test]
fn function_literals_start_a_new_loop_scope() {
    let l = lower(
        "func main() {
    loop {
        let f = func() { break }
    }
}",
    );
    assert_valid(&l);
    let info = loops(&l);
    // The `break` inside the literal can't leave the outer loop; a later
    // pass reports it.
    assert_eq!(info.jumps, vec![(None, None)]);
}

// ---------------------------------------------------------------------------
// Owners and resolutions
// ---------------------------------------------------------------------------

#[test]
fn nested_items_are_owners() {
    let l = lower(
        "func outer() -> int64 {
    func inner(x: int64) -> int64 { x * 2 }
    struct Local { v: int64 }
    inner(Local { v: 1 }.v)
}",
    );
    assert_valid(&l);
    assert!(l.diagnostics.is_empty(), "{}", l.diagnostics);
    let owners = l.krate.owners.iter().filter(|o| o.is_some()).count();
    // The crate root, `outer`, `inner` and `Local`.
    assert_eq!(owners, 4);
}

#[test]
fn or_pattern_uses_name_the_first_alternative() {
    let l = lower(
        "enum E { A(int64); B(int64) }
func f(e: E) -> int64 {
    match e {
        E.A(n) | E.B(n) => n
    }
}",
    );
    assert_valid(&l);
    assert!(l.diagnostics.is_empty(), "{}", l.diagnostics);
}

#[test]
fn flat_receiver_params_become_generics() {
    let l = lower(
        "struct Stack<T> { items: [T; 4] }
func (s: &Stack<T>) Peek() -> &T { &s.items[0] }",
    );
    assert_valid(&l);
    assert!(l.diagnostics.is_empty(), "{}", l.diagnostics);
    let imp = l
        .krate
        .owners
        .iter()
        .flatten()
        .find_map(|o| match &o.node {
            OwnerNode::Item(Item { kind: ItemKind::Impl(i), .. }) => Some(i),
            _ => None,
        })
        .expect("an impl");
    assert!(imp.flat_receiver);
    assert_eq!(imp.generics.params.len(), 1);
    let p = &imp.generics.params[0];
    assert!(p.from_flat_receiver && matches!(p.kind, GenericParamKind::Type { .. }));
    assert_eq!(p.name.as_str(), "T");
}

#[test]
fn lowering_errors() {
    for (src, want) in [
        ("func f() { let (a, .., b, ..) = (1, 2, 3) }", "`..` can only be used once per tuple pattern"),
        ("func f() { let [a, .., b, ..] = [1, 2, 3] }", "`..` can only be used once per slice pattern"),
        ("func f() { let (a, rest @ ..) = (1, 2) }", "`rest @` is not allowed in a tuple"),
        ("func f() { let x = _ }", "`_` can only be used on the left-hand side of an assignment"),
        ("type A", "type alias is missing its type"),
    ] {
        let l = lower(src);
        assert!(l.diagnostics.contains(want), "for {src}: expected {want:?}, got:\n{}", l.diagnostics);
        assert_valid(&l);
    }
}
