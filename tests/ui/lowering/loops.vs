//@ check-pass
//@ compile-flags: -Z unpretty=hir
// `for` and `while` desugar to `loop`, as in rustc. `break` and `continue`
// leave the loop they name, or the innermost one.

package main

func main(xs: &[int64]) {
    var total = 0
    'outer: for x in xs {
        for y in xs {
            if *y > 3 { continue 'outer }
            total += *x * *y
        }
    }
    while total > 10 {
        total -= 1
    }
    let found = loop {
        if total == 0 { break true }
        total -= 1
    }
    let v = 'block: {
        if found { break 'block 1 }
        2
    }
}
