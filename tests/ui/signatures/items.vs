//@ check-pass
//@ compile-flags: -Z dump-signatures
// Classes (exception 3), function values (exception 14), traits,
// associated types, flat receivers (exception 8) and aliases.

package main

@lang("Class")
struct Class<T> { inner: T }

@lang("Func")
struct Func<F> { f: F }

class Counter { value: int64 }

func tick(c: Counter, f: func(int64) -> int64) -> func() -> int64 { loop {} }

trait Shape {
    type Unit
    func Area(&self) -> float64
    func Scaled(&self, by: float64) -> Self.Unit
}

struct Square { side: float64 }

implement Shape for Square {
    type Unit = float64
    func Area(&self) -> float64 { self.side * self.side }
    func Scaled(&self, by: float64) -> float64 { by }
}

func total<T: Shape>(items: &[T]) -> T.Unit where T.Unit: Shape { loop {} }
func qualified<T: Shape>(x: <T as Shape>.Unit) {}

struct Stack<T, const N: uint> { items: [T; N]; len: uint }

func (s: &mut Stack<T, N>) Push(x: T) {}

type Grid<T> = [[T; 3]; 3]
func center(g: Grid<int32>) -> int32 { 0 }

enum Tree<T> { Leaf(T); Node { left: uint; right: uint } }

unsafe extern "C" {
    type FILE
    func fopen(path: *const uint8, mode: *const uint8) -> *mut FILE
    safe func abs(n: int32) -> int32
}
