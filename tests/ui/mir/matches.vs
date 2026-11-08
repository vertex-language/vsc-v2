//@ check-pass
//@ compile-flags: -Z dump-mir
// Matches test each arm in turn: discriminants for enums, values for
// integers, ranges by comparison, slices by length; bindings are made once
// the arm matches.

package main

enum Shape { Circle(float64); Rect { w: float64; h: float64 }; Empty }

func area(s: Shape) -> float64 {
    match s {
        Shape.Circle(r) => r * r
        Shape.Rect { w, h } => w * h
        Shape.Empty => 0.0
    }
}

func classify(n: uint8) -> int32 {
    match n {
        0 => 0
        1..=9 => 1
        _ => 2
    }
}

func ends(xs: &[int64]) -> int64 {
    match xs {
        [] => 0
        [first, .., last] => *first + *last
        [one] => *one
    }
}
