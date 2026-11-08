use std::path::PathBuf;

use vsc_errors::SharedBuffer;
use vsc_session::ParseSess;
use vsc_session::config::{TargetInfo, default_configuration};

use crate::dump::dump_resolutions;
use crate::resolve_crate;

/// Writes the files to a scratch crate, expands and resolves it. Returns the
/// resolution dump and the diagnostics.
fn resolve(name: &str, files: &[(&str, &str)]) -> (String, String) {
    let dir: PathBuf = std::env::temp_dir().join(format!("vsc-resolve-{}-{name}", std::process::id()));
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
    let out = resolve_crate(&psess, &krate);
    (dump_resolutions(&krate, &out, &psess.source_map), buf.contents())
}

fn one(src: &str) -> (String, String) {
    resolve(&format!("t{:x}", hash(src)), &[("main.vs", src)])
}

fn hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[track_caller]
fn has(dump: &str, line: &str) {
    assert!(dump.lines().any(|l| l == line), "missing line {line:?} in:\n{dump}");
}

#[track_caller]
fn no_errors(errors: &str) {
    assert!(errors.is_empty(), "unexpected errors:\n{errors}");
}

#[test]
fn locals_items_and_paths() {
    let (dump, errors) = one(
        "package main\n\
         struct Task { id: uint64; done: bool }\n\
         enum Status { Todo; Done }\n\
         implement Task {\n\
         \x20   func New(id: uint64) -> Task { Task { id, done: false } }\n\
         \x20   func IsOpen(&self) -> bool { !self.done }\n\
         }\n\
         func main() {\n\
         \x20   var t = Task.New(1)\n\
         \x20   let s = Status.Done\n\
         \x20   let n = t.items.Len()\n\
         \x20   println(\"{}\", n)\n\
         }\n",
    );
    no_errors(&errors);
    has(&dump, "2:19 type `uint64` -> builtin type `uint64`");
    has(&dump, "5:29 type `Task` -> struct `app.Task`");
    has(&dump, "5:36 struct `Task` -> struct `app.Task`");
    has(&dump, "5:43 value `id` -> local #19");
    has(&dump, "6:35 value `self.done` -> local #35 + `done`");
    has(&dump, "9:13 value `Task.New` -> struct `app.Task` + `New`");
    has(&dump, "10:13 value `Status.Done` -> unit variant `app.Status.Done.{unit variant}`");
    has(&dump, "11:13 value `t.items.Len` -> local #50 + `items.Len`");
    // `println`'s arguments are resolved like any others.
    assert!(dump.lines().any(|l| l.starts_with("12:19 value `n` -> local #")), "{dump}");
}

#[test]
fn packages_and_imports() {
    let (dump, errors) = resolve(
        "pkgs",
        &[
            (
                "main.vs",
                "package main\nimport (\n    geo \"./geometry\"\n    \"app/geometry/shapes\"\n)\n\
                 func main() {\n    let p = geo.Zero()\n    let c = shapes.Circle { r: 1.0 }\n    let x = geo.secret\n}\n",
            ),
            ("geometry/point.vs", "package geometry\npublic func Zero() -> int64 { 0 }\nconst secret: int64 = 1\n"),
            ("geometry/shapes/circle.vs", "package shapes\npublic struct Circle { public r: float64 }\n"),
        ],
    );
    has(&dump, "7:13 value `geo.Zero` -> function `app.geometry.Zero`");
    has(&dump, "8:13 struct `shapes.Circle` -> struct `app.geometry.shapes.Circle`");
    assert!(errors.contains("error[E0603]: constant `secret` is private"), "{errors}");
}

#[test]
fn imports_are_per_file() {
    let (_, errors) = resolve(
        "perfile",
        &[
            ("main.vs", "package main\nimport geo \"./geometry\"\nfunc a() -> int64 { geo.Zero() }\n"),
            ("other.vs", "package main\nfunc b() -> int64 { geo.Zero() }\n"),
            ("geometry/g.vs", "package geometry\npublic func Zero() -> int64 { 0 }\n"),
        ],
    );
    assert!(errors.contains("failed to resolve: use of undeclared type or package `geo`"), "{errors}");
    assert!(errors.contains("--> "), "{errors}");
    assert_eq!(errors.matches("error[E0433]").count(), 1, "{errors}");
}

