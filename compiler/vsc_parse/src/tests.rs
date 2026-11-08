use vsc_ast_pretty::source_file_to_string;
use vsc_errors::SharedBuffer;
use vsc_session::ParseSess;
use vsc_span::FileName;

use crate::parse_source_str;

/// Parses `src`; returns the printed file and the diagnostics.
fn parse(src: &str) -> (String, String) {
    let buf = SharedBuffer::new();
    let psess = ParseSess::with_buffer(buf.clone());
    let file = parse_source_str(&psess, FileName::Custom("test.vs".into()), src.to_string());
    (source_file_to_string(&file), buf.contents())
}

/// Parses `src` with no errors, prints it, and checks the print parses back
/// to the same print.
#[track_caller]
fn round_trip(src: &str) -> String {
    let (printed, errors) = parse(src);
    assert!(errors.is_empty(), "unexpected errors for:\n{src}\n---\n{errors}");
    let (again, errors) = parse(&printed);
    assert!(errors.is_empty(), "printed source doesn't parse:\n{printed}\n---\n{errors}");
    assert_eq!(printed, again, "printing isn't stable");
    printed
}

#[track_caller]
fn assert_prints(src: &str, expected: &str) {
    assert_eq!(round_trip(src), expected);
}

#[track_caller]
fn assert_error(src: &str, expected_fragment: &str) {
    let (_, errors) = parse(src);
    assert!(errors.contains(expected_fragment), "expected an error containing {expected_fragment:?}, got:\n{errors}");
}

// ---------------------------------------------------------------------------
// Files, packages and imports (exception 1)
// ---------------------------------------------------------------------------

#[test]
fn package_and_imports() {
    assert_prints(
        "package main\n\nimport \"std/fs\"\n\nfunc main() {}\n",
        "package main\n\nimport \"std/fs\"\n\nfunc main() {}\n",
    );
    assert_prints(
        "package main\nimport (\n    \"std/collections\"\n    geo \"./geometry\"\n)\nfunc main() {}",
        "package main\n\nimport (\n    \"std/collections\"\n    geo \"./geometry\"\n)\n\nfunc main() {}\n",
    );
}

#[test]
fn package_attributes() {
    assert_prints("@allow(deadCode)\npackage main\n", "@allow(deadCode)\npackage main\n");
}

#[test]
fn package_visibility_is_not_a_declaration() {
    assert_prints("package func helper() {}\n", "package func helper() {}\n");
}

#[test]
fn import_after_items_is_an_error() {
    assert_error("func a() {}\nimport \"std/fs\"\n", "imports must come before the file's items");
}

// ---------------------------------------------------------------------------
// Line breaks (exception 10)
// ---------------------------------------------------------------------------

#[test]
fn statements_end_at_line_breaks() {
    assert_prints(
        "func f() {\n    let a = 1\n    var b = 2\n    b += a\n}\n",
        "func f() {\n    let a = 1\n    var b = 2\n    b += a\n}\n",
    );
}

#[test]
fn semicolons_share_a_line() {
    assert_prints("func f() { let a = 1; let b = 2 }\n", "func f() {\n    let a = 1\n    let b = 2\n}\n");
}

#[test]
fn method_chains_continue_on_dot() {
    assert_prints(
        "func f() {\n    let total = orders\n        .Iter()\n        .Filter(func(o) { o.paid })\n        .Sum()\n}\n",
        "func f() {\n    let total = orders.Iter().Filter(func(o) { o.paid }).Sum()\n}\n",
    );
}

#[test]
fn binary_operators_continue_when_followed_by_space() {
    assert_prints(
        "func f() {\n    let ok = hasName\n        && hasEmail\n        && age >= 18\n}\n",
        "func f() {\n    let ok = hasName && hasEmail && age >= 18\n}\n",
    );
    // `-x` on its own line is a new statement.
    assert_prints("func f() {\n    let a = b\n    -x\n}\n", "func f() {\n    let a = b\n    -x\n}\n");
    // `- x` continues the previous line.
    assert_prints("func f() {\n    let a = b\n        - x\n}\n", "func f() {\n    let a = b - x\n}\n");
}

