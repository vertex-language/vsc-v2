//@ add-core-stubs
// `Marker.hello` names the method on the type (exception 7), so a unit
// struct's methods are called on the value: `(Marker).hello()`.

package main

struct Marker

implement Marker {
    func hello(&self) -> int32 { 1 }
}

func main() {
    let a = Marker.hello() //~ ERROR `hello` is a method, so `Marker` needs a value to call it on
    let b = (Marker).hello()
}