#[test]
fn missing_crates() {
    let (_, errors) = one("package main\nimport (\n    \"std/fs\"\n    \"std/io\"\n    \"net/http\"\n    \"./nothere\"\n)\n");
    assert_eq!(errors.matches("error[E0463]: can't find crate for `std`").count(), 1, "{errors}");
    assert!(errors.contains("vsc-v2 can't build the standard library yet"), "{errors}");
    assert!(errors.contains("error[E0463]: can't find crate for `net`"), "{errors}");
    assert!(errors.contains("error[E0432]: unresolved import `./nothere`"), "{errors}");
}

#[test]
fn not_found_errors() {
    let (_, errors) = one(
        "func f(x: i64) -> Vec<int64> {\n    let a = missing\n    let b = Option.None\n    Task {}\n}\n",
    );
    assert!(errors.contains("error[E0412]: cannot find type `i64` in this scope"), "{errors}");
    assert!(errors.contains("Vertex writes `i64` as `int64`"), "{errors}");
    assert!(errors.contains("error[E0412]: cannot find type `Vec` in this scope"), "{errors}");
    assert!(errors.contains("`Vec` comes from the standard library"), "{errors}");
    assert!(errors.contains("error[E0425]: cannot find value `missing` in this scope"), "{errors}");
    assert!(errors.contains("error[E0433]: failed to resolve: use of undeclared type or package `Option`"), "{errors}");
    assert!(errors.contains("error[E0422]: cannot find struct, variant or union type `Task` in this scope"), "{errors}");
}

#[test]
fn wrong_kind_errors() {
    let (_, errors) = one(
        "struct Point { x: int64 }\ntrait Shape {}\nfunc f<T>(s: Shape) {\n    let p = Point\n    let t = T\n}\n",
    );
    assert!(errors.contains("error[E0573]: expected type, found trait `Shape`"), "{errors}");
    assert!(errors.contains("error[E0423]: expected value, found struct `Point`"), "{errors}");
    assert!(errors.contains("use struct literal syntax: `Point { … }`"), "{errors}");
    assert!(errors.contains("error[E0423]: expected value, found type parameter `T`"), "{errors}");
}

#[test]
fn duplicates() {
    let (_, errors) = resolve(
        "dups",
        &[
            ("main.vs", "import geometry \"./geometry\"\nfunc f() {}\nstruct geometry {}\n"),
            ("other.vs", "func f() {}\n"),
            ("geometry/g.vs", "package geometry\n"),
        ],
    );
    assert!(errors.contains("error[E0428]: the name `f` is defined multiple times"), "{errors}");
    assert!(errors.contains("error[E0255]: the name `geometry` is defined multiple times"), "{errors}");
}

#[test]
fn patterns() {
    let (dump, errors) = one(
        "const Limit: int64 = 3\nenum Status { Todo; Done; Other(int64) }\n\
         func f(s: Status, n: int64) {\n\
         \x20   match s {\n\
         \x20       Status.Todo => {}\n\
         \x20       Status.Other(Limit) => {}\n\
         \x20       Status.Other(x) | Status.Other(x) => {}\n\
         \x20       _ => {}\n\
         \x20   }\n\
         \x20   let (a, var b) = (1, 2)\n\
         }\n",
    );
    no_errors(&errors);
    has(&dump, "5:9 pattern `Status.Todo` -> unit variant `app.Status.Todo.{unit variant}`");
    has(&dump, "6:22 pattern `Limit` -> constant `app.Limit`");
    has(&dump, "6:9 pattern `Status.Other` -> tuple variant `app.Status.Other.{tuple variant}`");

    let (_, errors) = one(
        "struct Meters(float64)\nfunc f() {\n    let (x, x) = (1, 2)\n    let Meters = 1\n    match 1 { y | 2 => {} }\n}\n",
    );
    assert!(errors.contains("error[E0416]: identifier `x` is bound more than once in the same pattern"), "{errors}");
    assert!(errors.contains("error[E0530]: bindings cannot shadow tuple structs"), "{errors}");
    assert!(errors.contains("error[E0408]: variable `y` is not bound in all patterns"), "{errors}");
}

#[test]
fn shadowing_and_scopes() {
    let (dump, errors) = one(
        "func f(x: int64) -> int64 {\n    let x = x * 2\n    if let y = x {\n        return y\n    }\n    x\n}\n",
    );
    no_errors(&errors);
    // The new `x` sees the parameter; later uses see the new `x`.
    has(&dump, "2:13 value `x` -> local #4");
    has(&dump, "6:5 value `x` -> local #12");
    let (_, errors) = one("func f() {\n    if let y = 1 {}\n    y\n}\n");
    assert!(errors.contains("error[E0425]: cannot find value `y` in this scope"), "{errors}");
}

