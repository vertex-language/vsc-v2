//@ check-pass
//@ add-core-stubs
//@ compile-flags: -Z dump-types
// Patterns, with Rust's default binding modes: matching a reference binds
// by reference.

package main

enum Shape { Circle(float64); Rect { w: float64; h: float64 }; Empty }

func area(s: &Shape) -> float64 {
    match s {
        Shape.Circle(r) => *r * *r
        Shape.Rect { w, h } => *w * *h
        Shape.Empty => 0.0
    }
}

func parts(t: (int64, (bool, char)), xs: &[int64]) {
    let (n, (flag, ch)) = t
    let [first, .., last] = [1, 2, 3, 4]
    match xs {
        [] => {}
        [one] => {}
        [head, rest @ ..] => {}
    }
    let Option.Some(v) = Option.Some(n) else { return }
    match n {
        0 | 1 => {}
        2..=9 => {}
        _ => {}
    }
}
