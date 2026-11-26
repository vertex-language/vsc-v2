//@ check-pass
//@ compile-flags: -Z dump-mir
// `if`, `&&`, loops with `break` values and labeled blocks, and early
// `return`.

package main

func pick(a: bool, b: bool) -> int32 {
    if a && b { 1 } else if a { 2 } else { 3 }
}

func first(n: uint) -> uint {
    var i = 0
    let found = loop {
        if i == n { break i }
        i += 1
    }
    found
}

func early(x: int32) -> int32 {
    if x < 0 { return 0 }
    x
}
