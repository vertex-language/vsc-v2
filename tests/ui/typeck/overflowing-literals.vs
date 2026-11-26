// An integer literal must fit its type, as with Rust's
// `overflowing_literals` (an error by default).

package main

func main() {
    let a: uint8 = 256 //~ ERROR literal out of range for `uint8`
    let b: int8 = -128
    let c: int8 = -129 //~ ERROR literal out of range for `int8`
    let d = 3000000000 //~ ERROR literal out of range for `int32`
    let e: int64 = 3000000000
}
