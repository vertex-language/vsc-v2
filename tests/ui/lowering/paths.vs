//@ check-pass
//@ compile-flags: -Z unpretty=hir
// Exception 7: the segments of `a.b.c` that name no item become fields,
// a callee's last one a method, and the first one after a type an
// associated item.

package main

enum Status { Open; Done(int64) }

struct Inner { count: int64 }
struct Task { id: int64; inner: Inner }

implement Task {
    func New(id: int64) -> Task { Task { id, inner: Inner { count: 0 } } }
    func Bump(&mut self) { self.inner.count += 1 }
}

func main() {
    var t = Task.New(1)
    let c = t.inner.count
    t.Bump()
    t.inner.Clear()
    let s = Status.Done(c)
    let o = Status.Open
    let first = (c, s).0
}
