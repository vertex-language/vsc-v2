// Errors found while lowering.

package main

func main() {
    let t = (1, 2, 3)
    let (a, .., b, ..) = t //~ ERROR `..` can only be used once per tuple pattern
    let x = _ //~ ERROR in expressions, `_` can only be used on the left-hand side of an assignment
}
