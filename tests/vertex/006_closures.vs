package main

func apply(f: func(int64) -> int64, x: int64) -> int64 {
    f(x)
}

func main() {
    let offset = 10
    let add = func(x: int64) -> int64 { x + offset }
    println("{}", apply(add, 5))
    println("{}", apply(func(x) { x * x }, 7))

    var total = 0
    let accumulate = func(n: int32) { total += n }
    accumulate(3)
    accumulate(4)
    println("total {total}")

    let tens = func(a: int32, b: int32) -> int32 { a * 10 + b }
    println("{}", tens(4, 2))
}
