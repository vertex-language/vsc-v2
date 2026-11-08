//@ check-pass
//@ compile-flags: -Z parse-only
// Exception 10: which line feeds end a statement.

package main

func main() {
    // `.` continues the line before.
    let total = orders
        .Iter()
        .Map(func(o) { o.amount })
        .Sum()

    // A binary operator followed by a space continues it.
    let ok = hasName
        && hasEmail
        && age >= 18

    // Inside parentheses, line feeds are whitespace.
    let p = Point.New(
        1.0,
        2.0,
    )

    // `-x` starts a new statement; `else` continues.
    var n = 1
    -n
    if n > 0 {
        n -= 1
    }
    else {
        n += 1
    }

    // `;` shares a line, and a block-like statement needs no separator.
    let a = 1; let b = 2
    if a < b { println("less") } println("done")
}

// `{` may start the next line after a declaration header.
func later()
{
}

struct Point
{
    x: float64
    y: float64
}
