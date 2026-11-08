// Origins that are missing or can't be resolved to a region.

package main

struct NoOrigin {
    text: &int64 //~ ERROR missing origin specifier
}

func two(a: &int64, b: &int64) -> &int64 { a } //~ ERROR missing origin specifier

func none() -> &int64 { loop {} } //~ ERROR missing origin specifier

func notRef(a: int64) -> &[a] int64 { loop {} } //~ ERROR origin `a` names a parameter that doesn't borrow

func later(a: &[b] int64, b: &int64) {} //~ ERROR origin `b` names a parameter declared after it
