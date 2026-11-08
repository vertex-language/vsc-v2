use vsc_ast::{BuiltinKind, ExprKind, FormatArgsPiece, FormatArgumentKind, FormatTrait, ItemKind, StmtKind};
use vsc_errors::SharedBuffer;
use vsc_session::ParseSess;
use vsc_span::FileName;

/// Parses `body` inside a function, expands built-ins, and returns the
/// statements' expressions and the errors.
fn expand(body: &str) -> (Vec<ExprKind>, String) {
    let buf = SharedBuffer::new();
    let psess = ParseSess::with_buffer(buf.clone());
    let src = format!("func f() {{\n{body}\n}}\n");
    let file = vsc_parse::parse_source_str(&psess, FileName::Custom("t.vs".into()), src);
    let mut krate = vsc_ast::Crate {
        root: vsc_ast::Package {
            name: vsc_span::Ident::from_str("t"),
            id: vsc_ast::DUMMY_NODE_ID,
            files: vec![file],
            children: vec![],
            dir: Default::default(),
            span: vsc_span::DUMMY_SP,
        },
        id: vsc_ast::DUMMY_NODE_ID,
        span: vsc_span::DUMMY_SP,
    };
    crate::expand_builtins(&psess, &mut krate);
    let ItemKind::Fn(f) = &krate.root.files[0].items[0].kind else { panic!() };
    let exprs = f
        .body
        .as_ref()
        .unwrap()
        .stmts
        .iter()
        .filter_map(|s| match &s.kind {
            StmtKind::Expr(e) => Some(e.kind.clone()),
            StmtKind::Let(l) => match &l.kind {
                vsc_ast::LocalKind::Init(e) => Some(e.kind.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    (exprs, buf.contents())
}

#[test]
fn println_with_captures_and_specs() {
    let (exprs, errors) = expand("println(\"{name} is {} ({:>8.2?})\", age, ratio)");
    assert_eq!(errors, "");
    let ExprKind::Builtin(b) = &exprs[0] else { panic!("{:?}", exprs[0]) };
    let BuiltinKind::Print { newline: true, stderr: false, args } = &b.kind else { panic!() };
    // Explicit arguments first, then the captured `name`.
    let kinds: Vec<_> = args.arguments.iter().map(|a| a.kind).collect();
    assert!(matches!(kinds[..], [FormatArgumentKind::Normal, FormatArgumentKind::Normal, FormatArgumentKind::Captured(n)] if n.as_str() == "name"));
    let placeholders: Vec<_> = args
        .template
        .iter()
        .filter_map(|p| match p {
            FormatArgsPiece::Placeholder(p) => Some((p.argument, p.format_trait)),
            _ => None,
        })
        .collect();
    assert_eq!(placeholders, vec![(2, FormatTrait::Display), (0, FormatTrait::Display), (1, FormatTrait::Debug)]);
}

#[test]
fn all_builtins_expand() {
    let (exprs, errors) = expand(
        "print(\"a\")\nprintln()\neprintln(\"e {}\", 1)\nlet s = format(\"{x}\", x = 2)\npanic()\npanic(\"bad {}\", 3)\ntodo()\nunreachable(\"no\")\nassert(ok)\nassert(ok, \"why {}\", w)\nassertEq(a, b)",
    );
    assert_eq!(errors, "");
    let names: Vec<&str> = exprs
        .iter()
        .map(|e| match e {
            ExprKind::Builtin(b) => b.kind.name(),
            other => panic!("not expanded: {other:?}"),
        })
        .collect();
    assert_eq!(
        names,
        vec!["print", "println", "eprintln", "format", "panic", "panic", "todo", "unreachable", "assert", "assert", "assertEq"]
    );
}

#[test]
fn errors() {
    let cases = [
        ("println(\"{} {}\", a)", "2 positional arguments in format string, but there is 1 argument"),
        ("println(\"{1}\", a)", "invalid reference to positional argument 1 (there is 1 argument)"),
        ("println(\"x\", a)", "argument never used"),
        ("println(\"x\", a, b)", "multiple unused formatting arguments"),
        ("println(\"{:z}\", a)", "unknown format trait `z`"),
        ("println(\"{\")", "invalid format string: expected `}` but string was terminated"),
        ("println(\"}\")", "invalid format string: unmatched `}` found"),
        ("println(x)", "format argument must be a string literal"),
        ("format()", "`format` requires at least a format string argument"),
        ("assertEq(a)", "`assertEq` requires two values to compare"),
        ("println(\"{a}\", a = 1, 2)", "positional arguments cannot follow named arguments"),
        ("println(\"{a}\", a = 1, a = 2)", "duplicate argument named `a`"),
    ];
    for (src, want) in cases {
        let (_, errors) = expand(src);
        assert!(errors.contains(want), "for {src}: expected {want:?}, got:\n{errors}");
    }
}

#[test]
fn error_spans_point_into_the_string() {
    let (_, errors) = expand("println(\"ok {:z}\", a)");
    assert!(errors.contains("^"), "{errors}");
    assert!(errors.contains(" --> <t.vs>:2:15"), "{errors}");
}
