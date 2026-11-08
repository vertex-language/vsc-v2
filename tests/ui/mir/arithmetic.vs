//@ check-pass
//@ compile-flags: -Z dump-mir
// Integer arithmetic is checked as in a Rust debug build: overflow for
// `+`, `-` and `*`, a zero divisor for `/` and `%`; indexing is
// bounds-checked.

package main

func calc(a: int32, b: int32) -> int32 {
    (a + b) * 2 / b
}

func at(xs: [int64; 3], i: uint) -> int64 {
    xs[i]
}

func float(x: float64) -> float64 {
    x * 2.0 + 1.0
}
