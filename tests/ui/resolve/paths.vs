//@ check-pass
//@ add-core-stubs
// Exception 7: `.` paths resolve as far as they name packages, enums and
// traits; the rest are fields, methods and associated items.

enum Status { Todo; Doing; Done }

struct Task {
    id: uint64
    status: Status
}

implement Task {
    func New(id: uint64) -> Task { Task { id, status: Status.Todo } }
    func IsOpen(&self) -> bool { self.status.IsDone() }
}

func (s: &Status) IsDone() -> bool { false }

implement String {
    func From(s: &string) -> string { loop {} }
    func Len(&self) -> uint { 0 }
}

func (t: &Task) Advance() -> Status {
    match t.status {
        Status.Todo => Status.Doing
        Status.Doing | Status.Done => Status.Done
    }
}

func main() {
    var t = Task.New(1)
    let open = t.IsOpen()
    let next = t.Advance()
    let n = string.From("x").Len()
    println("{}", open)
}
