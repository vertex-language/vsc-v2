//@ check-pass
//@ add-core-stubs
//@ compile-flags: -Z dump-types
// Exception 3: a class value's fields and methods are reached through it.
// Exception 14: function literals and functions coerce to `func` values,
// which are called like functions.

package main

class Counter { value: int64 }

implement Counter {
    func Get(&self) -> int64 { self.value }
}

func double(x: int64) -> int64 { x * 2 }

func apply(f: func(int64) -> int64, x: int64) -> int64 { f(x) }

func main() {
    let c = Counter { value: 1 }
    let v = c.value
    let g = c.Get()
    let a = apply(func(x) { x + 1 }, 2)
    let b = apply(double, 3)
    let inc = func(x: int64) -> int64 { x + 1 }
    let d = inc(4)
}
