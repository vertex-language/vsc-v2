//@ add-core-stubs
// Type errors in bodies.

package main

struct Point { x: float64; y: float64 }

func takes(n: int64) -> int64 { n }

func main() {
    let a: int64 = true //~ ERROR mismatched types
    let b = takes(1, 2) //~ ERROR this function takes 1 argument but 2 arguments were supplied
    let c = takes("hi") //~ ERROR mismatched types
    let p = Point { x: 1.0 } //~ ERROR missing field `y` in initializer of `Point`
    let q = Point { x: 1.0, y: 2.0, z: 3.0 } //~ ERROR struct `Point` has no field named `z`
    let d = p.z //~ ERROR no field `z` on type `Point`
    let e = p.Norm() //~ ERROR no method named `Norm` found for struct `Point` in the current scope
    let f = 5() //~ ERROR expected function, found `{integer}`
    let g = !3.5 //~ ERROR cannot apply unary operator `!` to type `{float}`
    let h = *a //~ ERROR type `int64` cannot be dereferenced
    let i = if true { 1 } else { "no" } //~ ERROR `if` and `else` have incompatible types
    let j = p == p //~ ERROR binary operation `==` cannot be applied to type `Point`
    let k = 1 as bool //~ ERROR cannot cast `int32` as `bool`
    3 = 4 //~ ERROR invalid left-hand side of assignment
}

func ret() -> int64 {
    return //~ ERROR `return` without a value in a function whose return type is not `()`
}

trait Named { func Name(&self) -> int64 }

func named<T: Named>(x: &T) -> int64 { x.Name() }

func unbound() -> int64 {
    named(&5) //~ ERROR the trait bound `{integer}: Named` is not satisfied
}

func unknown() {
    let v = vec[] //~ ERROR type annotations needed
}
