// Unsafe operations need an `unsafe` block. In an `unsafe func` they're
// warnings, as in Rust 2024: an unsafe function's body is safe code.

package main

unsafe func danger() {}
static var Counter: uint = 0

unsafe extern "C" {
    func abs(n: int32) -> int32
    static errno: int32
    safe func labs(n: int64) -> int64
}

union Bits { i: uint32; f: float32 }

func g(p: *const int32, b: Bits) -> int32 {
    danger() //~ ERROR call to unsafe function `danger` is unsafe and requires unsafe block
    Counter += 1 //~ ERROR use of mutable static is unsafe and requires unsafe block
    let a = abs(-3) //~ ERROR call to unsafe function `abs` is unsafe and requires unsafe block
    let e = errno //~ ERROR use of extern static is unsafe and requires unsafe block
    let x = *p //~ ERROR dereference of raw pointer is unsafe and requires unsafe block
    let i = b.i //~ ERROR access to union field is unsafe and requires unsafe block
    let l = labs(-4)
    unsafe {
        danger()
        let y = *p
        let f = func() { danger() }
    }
    let h = func() { danger() } //~ ERROR call to unsafe function `danger` is unsafe and requires unsafe block
    x
}

unsafe func inside(p: *const int32) -> int32 {
    *p //~ WARN dereference of raw pointer is unsafe and requires unsafe block
}
