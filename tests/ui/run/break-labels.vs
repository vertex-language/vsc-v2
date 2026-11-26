//@ run-pass
//@ add-core-stubs
// `break name` names a label only when an enclosing loop or block has that
// label; otherwise `name` is the value. `break (name)` breaks with a value
// that shares an enclosing label's name. Labels don't reach into function
// literals.

package main

func main() {
    outer: loop {
        break outer
    }
    let result = 5
    let x = loop {
        break result
    }
    let found = outer: loop {
        let outer = 7
        loop {
            break outer outer + 1
        }
    }
    let same = outer: loop {
        let outer = 9
        break (outer)
    }
    let f = func() -> int32 {
        let outer = 3
        loop {
            break outer
        }
    }
    total: for i in 0..3 {
        if i == 1 {
            continue total
        }
        print("{i} ")
    }
    println()
    println("{x} {found} {same} {}", f())
}
