//@ check-pass
//@ compile-flags: -Z unpretty=hir
// The format built-ins become calls with a `formatArguments` value, built
// as rustc builds `fmt::Arguments`.

package main

func main(a: int64, b: int64, ok: bool) {
    println()
    print("plain")
    println("{} and {:?}", a, b)
    println("{b} before {a}")
    println("{a} twice {a}")
    eprintln("{:>8.3}", a)
    let s = format("{0:x}-{0:#b}", a)
    panic()
    panic("bad {}", a)
    todo()
    unreachable("state {}", b)
    assert(ok)
    assert(ok, "not ok: {}", a)
    assertEq(a, b)
}
