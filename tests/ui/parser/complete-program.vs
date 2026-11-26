//@ check-pass
//@ compile-flags: -Z parse-only
// The complete program from the usage guide.

package main

import { HashMap } from "std/collections"
import "std/fs"
import "std/io"
import "std/task"
import "net/http"

@derive(Debug, Clone, Copy, PartialEq)
enum Status { Todo; Doing; Done }

@derive(Debug, Clone)
struct Task {
    id: int64
    title: string
    status: Status
}

implement Task {
    public func new(id: int64, title: string) -> Task {
        Task { id, title, status: Status.Todo }
    }
    public func isOpen(&self) -> bool { self.status != Status.Done }
    public func advance(&mut self) {
        self.status = match self.status {
            Status.Todo => Status.Doing
            Status.Doing | Status.Done => Status.Done
        }
    }
}

trait Renderer {
    func render(&self) -> string
}

implement Renderer for Task {
    func render(&self) -> string {
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
    func from(e: io.Error) -> Self { StoreError.Io(e) }
}
implement From<http.Error> for StoreError {
    func from(e: http.Error) -> Self { StoreError.Http(e) }
}

func parseLine(line: &string, n: int64) -> Result<Task, StoreError> {
    let (rawId, title) = line.splitOnce('|').okOr(StoreError.Parse {
        line: n,
        message: "expected id|title".toString(),
    })?
    let id: int64 = rawId.trim().parse().mapErr(func(_) {
        StoreError.Parse { line: n, message: format("bad id {rawId:?}") }
    })?
    Ok(Task.new(id, title.trim().toString()))
}

class Store {
    tasks: HashMap<int64, Task>
    nextId: int64
}

implement Store {
    public func open(path: &string) -> Result<Store, StoreError> {
        let text = fs.readToString(path)?
        var tasks = HashMap.new()
        for (i, line) in text.lines().enumerate() {
            let task = parseLine(line, i as int64 + 1)?
            tasks.insert(task.id, task)
        }
        let nextId = tasks.keys().max().copied().unwrapOr(0) + 1
        Ok(Store { tasks, nextId })
    }
}

public func (s: &Store) add(title: string) -> int64 {
    let id = s.nextId
    s.nextId += 1
    s.tasks.insert(id, Task.new(id, title))
    id
}

public func (s: &Store) advance(id: int64) -> Result<(), StoreError> {
    let task = s.tasks.getMut(&id).okOr(StoreError.NotFound(id))?
    task.advance()
    Ok(())
}

public func (s: &Store) openTasks() -> Vec<Task> {
    var open: Vec<Task> = s.tasks.values()
        .filter(func(t) { t.isOpen() })
        .cloned()
        .collect()
    open.sortByKey(func(t) { t.id })
    open
}

func printAll<T: Renderer>(items: &[T]) {
    for item in items {
        println("{}", item.render())
    }
}

func longestTitle<a>(a: &a Task, b: &a Task) -> &a string {
    if a.title.len() >= b.title.len() { &a.title } else { &b.title }
}

async func syncRemote(store: &Store, url: &string) -> Result<uint, StoreError> {
    let body = http.get(url).await?.text().await?
    var added = 0
    for line in body.lines() {
        store.add(line.toString())
        added += 1
    }
    Ok(added)
}

async func main() -> Result<(), StoreError> {
    let store = Store.open("tasks.txt")?
    let id = store.add("write the usage guide".toString())
    store.advance(id)?

    let (a, b) = task.join(
        syncRemote(&store, "https://example.com/a.txt"),
        syncRemote(&store, "https://example.com/b.txt"),
    ).await
    println("synced {} + {} tasks", a?, b?)

    let open = store.openTasks()
    printAll(&open)

    let doing = open.iter().filter(func(t) { t.status == Status.Doing }).count()
    println("{doing} in progress")

    if let [first, second, ..] = open.asSlice() {
        println("longest title: {}", longestTitle(first, second))
    }

    match store.advance(999) {
        Ok(()) => println("advanced")
        Err(StoreError.NotFound(id)) => println("no task #{id}")
        Err(e) => return Err(e)
    }
    Ok(())
}
