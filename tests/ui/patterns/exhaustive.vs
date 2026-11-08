//@ check-pass
//@ add-core-stubs
// Matches that cover every value: ranges split at their boundaries,
// nested constructors, slices by length, and default binding modes.

package main

enum Shape { Circle(float64); Rect { w: float64; h: float64 }; Empty }

func all(s: &Shape, n: uint8, i: int8, b: bool, o: Option<Option<int32>>, xs: &[int64], c: char, t: (bool, bool)) {
    match s {
        Shape.Circle(_) => {}
        Shape.Rect { w, .. } => {}
        Shape.Empty => {}
    }
    match n {
        0 => {}
        1..=9 => {}
        10..=255 => {}
    }
    match i {
        -128..=-1 => {}
        0 => {}
        1.. => {}
    }
    match o {
        Option.Some(Option.Some(_)) => {}
        Option.Some(Option.None) | Option.None => {}
    }
    match xs {
        [] => {}
        [x] => {}
        [first, .., last] => {}
    }
    match c {
        'a'..='z' => {}
        _ => {}
    }
    match t {
        (true, true) => {}
        (true, false) => {}
        (false, _) => {}
    }
    match [1, 2, 3] {
        [a, ..] => {}
    }
    let (p, q) = t
    match n {
        x if x > 3 => {}
        _ => {}
    }
}
