// `break` and `continue` need a loop, or a labeled block for `break`.

package main

func main() {
    break //~ ERROR `break` outside of a loop or labeled block
    continue //~ ERROR `continue` outside of a loop
    loop {
        let f = func() { break } //~ ERROR `break` inside of a function literal
    }
    while true {
        break 1 //~ ERROR `break` with value from a `while` loop
    }
    'b: {
        loop { continue 'b } //~ ERROR `continue` pointing to a labeled block
    }
}
