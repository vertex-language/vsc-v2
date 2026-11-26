//@ run-pass
//@ add-core-stubs
// Function items, `func` values and literals all implement the function
// traits; a literal inside another makes it `FnMut` when it mutates what
// both capture.

package main

func double(x: int32) -> int32 {
    x * 2
}

func applyTwice<F: Fn(int32) -> int32>(f: F, x: int32) -> int32 {
    f(f(x))
}

func forward<F: Fn(int32) -> int32>(f: &F, x: int32) -> int32 {
    applyTwice(f, x)
}

func repeat<F: FnMut()>(var f: F, n: uint32) {
    for _ in 0..n {
        f()
    }
}

func main() {
    println("{}", applyTwice(double, 3))
    let boxed: func(int32) -> int32 = func(x) { x - 1 }
    println("{}", applyTwice(boxed, 10))
    let k = 7
    let add = func(x: int32) -> int32 { x + k }
    println("{}", forward(&add, 0))
    var total = 0
    repeat(func() {
        let inner = func() { total += 1 }
        inner()
    }, 4)
    println("{total}")
    for i in 1..=3 {
        print("{i} ")
    }
    println()
}
