package main

func find(xs: &[int32], target: int32) -> Option<uint> {
    var i = 0
    while i < sliceLen(xs) {
        if xs[i] == target {
            return Option.Some(i)
        }
        i += 1
    }
    Option.None
}

func checkedDiv(a: int32, b: int32) -> Option<int32> {
    if b == 0 {
        return Option.None
    }
    Option.Some(a / b)
}

func compute(a: int32, b: int32, c: int32) -> Option<int32> {
    let x = checkedDiv(a, b)?
    let y = checkedDiv(x, c)?
    Option.Some(x + y)
}

func firstNegative(xs: &[int32]) -> int32 {
    for x in xs {
        if *x < 0 {
            return *x
        }
    }
    0
}

func main() {
    let xs = [4, 8, 15, 16, 23, 42]
    println("{:?} {:?}", find(&xs, 16), find(&xs, 5))
    println("{:?} {:?} {:?}", compute(100, 5, 2), compute(1, 0, 2), compute(10, 2, 0))
    println("{} {}", firstNegative(&[3, -4, -5]), firstNegative(&xs))
}