#[test]
fn line_feeds_inside_parens_are_whitespace() {
    assert_prints(
        "func f() {\n    g(\n        1,\n        2,\n    )\n}\n",
        "func f() {\n    g(1, 2)\n}\n",
    );
}

#[test]
fn call_on_next_line_is_a_new_statement() {
    assert_prints("func f() {\n    a\n    (b)\n}\n", "func f() {\n    a\n    (b)\n}\n");
}

#[test]
fn else_continues_on_next_line() {
    assert_prints(
        "func f() {\n    if a {\n        b()\n    }\n    else {\n        c()\n    }\n}\n",
        "func f() {\n    if a {\n        b()\n    } else {\n        c()\n    }\n}\n",
    );
}

#[test]
fn block_like_statements_need_no_separator() {
    assert_prints("func f() { if a { b() } c() }\n", "func f() {\n    if a {\n        b()\n    }\n    c()\n}\n");
}

#[test]
fn missing_separator_is_an_error() {
    assert_error("func f() { let a = 1 let b = 2 }\n", "expected `;` or a line break, found keyword `let`");
}

#[test]
fn brace_on_next_line_after_declaration_header() {
    assert_prints("func f()\n{\n    g()\n}\n", "func f() {\n    g()\n}\n");
    assert_prints("struct S\n{\n    x: int64\n}\n", "struct S {\n    x: int64\n}\n");
}

#[test]
fn return_value_must_be_on_the_same_line() {
    assert_prints(
        "func f() {\n    return\n    g()\n}\n",
        "func f() {\n    return\n    g()\n}\n",
    );
}

