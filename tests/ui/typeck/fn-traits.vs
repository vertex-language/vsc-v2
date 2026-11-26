//@ add-core-stubs
// Calls through the function traits: a literal that mutates what it
// captures is only `FnMut`, and a value that isn't callable is an error.

package main

func callFn<F: Fn() -> int32>(f: F) -> int32 {
    f()
}

func callMut<F: FnMut() -> int32>(var f: F) -> int32 {
    f()
}

func callWith<F: Fn(int32) -> bool>(f: F) -> bool {
    f(1, 2) //~ ERROR this function takes 1 argument but 2 arguments were supplied
}

func main() {
    var n = 0
    let bump = func() -> int32 { //~ ERROR expected a function literal that implements the `Fn` trait
        n += 1
        n
    }
    callFn(bump)
    let ok = func() -> int32 {
        n += 1
        n
    }
    callMut(ok)
    let x = 5
    x() //~ ERROR expected function, found `{integer}`
}
