//@ run-fail
// Integer overflow panics, as in a Rust debug build.

package main

func main() {
    var x: uint8 = 250
    for i in 0..10 {
        x += 1
    }
}
