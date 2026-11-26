// Nested items can't see what's around them, and labels must exist.
// `break name` with no enclosing `name:` breaks with the value `name`.

func outer<T>(x: int64) {
    func inner() -> int64 { x } //~ ERROR can't capture dynamic environment in a function item
    func generic(v: T) {} //~ ERROR can't use generic parameters from outer item
    a: loop {
        continue b //~ ERROR use of undeclared label `b`
        break c //~ ERROR cannot find value `c` in this scope
    }
}

func (t: &Task) method() -> bool { self.done } //~ ERROR expected value, found package `self`
struct Task { done: bool }
