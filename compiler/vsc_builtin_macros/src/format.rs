//! Checking format strings (rustc's `builtin_macros/format.rs`).

use vsc_ast::token::{Lit, LitKind};
use vsc_ast::*;
use vsc_errors::Applicability;
use vsc_parse_format::{self as fmt, Count, Piece, Position};
use vsc_session::ParseSess;
use vsc_span::{BytePos, Ident, Span, Symbol};

/// `format_args`: the first argument must be a string literal; the rest are
/// positional arguments, then `name = value` arguments. `{name}` with no
/// such argument captures the variable `name`.
pub fn parse_format_args(psess: &ParseSess, call_span: Span, args: Vec<P<Expr>>) -> Option<P<FormatArgs>> {
    let dcx = &psess.dcx;
    let mut args = args.into_iter();
    let first = args.next()?;
    let (lit, lit_span) = match &first.kind {
        ExprKind::Lit(lit @ Lit { kind: LitKind::Str | LitKind::StrRaw(_), suffix: None, .. }) => (*lit, first.span),
        _ => {
            dcx.struct_span_err(first.span, "format argument must be a string literal")
                .with_span_suggestion(
                    first.span.shrink_to_lo(),
                    "you might be missing a string literal to format with",
                    "\"{}\", ",
                    Applicability::MaybeIncorrect,
                )
                .emit();
            return None;
        }
    };
    // Where the string's contents start in the source.
    let content_start = match lit.kind {
        LitKind::StrRaw(n) => lit_span.lo().0 + 2 + n as u32,
        _ => lit_span.lo().0 + 1,
    };
    let inner = |range: std::ops::Range<usize>| {
        Span::new(BytePos(content_start + range.start as u32), BytePos(content_start + range.end as u32))
    };

    // Explicit arguments: positional, then named.
    let mut arguments: Vec<FormatArgument> = Vec::new();
    let mut positional = 0;
    let mut first_named: Option<Span> = None;
    let mut ok = true;
    for arg in args {
        if let ExprKind::Assign(lhs, rhs, _) = &arg.kind {
            if let ExprKind::Path(None, p) = &lhs.kind {
                if let Some(name) = p.as_ident() {
                    if let Some(prev) = arguments.iter().find_map(|a| match a.kind {
                        FormatArgumentKind::Named(n) if n.name == name.name => Some(n.span),
                        _ => None,
                    }) {
                        dcx.struct_span_err(name.span, format!("duplicate argument named `{name}`"))
                            .with_span_label(prev, "previously here")
                            .with_span_label(name.span, "duplicate argument")
                            .emit();
                        ok = false;
                        continue;
                    }
                    first_named.get_or_insert(arg.span);
                    arguments.push(FormatArgument { kind: FormatArgumentKind::Named(name), expr: rhs.clone() });
                    continue;
                }
            }
        }
        if let Some(named) = first_named {
            dcx.struct_span_err(arg.span, "positional arguments cannot follow named arguments")
                .with_span_label(arg.span, "positional arguments must be before named arguments")
                .with_span_label(named, "named argument")
                .emit();
            ok = false;
            continue;
        }
        positional += 1;
        arguments.push(FormatArgument { kind: FormatArgumentKind::Normal, expr: arg });
    }
    let explicit = arguments.len();

    let text = lit.symbol.as_str();
    let pieces = match fmt::parse(text) {
        Ok(p) => p,
        Err(e) => {
            let mut err = dcx
                .struct_span_err(inner(e.span.clone()), format!("invalid format string: {}", e.description))
                .with_span_label(inner(e.span), e.label);
            if let Some(note) = e.note {
                err.note(note);
            }
            err.emit();
            return None;
        }
    };

    let mut used = vec![false; explicit];
    let mut template = Vec::new();
    let mut missing_positional: Vec<(usize, Span, bool)> = Vec::new();

    // An argument by position or name, capturing names from the scope.
    let lookup = |arguments: &mut Vec<FormatArgument>,
                      used: &mut Vec<bool>,
                      missing: &mut Vec<(usize, Span, bool)>,
                      position: Position<'_>,
                      span: Span|
     -> Option<usize> {
        match position {
            Position::ArgumentImplicitlyIs(i) | Position::ArgumentIs(i) => {
                if i < positional {
                    used[i] = true;
                    Some(i)
                } else {
                    missing.push((i, span, matches!(position, Position::ArgumentImplicitlyIs(_))));
                    None
                }
            }
            Position::ArgumentNamed(name) => {
                let sym = Symbol::intern(name);
                if let Some(i) = arguments.iter().position(|a| match a.kind {
                    FormatArgumentKind::Named(n) | FormatArgumentKind::Captured(n) => n.name == sym,
                    FormatArgumentKind::Normal => false,
                }) {
                    if i < used.len() {
                        used[i] = true;
                    }
                    return Some(i);
                }
                // Capture the variable (Rust 2021's implicit arguments).
                let ident = Ident::new(sym, span);
                let expr = P::new(Expr {
                    id: DUMMY_NODE_ID,
                    kind: ExprKind::Path(None, Path::from_ident(ident)),
                    span,
                    attrs: Vec::new(),
                });
                arguments.push(FormatArgument { kind: FormatArgumentKind::Captured(ident), expr });
                Some(arguments.len() - 1)
            }
        }
    };

    for piece in &pieces {
        match piece {
            Piece::Lit(s) => template.push(FormatArgsPiece::Literal(Symbol::intern(s))),
            Piece::NextArgument(a) => {
                let span = inner(a.span.clone());
                let pos_span = if a.position_span.is_empty() { span } else { inner(a.position_span.clone()) };
                let argument = lookup(&mut arguments, &mut used, &mut missing_positional, a.position, pos_span);
                let mut count = |c: &Count<'_>| -> Option<FormatCount> {
                    match c {
                        Count::CountImplied => None,
                        Count::CountIs(n) => Some(FormatCount::Literal(*n)),
                        Count::CountIsParam(i) | Count::CountIsStar(i) => {
                            lookup(&mut arguments, &mut used, &mut missing_positional, Position::ArgumentIs(*i), span)
                                .map(FormatCount::Argument)
                        }
                        Count::CountIsName(name, r) => lookup(
                            &mut arguments,
                            &mut used,
                            &mut missing_positional,
                            Position::ArgumentNamed(name),
                            inner(r.clone()),
                        )
                        .map(FormatCount::Argument),
                    }
                };
                let width = count(&a.format.width);
                let precision = count(&a.format.precision);
                let format_trait = match a.format.ty {
                    "" => FormatTrait::Display,
                    "?" => FormatTrait::Debug,
                    "e" => FormatTrait::LowerExp,
                    "E" => FormatTrait::UpperExp,
                    "o" => FormatTrait::Octal,
                    "p" => FormatTrait::Pointer,
                    "b" => FormatTrait::Binary,
                    "x" => FormatTrait::LowerHex,
                    "X" => FormatTrait::UpperHex,
                    other => {
                        let ty_span = a.format.ty_span.clone().map(inner).unwrap_or(span);
                        dcx.struct_span_err(ty_span, format!("unknown format trait `{other}`"))
                            .with_note("the only appropriate formatting traits are:\n- ``, which uses the `Display` trait\n- `?`, which uses the `Debug` trait\n- `e`, which uses the `LowerExp` trait\n- `E`, which uses the `UpperExp` trait\n- `o`, which uses the `Octal` trait\n- `p`, which uses the `Pointer` trait\n- `b`, which uses the `Binary` trait\n- `x`, which uses the `LowerHex` trait\n- `X`, which uses the `UpperHex` trait")
                            .emit();
                        ok = false;
                        FormatTrait::Display
                    }
                };
                let format_options = FormatOptions {
                    width,
                    precision,
                    alignment: match a.format.align {
                        fmt::Alignment::Left => Some(FormatAlignment::Left),
                        fmt::Alignment::Right => Some(FormatAlignment::Right),
                        fmt::Alignment::Center => Some(FormatAlignment::Center),
                        fmt::Alignment::Unknown => None,
                    },
                    fill: a.format.fill,
                    sign: a.format.sign.map(|s| match s {
                        fmt::Sign::Plus => FormatSign::Plus,
                        fmt::Sign::Minus => FormatSign::Minus,
                    }),
                    alternate: a.format.alternate,
                    zero_pad: a.format.zero_pad,
                    debug_hex: a.format.debug_hex.map(|h| match h {
                        fmt::DebugHex::Lower => FormatDebugHex::Lower,
                        fmt::DebugHex::Upper => FormatDebugHex::Upper,
                    }),
                };
                if let Some(argument) = argument {
                    template.push(FormatArgsPiece::Placeholder(FormatPlaceholder { argument, span, format_trait, format_options }));
                }
            }
        }
    }

    if !missing_positional.is_empty() {
        report_missing(psess, &missing_positional, positional);
        ok = false;
    }

    // Arguments the string doesn't use.
    let unused: Vec<usize> = (0..explicit).filter(|&i| !used[i]).collect();
    if !unused.is_empty() && missing_positional.is_empty() {
        let mut err = if unused.len() == 1 {
            let named = matches!(arguments[unused[0]].kind, FormatArgumentKind::Named(_));
            let msg = if named { "named argument never used" } else { "argument never used" };
            dcx.struct_span_err(arguments[unused[0]].expr.span, msg)
                .with_span_label(arguments[unused[0]].expr.span, msg)
        } else {
            let mut err = dcx.struct_span_err(
                unused.iter().map(|&i| arguments[i].expr.span).collect::<Vec<_>>(),
                "multiple unused formatting arguments",
            );
            for &i in &unused {
                err.span_label(arguments[i].expr.span, "argument never used");
            }
            err
        };
        err.span_label(lit_span, if unused.len() == 1 { "formatting specifier missing" } else { "multiple missing formatting specifiers" });
        err.emit();
        ok = false;
    }

    ok.then(|| P::new(FormatArgs { span: call_span, lit, lit_span, template, arguments }))
}

/// rustc's two messages: a count when every missing argument is a `{}`, and
/// "invalid reference" when one was asked for by number.
fn report_missing(psess: &ParseSess, missing: &[(usize, Span, bool)], given: usize) {
    let there_are = match given {
        0 => "no arguments were given".to_string(),
        1 => "there is 1 argument".to_string(),
        n => format!("there are {n} arguments"),
    };
    let spans: Vec<Span> = missing.iter().map(|&(_, s, _)| s).collect();
    let msg = if missing.iter().all(|&(_, _, implicit)| implicit) {
        let wanted = missing.iter().map(|&(i, _, _)| i + 1).max().unwrap_or(0);
        let s = if wanted == 1 { "" } else { "s" };
        format!("{wanted} positional argument{s} in format string, but {there_are}")
    } else {
        let &(i, _, _) = missing.iter().find(|&&(_, _, implicit)| !implicit).unwrap();
        format!("invalid reference to positional argument {i} ({there_are})")
    };
    let mut err = psess.dcx.struct_span_err(spans, msg);
    if missing.iter().any(|&(_, _, implicit)| !implicit) {
        err.note("positional arguments are zero-based");
    }
    err.emit();
}
