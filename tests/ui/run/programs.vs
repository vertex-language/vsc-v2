//@ run-pass
// Recursion, loops, structs, enums and matches, closures with captures,
// classes, vectors, `?` and `for` over ranges and slices.

package main

enum Shape { Circle(float64); Rect { w: float64; h: float64 }; Empty }

func area(s: &Shape) -> float64 {
    match s {
        Shape.Circle(r) => 3.0 * *r * *r
        Shape.Rect { w, h } => *w * *h
        Shape.Empty => 0.0
    }
}

func fib(n: uint64) -> uint64 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

func half(n: int32) -> Option<int32> {
    if n % 2 == 0 { Option.Some(n / 2) } else { Option.None }
}

func quarter(n: int32) -> Option<int32> {
    let h = half(n)?
    half(h)
}

class Counter { value: int64 }

func main() {
    var total = 0.0
    for s in &[Shape.Circle(1.0), Shape.Rect { w: 2.0, h: 3.5 }, Shape.Empty] {
        total += area(s)
    }
    println("total area {total:.2}")
    println("fib(20) = {}", fib(20))

    var evens = vec[]
    for i in 0..10 {
        if i % 2 == 0 { evens.push(i) }
    }
    println("{:?} ({} of them)", evens, evens.len())

    var sum = 0
    let add = func(n: int32) { sum += n }
    add(5)
    add(7)
    println("sum {sum}")

    let c = Counter { value: 1 }
    let d = c
    d.value += 10
    println("shared {}", c.value)

    println("{:?} {:?} {:?}", quarter(8), quarter(6), quarter(3))

    var i = 0
    let found = loop {
        i += 1
        if i * i > 50 { break i }
    }
    let label = outer: {
        for x in 0..5 {
            if x == 3 { break outer "three" }
        }
        "none"
    }
    println("{found} {label}")
    assertEq(fib(10), 55)
}
