//@ add-core-stubs
// Operators on types that don't implement their traits.

package main

struct V { x: int32 }

func add<T>(a: T, b: T) -> T {
    a + b //~ ERROR cannot add `T` to `T`
}

func main() {
    let a = V { x: 1 }
    let b = V { x: 2 }
    let c = a + b //~ ERROR cannot add `V` to `V`
    let d = a == b //~ ERROR binary operation `==` cannot be applied to type `V`
    let e = -a //~ ERROR cannot apply unary operator `-` to type `V`
    let v = vec[1, 2, 3]
    let f = v[true] //~ ERROR cannot index into a value of type `Vec<{integer}>`
}
