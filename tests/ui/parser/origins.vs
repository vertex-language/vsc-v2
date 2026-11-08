//@ check-pass
//@ compile-flags: -Z parse-only
// Exception 13: origins name what a reference borrows from.

package main

struct Highlight<src: Origin> {
    text: &[src] string
    start: uint
    end: uint
}

implement<src: Origin> Highlight<src> {
    func Part(&self) -> &[src] string {
        &self.text[self.start..self.end]
    }
}

func longest(a: &string, b: &string) -> &[a, b] string {
    if a.Len() >= b.Len() { a } else { b }
}

func bytes(v: &[uint8]) -> &[uint8] { v }

const Greeting: &[static] string = "hello"

func apply<F>(f: F) where F: for<a: Origin> Fn(&[a] string) -> &[a] string {}
