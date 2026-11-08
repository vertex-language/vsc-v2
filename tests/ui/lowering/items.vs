//@ check-pass
//@ compile-flags: -Z unpretty=hir
// Items: flat receivers become impls whose receiver type's names are
// generic parameters (exception 8), origins become lifetimes (exception
// 13), `var` makes every binding mutable (exception 5).

package main

struct Stack<T> { items: [T; 4]; len: uint }

func (s: &mut Stack<T>) Push(x: T) {
    s.len += 1
}

struct Highlight<src: Origin> { text: &[src] string }

func (h: &Highlight<src>) Text() -> &[src] string { h.text }

func longest(a: &string, b: &string) -> &[a, b] string {
    if a.Len() >= b.Len() { a } else { b }
}

func pick<x: Origin, y: Origin>(p: &[x] int64, q: &[y] int64) -> &[x] int64 where x: [y] {
    p
}

trait Shape {
    type Unit
    const Sides: uint
    func Area(&self) -> float64
    func Name(&self) -> string { "shape" }
}

class Counter { value: int64 }

unsafe extern "C" {
    func abs(n: int32) -> int32
    static errno: int32
}

func main() {
    var (a, b) = (1, 2)
    let (c, var d) = (3, 4)
}
