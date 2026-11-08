//@ run-pass
//@ add-core-stubs
// Generic arguments in expressions, written without `::`: on functions,
// types, struct literals, variants and methods. A `<` that can't start
// type arguments (a `camelCase` value after it) stays a comparison.

package main

struct Pair<T> { a: T; b: T }

implement<T> Pair<T> {
    func new(a: T, b: T) -> Pair<T> { Pair { a, b } }
}

trait Zero {
    func zero() -> Self
}

implement Zero for int64 { func zero() -> Self { 7 } }
implement Zero for float64 { func zero() -> Self { 0.5 } }

func make<T: Zero>() -> T { T.zero() }

func count<const N: uint>() -> uint { N }

struct S

implement S {
    func get<T: Zero>(&self) -> T { T.zero() }
}

func main() {
    let half = make<float64>()
    let typed: float64 = make<float64>()
    let p = Pair<int64>.new(1, 2)
    let q = Pair<int64> { a: 3, b: 4 }
    let g = S.get<int64>(&S)
    let s = S
    let h = s.get<float64>()
    let n = Option<int32>.None
    let a = 1
    let b = 2
    let c = 3
    let both = (a < b, c > (a))
    println("{half} {typed} {} {} {g} {h} {n:?} {both:?}", p.a + p.b, q.a + q.b)
    println("{} {}", make<int64>() + 1, count<3>())
}
