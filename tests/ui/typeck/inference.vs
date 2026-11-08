//@ check-pass
//@ add-core-stubs
//@ compile-flags: -Z dump-types
// Inference: literals default to `int32` and `float64`; methods and
// associated functions resolve through impls and flat receivers; `?`,
// `for` and the format built-ins type check through their lang items.

package main

struct Task { id: int64; done: bool; tags: Vec<string> }

implement Task {
    func New(id: int64) -> Task { Task { id, done: false, tags: vec[] } }
    func IsOpen(&self) -> bool { !self.done }
}

func (t: &mut Task) Close() { t.done = true }

enum Status { Open; Done(int64) }

func sum(xs: &[int64]) -> int64 {
    var total = 0
    for x in xs {
        total += *x
    }
    total
}

func next(v: Option<int64>) -> Option<int64> {
    let x = v?
    Option.Some(x + 1)
}

func main() {
    var t = Task.New(1)
    let open = t.IsOpen()
    t.Close()
    let n = t.tags.len()
    let code = match Status.Done(3) {
        Status.Open => 0
        Status.Done(c) if c > 2 => c
        Status.Done(_) => 1
    }
    let xs = [1, 2, 3]
    let total = sum(&xs)
    let ratio = 2.5
    let small = 7u8
    let pair = (open, n)
    let doubled = vec[0; n]
    for i in 0..3 {
        let k = i
    }
    println("{} {} {:?}", total, code, pair)
    let msg = format("{total:>5}")
    assertEq(total, 6)
}
