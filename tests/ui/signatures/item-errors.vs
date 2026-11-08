// Errors in item signatures.

package main

struct Pair<T, U> { a: T; b: U }

func one(p: Pair<int64>) {} //~ ERROR struct takes 2 generic arguments but 1 generic argument was supplied
func none(p: Pair) {} //~ ERROR missing generics for struct `Pair`
func infer(x: _) {} //~ ERROR the placeholder `_` is not allowed within types on item signatures
func generic<T>(x: T.Item) {} //~ ERROR associated type `Item` not found for `T`

type Loop = Loop2 //~ ERROR cycle detected when computing the type of `Loop`
type Loop2 = Loop

trait Shape {
    func Area(&self) -> float64
    func Name(&self) -> int64 { 0 }
}

struct Circle { r: float64 }

implement Shape for Circle { //~ ERROR not all trait items implemented, missing: `Area`
    func Perimeter(&self) -> float64 { 0.0 } //~ ERROR method `Perimeter` is not a member of trait `Shape`
}

struct Task { id: int64 }

func (t: &Task) Id() -> int64 { t.id }
func (t: &Task) Id() -> int64 { 0 } //~ ERROR duplicate definitions with name `Id`

func (x: int64) Double() -> int64 { x * 2 } //~ ERROR cannot define inherent methods for `int64`
