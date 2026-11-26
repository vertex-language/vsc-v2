//@ check-pass
//@ add-core-stubs
//@ compile-flags: -Z dump-types
// Trait methods on bounded parameters and on types with impls; associated
// types normalize through the impl, or stay as projections on a parameter.

package main

trait Shape {
    type Unit
    func Area(&self) -> float64
    func Unit(&self) -> Self.Unit
    func Twice(&self) -> float64 { self.Area() * 2.0 }
}

struct Square { side: float64 }

implement Shape for Square {
    type Unit = float64
    func Area(&self) -> float64 { self.side * self.side }
    func Unit(&self) -> float64 { self.side }
}

func area<T: Shape>(s: &T) -> float64 { s.Area() }
func unit<T: Shape>(s: &T) -> T.Unit { s.Unit() }

func main() {
    let sq = Square { side: 2.0 }
    let a = area(&sq)
    let u = unit(&sq)
    let direct = sq.Unit()
    let t = sq.Twice()
}
