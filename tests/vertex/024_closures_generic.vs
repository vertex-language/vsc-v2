package main

func applyTwice<F: Fn(int32) -> int32>(f: F, x: int32) -> int32 {
    f(f(x))
}

func callMut<F: FnMut()>(var f: F, times: uint32) {
    for _ in 0..times {
        f()
    }
}

func main() {
    let add3 = func(x) { x + 3 }
    println("{}", applyTwice(add3, 10))
    let factor = 4
    println("{}", applyTwice(func(x) { x * factor }, 2))
    var count = 0
    callMut(func() { count += 2 }, 5)
    println("count {count}")
    var log = vec[]
    var record = func(n: int32) { log.push(n * n) }
    record(3)
    record(4)
    println("{:?}", log)
    let pick = func(flag: bool) { if flag { "yes" } else { "no" } }
    println("{} {}", pick(true), pick(false))
}
