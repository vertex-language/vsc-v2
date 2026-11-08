//@ run-pass
//@ add-core-stubs
// `var` on a flat receiver makes it a mutable binding, as on any parameter.

package main

struct Counter { n: int32 }

func (var c: Counter) bumped() -> Counter {
    c.n += 1
    c
}

func main() {
    let c = Counter { n: 1 }
    println("{}", c.bumped().bumped().n)
}
