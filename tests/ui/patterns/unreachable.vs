//@ check-pass
//@ add-core-stubs
// An arm no value can reach is warned about.

package main

func f(n: uint8, o: Option<int32>) {
    match n {
        _ => {}
        5 => {} //~ WARN unreachable pattern
    }
    match o {
        Option.Some(_) => {}
        Option.None => {}
        Option.Some(3) => {} //~ WARN unreachable pattern
    }
}
