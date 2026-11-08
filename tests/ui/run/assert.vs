//@ run-fail
// A failed `assertEq` shows both values, as Rust's `assert_eq!` does.

package main

func main() {
    let a = 2 + 2
    assertEq(a, 5, "math is {}", "hard")
}
