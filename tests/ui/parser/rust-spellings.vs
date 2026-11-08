//@ compile-flags: -Z parse-only
// Rust spellings that Vertex replaced are reported with the Vertex form.

package main

pub fn main() {
//~^ ERROR visibility is written `public` in Vertex
//~| ERROR functions are declared with `func` in Vertex
    let mut count = 0 //~ ERROR mutable bindings are declared with `var` in Vertex
    let text = fs::ReadToString("notes.txt") //~ ERROR paths are separated by `.` in Vertex, not `::`
    let label = match count {
        0 => "none", //~ ERROR match arms are separated by line breaks, not `,`
        _ => "many"
    }
}

#[derive(Debug)] //~ ERROR attributes are written `@name` in Vertex, not `#[name]`
struct Point { x: float64, y: float64 } //~ ERROR struct fields are separated by line breaks, not `,`

func longest<'a>(a: &'a string) -> &'a string { a }
//~^ ERROR Vertex declares origins, not lifetimes
//~| ERROR Vertex has no lifetimes
//~| ERROR Vertex has no lifetimes
