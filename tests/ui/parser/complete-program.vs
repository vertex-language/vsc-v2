//@ check-pass
//@ compile-flags: -Z parse-only
// The complete program from the usage guide.

package main

import (
    "std/collections"
    "std/fs"
    "std/io"
    "std/task"
    "net/http"
)

@derive(Debug, Clone, Copy, PartialEq)
enum Status { Todo; Doing; Done }

@derive(Debug, Clone)
struct Task {
    id: int64
    title: string
    status: Status
}

implement Task {
    public func New(id: int64, title: string) -> Task {
        Task { id, title, status: Status.Todo }
    }
    public func IsOpen(&self) -> bool { self.status != Status.Done }
    public func Advance(&mut self) {
        self.status = match self.status {
            Status.Todo => Status.Doing
            Status.Doing | Status.Done => Status.Done
        }
    }
}

trait Renderer {
    func Render(&self) -> string
}

implement Renderer for Task {
    func Render(&self) -> string {
        let mark = match self.status {
            Status.Todo => " "
            Status.Doing => "~"
            Status.Done => "x"
        }
        format("[{mark}] #{} {}", self.id, self.title)
    }
}

@derive(Debug)
enum StoreError {
    Io(io.Error)
    Http(http.Error)
    Parse { line: int64; message: string }
    NotFound(int64)
}

implement From<io.Error> for StoreError {
    func From(e: io.Error) -> Self { StoreError.Io(e) }
}
implement From<http.Error> for StoreError {
    func From(e: http.Error) -> Self { StoreError.Http(e) }
}

func parseLine(line: &string, n: int64) -> Result<Task, StoreError> {
    let (rawID, title) = line.SplitOnce('|').OkOr(StoreError.Parse {
        line: n,
        message: "expected id|title".ToString(),
    })?
    let id: int64 = rawID.Trim().Parse().MapErr(func(_) {
        StoreError.Parse { line: n, message: format("bad id {rawID:?}") }
    })?
    Ok(Task.New(id, title.Trim().ToString()))
}

class Store {
    tasks: collections.HashMap<int64, Task>
    nextID: int64
}

implement Store {
    public func Open(path: &string) -> Result<Store, StoreError> {
        let text = fs.ReadToString(path)?
        var tasks = collections.HashMap.New()
        for (i, line) in text.Lines().Enumerate() {
            let task = parseLine(line, i as int64 + 1)?
            tasks.Insert(task.id, task)
        }
        let nextID = tasks.Keys().Max().Copied().UnwrapOr(0) + 1
        Ok(Store { tasks, nextID })
    }
}

public func (s: &Store) Add(title: string) -> int64 {
    let id = s.nextID
    s.nextID += 1
    s.tasks.Insert(id, Task.New(id, title))
    id
}

public func (s: &Store) Advance(id: int64) -> Result<(), StoreError> {
    let task = s.tasks.GetMut(&id).OkOr(StoreError.NotFound(id))?
    task.Advance()
    Ok(())
}

public func (s: &Store) OpenTasks() -> Vec<Task> {
    var open: Vec<Task> = s.tasks.Values()
        .Filter(func(t) { t.IsOpen() })
        .Cloned()
        .Collect()
    open.SortByKey(func(t) { t.id })
    open
}

func printAll<T: Renderer>(items: &[T]) {
    for item in items {
        println("{}", item.Render())
    }
}

func longestTitle(a: &Task, b: &Task) -> &[a, b] string {
    if a.title.Len() >= b.title.Len() { &a.title } else { &b.title }
}

async func syncRemote(store: &Store, url: &string) -> Result<uint, StoreError> {
    let body = http.Get(url).await?.Text().await?
    var added = 0
    for line in body.Lines() {
        store.Add(line.ToString())
        added += 1
    }
    Ok(added)
}

async func main() -> Result<(), StoreError> {
    let store = Store.Open("tasks.txt")?
    let id = store.Add("write the usage guide".ToString())
    store.Advance(id)?

    let (a, b) = task.Join(
        syncRemote(&store, "https://example.com/a.txt"),
        syncRemote(&store, "https://example.com/b.txt"),
    ).await
    println("synced {} + {} tasks", a?, b?)

    let open = store.OpenTasks()
    printAll(&open)

    let doing = open.Iter().Filter(func(t) { t.status == Status.Doing }).Count()
    println("{doing} in progress")

    if let [first, second, ..] = open.AsSlice() {
        println("longest title: {}", longestTitle(first, second))
    }

    match store.Advance(999) {
        Ok(()) => println("advanced")
        Err(StoreError.NotFound(id)) => println("no task #{id}")
        Err(e) => return Err(e)
    }
    Ok(())
}