#[test]
fn origins() {
    let (dump, errors) = one(
        "struct Highlight<src: Origin> {\n    text: &[src] string\n}\n\
         implement<src: Origin> Highlight<src> {\n    func Part(&self) -> &[src] string { self.text }\n}\n\
         func longest(a: &string, b: &string) -> &[a, b] string { a }\n\
         const G: &[static] string = \"hi\"\n\
         func bad() -> &[nope] string { \"\" }\n",
    );
    has(&dump, "2:13 origin `src` -> origin parameter `app.Highlight.src`");
    has(&dump, "4:34 origin `src` -> origin parameter `app.{implementation}.src`");
    has(&dump, "7:43 origin `a` -> parameter #29");
    has(&dump, "7:46 origin `b` -> parameter #34");
    has(&dump, "8:12 origin `static` -> static");
    assert!(errors.contains("error[E0261]: use of undeclared origin `nope`"), "{errors}");
    assert_eq!(errors.matches("error").count(), 1, "only the undeclared origin is an error:\n{errors}");
}

#[test]
fn flat_receivers() {
    let (dump, errors) = one(
        "struct Stack<T> { items: [T; 4] }\nstruct Highlight<src: Origin> { text: &[src] string }\n\
         func (s: &mut Stack<T>) Push(item: T) {}\n\
         func (h: &Highlight<src>) Part() -> &[src] string { h.text }\n\
         func (t: &Stack<T>) Bad() { self.items }\n",
    );
    has(&dump, "3:36 type `T` -> type parameter `app.{implementation}.T`");
    has(&dump, "4:39 origin `src` -> origin parameter `app.{implementation}.src`");
    has(&dump, "4:53 value `h.text` -> local #41 + `text`");
    assert!(errors.contains("error[E0424]: expected value, found package `self`"), "{errors}");
    assert!(errors.contains("in a flat method, the receiver has the name you gave it"), "{errors}");
}

#[test]
fn self_types() {
    let (dump, errors) = one(
        "trait Shape { func Area(&self) -> float64; func Unit() -> Self }\n\
         struct P(int64)\nimplement P { func Make() -> Self { Self(1) } }\n\
         func f() -> Self {}\n",
    );
    has(&dump, "1:59 type `Self` -> `Self` of trait `app.Shape`");
    has(&dump, "3:30 type `Self` -> `Self` of `app.{implementation}`");
    has(&dump, "3:37 value `Self` -> `Self` constructor of `app.{implementation}`");
    assert!(errors.contains("error[E0411]: cannot find type `Self` in this scope"), "{errors}");
}

#[test]
fn item_barriers_and_labels() {
    let (_, errors) = one(
        "func outer<T>(x: int64) {\n    func inner() -> int64 { x }\n    func generic(v: T) {}\n    'a: loop { break 'b }\n    let f = func() { break 'a }\n}\n",
    );
    assert!(errors.contains("error[E0434]: can't capture dynamic environment in a function item"), "{errors}");
    assert!(errors.contains("error[E0401]: can't use generic parameters from outer item"), "{errors}");
    assert!(errors.contains("error[E0426]: use of undeclared label `'b`"), "{errors}");
    assert!(errors.contains("error[E0426]: use of undeclared label `'a`"), "{errors}");
}

#[test]
fn block_items_are_hoisted() {
    let (dump, errors) = one("func f() -> int64 {\n    let v = helper()\n    func helper() -> int64 { 1 }\n    v\n}\n");
    no_errors(&errors);
    has(&dump, "2:13 value `helper` -> function `app.f.helper`");
}

#[test]
fn builtin_names_are_reserved() {
    let (_, errors) = one("func println() {}\nfunc f() { let vec = 1 }\n");
    assert!(errors.contains("`println` is a built-in name and can't be declared"), "{errors}");
    assert!(errors.contains("`vec` is a built-in name and can't be declared"), "{errors}");
}

#[test]
fn guide_program_resolves_except_std() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/ui/parser/complete-program.vs");
    let src = std::fs::read_to_string(path).unwrap();
    let (_, errors) = one(&src);
    // Without the standard library, the remaining errors are all std names.
    for line in errors.lines().filter(|l| l.starts_with("error")) {
        let ok = line.contains("can't find crate")
            || ["Vec", "Option", "Result", "Ok", "Err", "Some", "None", "From", "Debug", "Clone", "Copy", "PartialEq"]
                .iter()
                .any(|n| line.contains(&format!("`{n}`")))
            || line.starts_with("error: aborting");
        assert!(ok, "unexpected error: {line}\n\nall errors:\n{errors}");
    }
}
