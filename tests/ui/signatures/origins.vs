//@ check-pass
//@ compile-flags: -Z dump-signatures
// Exception 13: origins are regions. Elided input references each get a
// region; `[a]` names a parameter's; `[a, b]` is a region both outlive.

package main

@lang("String")
struct String { len: uint }

@lang("Func")
struct Func<F> { f: F }

struct Highlight<src: Origin> { text: &[src] string }

implement<src: Origin> Highlight<src> {
    func Text(&self) -> &[src] string { self.text }
    func Self_(&self) -> &string { self.text }
}

func first(a: &string) -> &string { a }
func named(a: &string, b: &string) -> &[b] string { b }
func longest(a: &string, b: &string) -> &[a, b] string { a }
func forever() -> &[static] string { "x" }
func pick<x: Origin, y: Origin>(p: &[x] int64, q: &[y] int64) -> &[x] int64 where x: [y] { p }
func callback(f: func(&string) -> &string) {}
const Greeting: &string = "hi"
