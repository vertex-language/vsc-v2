// Built-ins check their format strings when compiling, as Rust's do.

func main() {
    let age = 36
    println("{} is {}", age) //~ ERROR 2 positional arguments in format string, but there is 1 argument
    println("{1}", age) //~ ERROR invalid reference to positional argument 1 (there is 1 argument)
    println("done", age) //~ ERROR argument never used
    println("{:z}", age) //~ ERROR unknown format trait `z`
    println("{age") //~ ERROR invalid format string: expected `}` but string was terminated
    println(age) //~ ERROR format argument must be a string literal
    assertEq(age) //~ ERROR `assertEq` requires two values to compare
}
