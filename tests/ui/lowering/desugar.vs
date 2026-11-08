//@ check-pass
//@ compile-flags: -Z unpretty=hir
// `?`, ranges, `vec[…]` and destructuring assignment.

package main

enum Res { Good(int64); Bad }

func parse(r: Res) -> Res {
    let n = r?
    Res.Good(n)
}

func ranges(n: int64) {
    let a = 0..n
    let b = n..
    let c = ..n
    let d = 0..=n
    let e = ..=n
    let f = ..
}

func vectors(n: uint) {
    let empty = vec[]
    let three = vec[1, 2, 3]
    let zeros = vec[0; n]
}

func swap() {
    var a = 1
    var b = 2
    (a, b) = (b, a)
    _ = a + b
    [a, .., b] = [1, 2, 3]
}