#[test]
fn comment_lines_dont_count() {
    assert_prints(
        "func f() {\n    let a = 1 // one\n\n    // a comment\n    let b = 2 /* two\n    lines */ let c = 3\n}\n",
        "func f() {\n    let a = 1\n    let b = 2\n    let c = 3\n}\n",
    );
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

#[test]
fn structs_fields_on_their_own_lines() {
    assert_prints(
        "@derive(Debug, Clone, PartialEq)\nstruct Task {\n    id: uint64\n    title: string\n    done: bool\n}\n",
        "@derive(Debug, Clone, PartialEq)\nstruct Task {\n    id: uint64\n    title: string\n    done: bool\n}\n",
    );
    assert_prints("struct Point { x: float64; y: float64 }\n", "struct Point {\n    x: float64\n    y: float64\n}\n");
    assert_prints("struct Meters(float64)\nstruct Marker\n", "struct Meters(float64)\n\nstruct Marker\n");
}

#[test]
fn rust_style_commas_in_fields_are_reported() {
    assert_error("struct P { x: float64, y: float64 }\n", "struct fields are separated by line breaks, not `,`");
}

#[test]
fn enums() {
    assert_prints(
        "enum Shape {\n    Circle(float64)\n    Rect { w: float64; h: float64 }\n}\n",
        "enum Shape {\n    Circle(float64)\n    Rect { w: float64; h: float64 }\n}\n",
    );
    assert_prints("@repr(int32)\nenum Whence { Set = 0; Cur = 1; End = 2 }\n", "@repr(int32)\nenum Whence {\n    Set = 0\n    Cur = 1\n    End = 2\n}\n");
}

#[test]
fn classes() {
    assert_prints(
        "class Store {\n    tasks: collections.HashMap<uint64, string>\n    nextID: uint64\n}\n",
        "class Store {\n    tasks: collections.HashMap<uint64, string>\n    nextID: uint64\n}\n",
    );
}

#[test]
fn implement_blocks_and_traits() {
    assert_prints(
        "implement Task {\n    public func New(id: uint64) -> Task { Task { id, done: false } }\n    public func IsOpen(&self) -> bool { !self.done }\n}\n",
        "implement Task {\n    public func New(id: uint64) -> Task {\n        Task { id, done: false }\n    }\n\n    public func IsOpen(&self) -> bool {\n        !self.done\n    }\n}\n",
    );
    assert_prints(
        "trait Shape {\n    func Area(&self) -> float64\n    func Name(&self) -> string { \"shape\".ToString() }\n}\n",
        "trait Shape {\n    func Area(&self) -> float64\n\n    func Name(&self) -> string {\n        \"shape\".ToString()\n    }\n}\n",
    );
    assert_prints(
        "implement<T: fmt.Display> Stack<T> {}\nimplement ops.Add for Point {\n    type Output = Point\n}\n",
        "implement<T: fmt.Display> Stack<T> {}\n\nimplement ops.Add for Point {\n    type Output = Point\n}\n",
    );
}

#[test]
fn flat_receivers() {
    assert_prints(
        "public func (t: &Task) IsOpen() -> bool { !t.done }\nfunc (s: &mut Stack<T>) Push(item: T) { s.items.Push(item) }\n",
        "public func (t: &Task) IsOpen() -> bool {\n    !t.done\n}\n\nfunc (s: &mut Stack<T>) Push(item: T) {\n    s.items.Push(item)\n}\n",
    );
}

#[test]
fn flat_receiver_introduces_parameters() {
    let buf = SharedBuffer::new();
    let psess = ParseSess::with_buffer(buf.clone());
    let file = parse_source_str(&psess, FileName::Custom("t.vs".into()), "func (s: &mut Stack<T>) Push(item: T) {}".into());
    let vsc_ast::ItemKind::Impl(i) = &file.items[0].kind else { panic!("expected an impl") };
    let vsc_ast::ImplOrigin::FlatReceiver { introduced_params } = &i.origin else { panic!() };
    assert_eq!(introduced_params.iter().map(|p| p.as_str()).collect::<Vec<_>>(), vec!["T"]);
    assert!(i.items[0].kind.is_fn_with_flat_receiver());
}

trait AssocExt {
    fn is_fn_with_flat_receiver(&self) -> bool;
}

impl AssocExt for vsc_ast::AssocItemKind {
    fn is_fn_with_flat_receiver(&self) -> bool {
        matches!(self, vsc_ast::AssocItemKind::Fn(f) if f.sig.decl.inputs[0].flat_receiver)
    }
}

#[test]
fn self_params() {
    assert_prints(
        "implement T {\n    func a(self) {}\n    func b(var self) {}\n    func c(&mut self) {}\n    func d(&[a] self) {}\n    func e(self: Box<Self>) {}\n}\n",
        "implement T {\n    func a(self) {}\n\n    func b(var self) {}\n\n    func c(&mut self) {}\n\n    func d(&[a] self) {}\n\n    func e(self: Box<Self>) {}\n}\n",
    );
}

#[test]
fn extern_blocks() {
    assert_prints(
        "@link(name = \"c\")\nunsafe extern \"C\" {\n    safe func abs(n: c_int) -> c_int\n    func printf(fmt: *const c_char, ...) -> c_int\n    type FILE\n}\n",
        "@link(name = \"c\")\nunsafe extern \"C\" {\n    safe func abs(n: c_int) -> c_int\n    func printf(fmt: *const c_char, ...) -> c_int\n    type FILE\n}\n",
    );
    assert_error("extern \"C\" { func f() }\n", "extern blocks must be unsafe");
}

#[test]
fn consts_statics_and_aliases() {
    assert_prints(
        "public const MaxUsers: uint = 100\nstatic var Counter: int64 = 0\ntype Compare = unsafe extern \"C\" func(*const c_void, *const c_void) -> c_int\n",
        "public const MaxUsers: uint = 100\n\nstatic var Counter: int64 = 0\n\ntype Compare = unsafe extern \"C\" func(*const c_void, *const c_void) -> c_int\n",
    );
}

#[test]
fn rust_spellings_are_reported() {
    assert_error("fn main() {}\n", "functions are declared with `func` in Vertex");
    assert_error("pub func f() {}\n", "visibility is written `public` in Vertex");
    assert_error("func f() { let mut x = 1 }\n", "mutable bindings are declared with `var` in Vertex");
    assert_error("#[derive(Debug)]\nstruct S\n", "attributes are written `@name` in Vertex, not `#[name]`");
    assert_error("func f() { let x = a::b }\n", "paths are separated by `.` in Vertex, not `::`");
    assert_error("func f() -> &'static string {}\n", "Vertex has no lifetimes");
    assert_error("func f() { let g = move |x| x }\n", "Vertex has no `move`");
}

// ---------------------------------------------------------------------------
// Types, origins (exception 13) and function types (exception 14)
// ---------------------------------------------------------------------------

#[test]
fn origins_and_slices() {
    assert_prints(
        "func longest(a: &string, b: &string) -> &[a, b] string { a }\nfunc part(v: &[uint8]) -> &[static] string { \"x\" }\n",
        "func longest(a: &string, b: &string) -> &[a, b] string {\n    a\n}\n\nfunc part(v: &[uint8]) -> &[static] string {\n    \"x\"\n}\n",
    );
    assert_prints(
        "struct Highlight<src: Origin> {\n    text: &[src] string\n}\n",
        "struct Highlight<src: Origin> {\n    text: &[src] string\n}\n",
    );
    assert_prints("func f<a: Origin + [b], b: Origin>() {}\n", "func f<a: Origin + [b], b: Origin>() {}\n");
}

#[test]
fn function_types() {
    assert_prints(
        "struct Button {\n    onClick: func(uint32)\n    check: func(&Task) -> bool\n    cb: unsafe extern \"C\" func(*const c_void) -> c_int\n}\n",
        "struct Button {\n    onClick: func(uint32)\n    check: func(&Task) -> bool\n    cb: unsafe extern \"C\" func(*const c_void) -> c_int\n}\n",
    );
}

#[test]
fn generics_where_and_bounds() {
    assert_prints(
        "func describe<T>(value: T) -> string\nwhere\n    T: fmt.Debug,\n{\n    format(\"{value:?}\")\n}\n",
        "func describe<T>(value: T) -> string where T: fmt.Debug {\n    format(\"{value:?}\")\n}\n",
    );
    assert_prints(
        "func apply<F: Fn(int64) -> int64>(f: F) -> impl Fn(int32) -> int32 {}\n",
        "func apply<F: Fn(int64) -> int64>(f: F) -> impl Fn(int32) -> int32 {}\n",
    );
    assert_prints("func f(v: Vec<Vec<int32>>) {}\n", "func f(v: Vec<Vec<int32>>) {}\n");
}

#[test]
fn never_type() {
    assert_prints("func fail() -> never { panic(\"stop\") }\n", "func fail() -> never {\n    panic(\"stop\")\n}\n");
}

// ---------------------------------------------------------------------------
// Expressions and patterns
// ---------------------------------------------------------------------------

#[test]
fn function_literals() {
    assert_prints(
        "func f() {\n    let add = func(a, b) { a + b }\n    let shout = func(s: &string) -> string { s.ToUppercase() }\n    server.Route(\"/\", async func(req) { handle(req).await })\n}\n",
        "func f() {\n    let add = func(a, b) { a + b }\n    let shout = func(s: &string) -> string { s.ToUppercase() }\n    server.Route(\"/\", async func(req) { handle(req).await })\n}\n",
    );
}

#[test]
fn match_arms_on_lines() {
    assert_prints(
        "func f() {\n    match e {\n        Event.Quit => \"quit\".ToString()\n        Event.Key(c) if c.IsAsciiDigit() => format(\"digit {c}\")\n        Event.Click { x, y } => format(\"{x},{y}\")\n        small @ 1..=9 => \"small\"\n        _ => \"many\"\n    }\n}\n",
        "func f() {\n    match e {\n        Event.Quit => \"quit\".ToString()\n        Event.Key(c) if c.IsAsciiDigit() => format(\"digit {c}\")\n        Event.Click { x, y } => format(\"{x},{y}\")\n        small @ 1..=9 => \"small\"\n        _ => \"many\"\n    }\n}\n",
    );
    assert_error("func f() { match x { A => 1, B => 2 } }\n", "match arms are separated by line breaks, not `,`");
}

#[test]
fn vec_and_arrays() {
    assert_prints(
        "func f() {\n    let a = vec[1, 2, 3]\n    let b = vec[0; n]\n    let c = vec[]\n    let d = [[0; 3]; 2]\n}\n",
        "func f() {\n    let a = vec[1, 2, 3]\n    let b = vec[0; n]\n    let c = vec[]\n    let d = [[0; 3]; 2]\n}\n",
    );
}

#[test]
fn control_flow() {
    assert_prints(
        "func f() {\n    'outer: for row in &grid {\n        for &cell in row {\n            if cell == 0 { break 'outer }\n        }\n    }\n    let found = loop {\n        if let Some(x) = next() { break x }\n    }\n    while let Some(top) = stack.Pop() { println(\"{top}\") }\n}\n",
        "func f() {\n    'outer: for row in &grid {\n        for &cell in row {\n            if cell == 0 {\n                break 'outer\n            }\n        }\n    }\n    let found = loop {\n        if let Some(x) = next() {\n            break x\n        }\n    }\n    while let Some(top) = stack.Pop() {\n        println(\"{top}\")\n    }\n}\n",
    );
}

#[test]
fn let_else_and_var_patterns() {
    assert_prints(
        "func f() {\n    let Some(user) = find(id) else {\n        return Err(Error.NotFound)\n    }\n    let (var a, b) = (1, 2)\n    var (c, d) = t\n}\n",
        "func f() {\n    let Some(user) = find(id) else {\n        return Err(Error.NotFound)\n    }\n    let (var a, b) = (1, 2)\n    var (c, d) = t\n}\n",
    );
}

#[test]
fn struct_literals_and_conditions() {
    assert_prints(
        "func f() {\n    let t = Task { id, title, done: false }\n    let c = Task { done: true, ..t.Clone() }\n    if x == y { g() }\n}\n",
        "func f() {\n    let t = Task { id, title, done: false }\n    let c = Task { done: true, ..t.Clone() }\n    if x == y {\n        g()\n    }\n}\n",
    );
}

#[test]
fn precedence_and_ranges() {
    assert_prints(
        "func f() {\n    let a = 1 + 2 * 3 - -x as int64\n    for i in (1..=3).Rev() {}\n    let s = &bytes[1..3]\n    let r = ..\n}\n",
        "func f() {\n    let a = 1 + 2 * 3 - -x as int64\n    for i in (1..=3).Rev() {}\n    let s = &bytes[1..3]\n    let r = ..\n}\n",
    );
    assert_error("func f() { let a = x == y == z }\n", "comparison operators cannot be chained");
}

#[test]
fn tuple_fields_and_try() {
    assert_prints(
        "func f() {\n    let m = x.0.1\n    let n: int64 = \"5\".Parse().Unwrap()\n    let p = readPort(path)?\n}\n",
        "func f() {\n    let m = x.0.1\n    let n: int64 = \"5\".Parse().Unwrap()\n    let p = readPort(path)?\n}\n",
    );
}

#[test]
fn unsafe_blocks_and_casts() {
    assert_prints(
        "func f() {\n    let x = unsafe { *(a as *const c_int) }\n    let p = &raw const x\n}\n",
        "func f() {\n    let x = unsafe {\n        *(a as *const c_int)\n    }\n    let p = &raw const x\n}\n",
    );
}

#[test]
fn inline_asm() {
    assert_prints(
        "func f() {\n    unsafe { asm(\"mov {0}, {1}\", out(reg) x, in(reg) y, options(nomem, nostack)) }\n}\n",
        "func f() {\n    unsafe {\n        asm(\"mov {0}, {1}\", out(reg) x, in(reg) y, options(nomem, nostack))\n    }\n}\n",
    );
}

#[test]
fn turbofish_is_reported() {
    assert_error("func f() { let n = s.Parse::<int64>() }\n", "Vertex has no turbofish");
}

#[test]
fn unbalanced_delimiters_are_reported() {
    assert_error("func f() {\n    g(\n}\n", "mismatched closing delimiter");
    assert_error("func f() {\n", "this file contains an unclosed delimiter");
}

#[test]
fn errors_recover_per_statement() {
    let (_, errors) = parse("func f() {\n    let = 1\n    let b = 2\n    let = 3\n}\nfunc g() {}\n");
    assert_eq!(errors.matches("error: expected").count(), 2, "{errors}");
}

// ---------------------------------------------------------------------------
// Every Vertex snippet in the usage guide parses
// ---------------------------------------------------------------------------

/// The code blocks marked `// Vertex` in usage.md.
fn usage_snippets() -> Vec<(usize, String)> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../src/doc/reference/usage.md");
    let text = std::fs::read_to_string(path).expect("usage.md is in src/doc/reference");
    let mut out = Vec::new();
    let mut lines = text.lines().enumerate();
    while let Some((n, line)) = lines.next() {
        if line.trim() != "```rust" {
            continue;
        }
        let mut body = Vec::new();
        for (_, l) in lines.by_ref() {
            if l.trim() == "```" {
                break;
            }
            body.push(l);
        }
        if body.first().is_some_and(|l| l.trim_start().starts_with("// Vertex")) {
            out.push((n + 1, body[1..].join("\n")));
        }
    }
    out
}

