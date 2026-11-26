// Lifetimes that are missing (E0106).

package main

struct NoOrigin {
    text: &int64 //~ ERROR missing lifetime specifier
}

func two(a: &int64, b: &int64) -> &int64 { a } //~ ERROR missing lifetime specifier

func none() -> &int64 { loop {} } //~ ERROR missing lifetime specifier
