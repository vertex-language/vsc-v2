// Names that don't resolve, and Rust names with Vertex spellings.

func count(x: i64) -> int64 { //~ ERROR cannot find type `i64` in this scope
    let a = missing //~ ERROR cannot find value `missing` in this scope
    let b = Task { done: false } //~ ERROR cannot find struct, variant or union type `Task` in this scope
    let c: Vec<int64> = b //~ ERROR cannot find type `Vec` in this scope
    a
}
