//@ check-pass
//@ add-core-stubs
// `{name}` captures `name` from the scope; arguments can be positional or named.

func main() {
    let name = "Ada"
    let age = 36
    println("{name} is {age}")
    println("{} is {}", name, age)
    println("{who} is {n:>4}", who = name, n = age)
    eprintln("{:?} {:#x} {:.2}", name, 255, 3.14159)
    let s = format("{name}!")
    assert(age > 0, "age was {age}")
    assertEq(age, 36)
    println()
}
