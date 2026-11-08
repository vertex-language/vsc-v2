package main

func sum<T: Add<Output = T> + Copy>(xs: &[T], zero: T) -> T {
    var total = zero
    for x in xs {
        total = total + *x
    }
    total
}

func dot<T: Add<Output = T> + Mul<Output = T> + Copy>(a: &[T], b: &[T], zero: T) -> T {
    var total = zero
    var i = 0
    while i < sliceLen(a) {
        total = total + a[i] * b[i]
        i += 1
    }
    total
}

func largest<T: PartialOrd + Copy>(xs: &[T]) -> T {
    var best = xs[0]
    for x in xs {
        if *x > best {
            best = *x
        }
    }
    best
}

func clamp<T: PartialOrd>(x: T, lo: T, hi: T) -> T {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

func main() {
    println("{}", sum(&[1, 2, 3, 4], 0))
    println("{}", sum(&[0.5, 0.25, 0.125], 0.0))
    println("{}", dot(&[1, 2, 3], &[4, 5, 6], 0))
    println("{}", dot(&[1.5, 2.0], &[2.0, 0.5], 0.0))
    println("{} {} {}", largest(&[3, 9, 2]), largest(&[1.5, -2.0]), largest(&['a', 'z', 'm']))
    println("{} {} {}", clamp(15, 0, 10), clamp(-3, 0, 10), clamp(5, 0, 10))
    println("{}", clamp(2.5, 1.0, 2.0))
}
