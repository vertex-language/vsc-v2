//@ add-core-stubs
// E0004: the error shows values no arm matches.

package main

enum Shape { Circle(float64); Rect { w: float64; h: float64 }; Empty }

func f(s: &Shape, n: uint8, i: int8, t: (bool, bool), o: Option<Option<int32>>, xs: &[int64], g: uint8) {
    match s { //~ ERROR non-exhaustive patterns: `&Shape.Rect { .. }` not covered
        Shape.Circle(r) => {}
        Shape.Empty => {}
    }
    match n { //~ ERROR non-exhaustive patterns: `201..=uint8.MAX` not covered
        0..=9 => {}
        10..=200 => {}
    }
    match i { //~ ERROR non-exhaustive patterns: `int8.MIN..=-1` not covered
        0 => {}
        1..=127 => {}
    }
    match t { //~ ERROR non-exhaustive patterns: `(true, true)` not covered
        (true, false) => {}
        (false, _) => {}
    }
    match o { //~ ERROR non-exhaustive patterns: `Option.Some(Option.None)` not covered
        Option.Some(Option.Some(_)) => {}
        Option.None => {}
    }
    match xs { //~ ERROR non-exhaustive patterns: `&[_, _, ..]` not covered
        [] => {}
        [x] => {}
    }
    match g { //~ ERROR non-exhaustive patterns: `uint8.MAX` not covered
        x if x > 0 => {}
        0..=254 => {}
    }
}
