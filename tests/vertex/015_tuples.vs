package main

func minMax(xs: &[int32]) -> (int32, int32) {
    var lo = xs[0]
    var hi = xs[0]
    for &x in xs {
        if x < lo {
            lo = x
        }
        if x > hi {
            hi = x
        }
    }
    (lo, hi)
}

func divmod(a: int32, b: int32) -> (int32, int32) {
    (a / b, a % b)
}

func main() {
    let (lo, hi) = minMax(&[4, -2, 9, 0])
    println("{lo} {hi}")
    var a = 1
    var b = 2
    (a, b) = (b, a)
    println("{a} {b}")
    let nested = ((1, 2), (3, (4, 5)))
    let ((p, _), (q, (r, s))) = nested
    println("{}", p + q + r + s)
    let t = (1u8, 'z', 2.5f32, "s")
    println("{:?} {} {}", t, t.1, (t.0 as float32) * t.2)
    var pt = (0, 0)
    pt.0 += 5
    pt.1 -= 3
    println("{:?}", pt)
    let unit = ()
    println("{:?}", unit)
    let (d, m) = divmod(17, 5)
    println("{d} rem {m}")
    let pairs = [(1, 'a'), (2, 'b'), (3, 'c')]
    for (n, c) in &pairs {
        print("{n}{c} ")
    }
    println()
}
