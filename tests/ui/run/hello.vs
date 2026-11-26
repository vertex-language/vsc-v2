//@ run-pass
// The interpreter runs `main`; the format built-ins print as Rust's do.

package main

func main() {
    let name = "Ada"
    let age = 36
    println("{name} is {age}")
    println("{} + {} = {}", 2, 3, 2 + 3)
    print("no newline, ")
    println("then one")
    eprintln("to stderr")
}
