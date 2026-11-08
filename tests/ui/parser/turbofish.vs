//@ compile-flags: -Z parse-only
// Rust's `::<…>` is reported with the fix: Vertex writes `make<T>()`.

package main

func main() {
    let n = parse::<int64>("5") //~ ERROR generic arguments in an expression have no `::` in Vertex
    let total = xs.iter().sum::<uint64>() //~ ERROR generic arguments in an expression have no `::` in Vertex
    let v = Vec::<int32>.new() //~ ERROR generic arguments in an expression have no `::` in Vertex
}
