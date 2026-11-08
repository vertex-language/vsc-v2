use super::*;

fn args(s: &str) -> Vec<(Position<'_>, FormatSpec<'_>)> {
    parse(s)
        .unwrap()
        .into_iter()
        .filter_map(|p| match p {
            Piece::NextArgument(a) => Some((a.position, a.format)),
            Piece::Lit(_) => None,
        })
        .collect()
}

#[test]
fn literals_and_escapes() {
    assert_eq!(parse("hello").unwrap(), vec![Piece::Lit("hello".into())]);
    assert_eq!(parse("{{a}}").unwrap(), vec![Piece::Lit("{a}".into())]);
    assert_eq!(parse("a\\n").unwrap(), vec![Piece::Lit("a\\n".into())]);
}

#[test]
fn positions() {
    let a = args("{} {} {1} {name} {}");
    let pos: Vec<_> = a.iter().map(|(p, _)| *p).collect();
    assert_eq!(pos, vec![
        Position::ArgumentImplicitlyIs(0),
        Position::ArgumentImplicitlyIs(1),
        Position::ArgumentIs(1),
        Position::ArgumentNamed("name"),
        Position::ArgumentImplicitlyIs(2),
    ]);
}

#[test]
fn specs() {
    let a = args("{:>8} {:.2} {:?} {:#?} {:x} {:#010b} {:*^5} {:+} {:width$} {:.prec$} {:x?} {:e}");
    let s: Vec<&FormatSpec> = a.iter().map(|(_, f)| f).collect();
    assert_eq!(s[0].align, Alignment::Right);
    assert_eq!(s[0].width, Count::CountIs(8));
    assert_eq!(s[1].precision, Count::CountIs(2));
    assert_eq!(s[2].ty, "?");
    assert!(s[3].alternate && s[3].ty == "?");
    assert_eq!(s[4].ty, "x");
    assert!(s[5].alternate && s[5].zero_pad && s[5].width == Count::CountIs(10) && s[5].ty == "b");
    assert_eq!((s[6].fill, s[6].align, s[6].width.clone()), (Some('*'), Alignment::Center, Count::CountIs(5)));
    assert_eq!(s[7].sign, Some(Sign::Plus));
    assert!(matches!(s[8].width, Count::CountIsName("width", _)));
    assert!(matches!(s[9].precision, Count::CountIsName("prec", _)));
    assert_eq!((s[10].ty, s[10].debug_hex), ("?", Some(DebugHex::Lower)));
    assert_eq!(s[11].ty, "e");
}

#[test]
fn star_precision_takes_an_argument_first() {
    let a = args("{:.*} {}");
    assert_eq!(a[0].0, Position::ArgumentImplicitlyIs(1));
    assert_eq!(a[0].1.precision, Count::CountIsStar(0));
    assert_eq!(a[1].0, Position::ArgumentImplicitlyIs(2));
}

#[test]
fn errors() {
    let e = parse("a } b").unwrap_err();
    assert_eq!(e.description, "unmatched `}` found");
    assert_eq!(e.span, 2..3);
    let e = parse("x {").unwrap_err();
    assert_eq!(e.description, "expected `}` but string was terminated");
    let e = parse("{0 x}").unwrap_err();
    assert_eq!(e.description, "expected `}`, found `x`");
    let e = parse("{:.}").unwrap_err();
    assert_eq!(e.description, "expected a precision after `.`");
}

#[test]
fn spans() {
    let p = parse("ab {name:?} c").unwrap();
    let Piece::NextArgument(a) = &p[1] else { panic!() };
    assert_eq!(a.span, 3..11);
    assert_eq!(a.position_span, 4..8);
    assert_eq!(a.format.ty_span, Some(9..10));
}
