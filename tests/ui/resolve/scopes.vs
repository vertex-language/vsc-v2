// Nested items can't see what's around them, and labels must exist.

func outer<T>(x: int64) {
    func inner() -> int64 { x } //~ ERROR can't capture dynamic environment in a function item
    func generic(v: T) {} //~ ERROR can't use generic parameters from outer item
    'a: loop {
        break 'b //~ ERROR use of undeclared label `'b`
    }
}

func (t: &Task) Method() -> bool { self.done } //~ ERROR expected value, found package `self`
struct Task { done: bool }
