//@ compile-flags: -Z parse-only
func main() {
    let a = 1 let b = 2 //~ ERROR expected `;` or a line break, found keyword `let`
}