/// A snippet as a file. Snippets that are statements rather than items get
/// wrapped in a function, after any `package` and `import` lines.
fn snippet_as_file(src: &str) -> String {
    let (_, errors) = parse(src);
    if errors.is_empty() {
        return src.to_string();
    }
    let mut header = String::new();
    let mut body = String::new();
    let mut in_imports = false;
    let mut still_header = true;
    for line in src.lines() {
        let t = line.trim();
        if still_header && (t.starts_with("package ") || t.starts_with("import") || t.starts_with('@') && t.contains("package") || in_imports || t.is_empty()) {
            if t == "import (" {
                in_imports = true;
            } else if in_imports && t == ")" {
                in_imports = false;
            }
            header.push_str(line);
            header.push('\n');
            continue;
        }
        still_header = false;
        body.push_str(line);
        body.push('\n');
    }
    format!("{header}func snippet() {{\n{body}}}\n")
}

#[test]
fn usage_guide_snippets_parse() {
    let snippets = usage_snippets();
    assert!(snippets.len() >= 39, "found only {} snippets", snippets.len());
    let mut failures = Vec::new();
    for (line, src) in &snippets {
        // `{ … }` marks elided code in the guide.
        let src = src.replace("{ … }", "{}");
        let file = snippet_as_file(&src);
        let (printed, errors) = parse(&file);
        if !errors.is_empty() {
            failures.push(format!("usage.md:{line}\n{file}\n{errors}"));
            continue;
        }
        let (again, errors) = parse(&printed);
        if !errors.is_empty() || again != printed {
            failures.push(format!("usage.md:{line}: printing isn't stable\n{printed}\n{errors}"));
        }
    }
    assert!(failures.is_empty(), "{} of {} snippets failed:\n\n{}", failures.len(), snippets.len(), failures.join("\n\n"));
}
