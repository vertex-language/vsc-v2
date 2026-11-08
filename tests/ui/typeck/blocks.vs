//@ check-pass
//@ add-core-stubs
// Exception 10: a block's last expression is its value, but a block
// expected to be `()` discards it. Rust would need a `;` for each of these.

package main

func pop(v: &mut Vec<int64>) {
    v.pop()
}

func each(v: &mut Vec<int64>, ok: bool) {
    if ok { v.pop() }
    while ok { v.pop() }
    match ok {
        true => { v.pop() }
        false => {}
    }
    for i in 0..3 { v.pop() }
}

func value(ok: bool) -> int64 {
    let x = if ok { 1 } else { 2 }
    let y = { let z = x * 2
    z + 1 }
    let found = loop {
        if ok { break 7 }
    }
    let labeled = 'b: {
        if ok { break 'b 3 }
        4
    }
    x + y + found + labeled
}

func diverges(ok: bool) -> int64 {
    if ok { return 1 }
    panic("no")
}
