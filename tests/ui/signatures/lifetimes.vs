//@ check-pass
//@ compile-flags: -Z dump-signatures
// Exception 13: lifetimes are regions. Elided input references each get a
// region, and an elided output follows Rust's elision rules.

package main

@lang("String")
struct String { len: uint }

@lang("Func")
struct Func<F> { f: F }

struct Highlight<src> { text: &src string }

implement<src> Highlight<src> {
    func Text(&self) -> &src string { self.text }
    func Self_(&self) -> &string { self.text }
}

func first(a: &string) -> &string { a }
func named<b>(a: &string, b: &b string) -> &b string { b }
func longest<l>(a: &l string, b: &l string) -> &l string { a }
func forever() -> &static string { "x" }
func pick<x, y>(p: &x int64, q: &y int64) -> &x int64 where x: y { p }
func callback(f: func(&string) -> &string) {}
const Greeting: &string = "hi"
