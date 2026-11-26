//@ check-pass
//@ add-core-stubs
//@ compile-flags: -Z dump-mir
// A function literal captures what it uses by reference (mutably if it
// assigns); its body reaches them through its first argument. A class
// literal is a class value (exception 3).

package main

class Counter { value: int64 }

func make() -> Counter { Counter { value: 0 } }

func capture(x: int64) -> int64 {
    var total = 0
    let add = func(n: int64) { total += n + x }
    add(1)
    total
}
