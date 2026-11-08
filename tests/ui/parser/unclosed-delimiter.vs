//@ compile-flags: -Z parse-only
func main() {
    let v = vec[1, 2, 3
} //~ ERROR mismatched closing delimiter: `}`
