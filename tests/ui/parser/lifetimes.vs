//@ check-pass
//@ compile-flags: -Z parse-only
// Exception 13: quote-free lifetimes and borrows.

package main

struct Highlight<src> {
    text: &src string
    start: uint
    end: uint
}

implement<src> Highlight<src> {
    func part(&self) -> &src string {
        &self.text[self.start..self.end]
    }
}

func longest<a>(a: &a string, b: &a string) -> &a string {
    if a.len() >= b.len() { a } else { b }
}

func bytes(v: &[uint8]) -> &[uint8] { v }

const greeting: &static string = "hello"

func apply<F>(f: F) where F: for<a> Fn(&a string) -> &a string {}
