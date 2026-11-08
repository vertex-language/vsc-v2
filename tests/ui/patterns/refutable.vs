//@ add-core-stubs
// E0005: `let`, parameters and `for` bindings must match every value.

package main

func f(o: Option<int32>, os: &[Option<int32>]) {
    let Option.Some(v) = o //~ ERROR refutable pattern in local binding
    for Option.Some(z) in os {} //~ ERROR refutable pattern in `for` loop binding
    let Option.Some(w) = o else { return }
    let g = func(Option.Some(x): Option<int32>) {} //~ ERROR refutable pattern in function argument
}
