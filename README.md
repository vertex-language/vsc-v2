# Vertex (`vsc`)

**Vertex** is a statically typed, memory-safe systems programming language that compiles directly to native machine code. It guarantees memory safety and data-race freedom at compile time through ownership and borrowing — with no garbage collector or runtime VM — while keeping everyday code clean and readable with line-oriented syntax, `camelCase` identifiers, dot-separated paths, and zero-cost `async`/`await`.

This repository contains **`vsc`**, the Vertex compiler, along with the dependency-free `core` library (`library/core`) and compiler tooling.

---

## Language Tour

### 1. Packages, Imports, and Entry Point

Every directory is a **package**, and every `.vs` file in that directory belongs to it. Programs start at `func main()` in `package main`. Line breaks end statements — semicolons are only needed when placing multiple items on the same line.

```vertex
package main

import "std/fs"
import { HashMap } from "std/collections"
import geo, { Point } from "./geometry"

func main() {
    let origin = Point { x: 0.0, y: 0.0 }
    let text = fs.readToString("notes.txt").unwrap()
    println("Read {} bytes from origin ({}, {})", text.len(), origin.x, origin.y)
}
```

### 2. Variables, Mutability, and Types

Bindings are **immutable by default** (`let`). Use `var` when a value needs to be reassigned or mutated in place.

```vertex
let limit = 100                  // immutable (inferred int32)
var count: uint64 = 0            // mutable with explicit type
count += 1

let name: &string = "Vertex"     // borrowed UTF-8 string slice
let owned: string = "Hello".toString()
let flags: [bool; 3] = [true, false, true]
let bytes: &[uint8] = &[1, 2, 3]
```

Built-in scalar types have explicit, unambiguous names:

- **Signed integers:** `int8`, `int16`, `int32`, `int64`, `int128`, `int` (pointer-sized)
- **Unsigned integers:** `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uint` (pointer-sized)
- **Floating point:** `float32`, `float64`
- **Other primitives:** `bool`, `char`, `string` / `&string`, `never` (diverging expressions)

### 3. Ownership, Borrowing, and Lifetimes

Every value in Vertex has a single owner. When the owner leaves scope, its resources are deterministically freed (via `Drop`). Values can be moved to a new owner or temporarily borrowed with references checked at compile time:

- Any number of shared, read-only references (`&T`), **or**
- Exactly one exclusive mutable reference (`&mut T`).

```vertex
func appendGreeting(buf: &mut string, target: &string) {
    buf.pushStr(", ")
    buf.pushStr(target)
}

// Lifetimes connect borrowed inputs to borrowed outputs without quotes or sigils
func longest<a>(a: &a string, b: &a string) -> &a string {
    if a.len() >= b.len() { a } else { b }
}
```

### 4. Structs, Enums, Pattern Matching, and Error Handling

Data is modeled with value-type `struct`s, heap-shared `class`es, and tagged `enum`s whose variants can carry payloads. `match` expressions are checked for exhaustiveness at compile time.

```vertex
@derive(Clone, Debug, PartialEq)
public struct Task {
    public id: uint64
    public title: string
    status: Status
}

public enum Status {
    Todo
    InProgress(uint8)            // percent complete
    Done
}

func describe(status: &Status) -> &string {
    match status {
        Status.Todo => "not started"
        Status.InProgress(0) => "just started"
        Status.InProgress(_) => "in progress"
        Status.Done => "completed"
    }
}
```

Recoverable errors are values using `Option<T>` (`Some(v)` / `None`) and `Result<T, E>` (`Ok(v)` / `Err(e)`), with `?` propagating errors early:

```vertex
enum ParseError {
    Empty
    InvalidDigit
}

func firstNonEmpty(input: &string) -> Result<&string, ParseError> {
    if input.isEmpty() {
        return Err(ParseError.Empty)
    }
    Ok(input)
}

func loadTitle(raw: &string) -> Result<string, ParseError> {
    let clean = firstNonEmpty(raw)?
    Ok(clean.toString())
}
```

### 5. Methods, Traits, and Generics

Methods can be grouped in an `implement` block or attached individually with a flat receiver. Shared behavior is defined with `trait` and implemented explicitly via `implement Trait for Type`.

```vertex
public struct Circle {
    radius: float64
}

// Flat receiver method
func (c: &Circle) diameter() -> float64 {
    c.radius * 2.0
}

public trait Shape {
    func area(&self) -> float64
    func label(&self) -> &string { "shape" }
}

implement Shape for Circle {
    func area(&self) -> float64 { 3.14159265 * self.radius * self.radius }
    func label(&self) -> &string { "circle" }
}

// Static dispatch via generic bounds
func larger<S: Shape>(a: S, b: S) -> S {
    if a.area() >= b.area() { a } else { b }
}

// Dynamic dispatch via trait objects
func totalArea(shapes: &[&dyn Shape]) -> float64 {
    var sum = 0.0
    for s in shapes {
        sum += s.area()
    }
    sum
}
```

Visibility is controlled explicitly by keywords: private to the current package by default, `package` for crate-internal visibility, and `public` for external callers.

### 6. Closures and Function Values

Closures use `func(...) { ... }` and capture variables from their environment automatically.

```vertex
let factor = 3
let scale = func(x: int64) -> int64 { x * factor }

let doubled = vec[1, 2, 3, 4]
    .iter()
    .map(func(n) { scale(*n) })
    .collect()
```

### 7. Zero-Cost `async` / `await`

`async func` and `async { ... }` blocks compile directly into stackless `Future` state machines — storing only the locals live across `.await` points with per-state drop tracking.

```vertex
async func computeSum(a: int64, b: int64) -> int64 {
    let x = fetchPart(a).await
    let y = fetchPart(b).await
    x + y
}
```

### 8. Low-Level Control, `unsafe`, and C FFI

When writing allocators, kernels, OS bindings, or hardware drivers, Vertex lets you opt into raw pointers (`*const T`, `*mut T`), inline assembly, and C ABI calls inside explicit `unsafe` blocks:

```vertex
unsafe extern "C" {
    func write(fd: int32, buf: *const uint8, count: uint) -> int
}

func writeStdout(msg: &string) {
    unsafe {
        write(1, msg.asPtr(), msg.len())
    }
}
```

---

## Compiler Pipeline

`vsc` compiles `.vs` source packages to native object files, libraries, and executables through a modular multi-stage pipeline powered by the Obsidian native backend:

```text
  .vs source files
        │
        ▼
  vsc_lexer + vsc_parse          Tokenization & line-sensitive AST parsing
        │
        ▼
  vsc_expand + vsc_builtin_macros Package loading, @cfg filtering & built-in expansion
        │
        ▼
  vsc_resolve                    Early & late name, lifetime, and label resolution
        │
        ▼
  vsc_ast_lowering               AST -> HIR lowering & desugaring
        │
        ▼
  vsc_hir_analysis + typeck      Type inference, trait selection & coherence
        │
        ▼
  vsc_pattern_analysis + mir_build THIR lowering, exhaustiveness & MIR construction
        │
        ▼
  vsc_borrowck                   Move, initialization, and borrow checking
        │
        ▼
  vsc_mir_transform              Coroutine state machines, drop elaboration & CFG passes
        │
        ▼
  vsc_monomorphize               Polymorphic specialization & collector
        │
        ▼
  vsc_codegen_obsidian           Native IR lowering, optimization, object emission & linking
```

### Repository Structure

- `compiler/` — Modular compiler crates (`vsc`, `vsc_driver_impl`, `vsc_lexer`, `vsc_parse`, `vsc_expand`, `vsc_resolve`, `vsc_hir*`, `vsc_trait_selection`, `vsc_mir*`, `vsc_borrowck`, `vsc_const_eval`, `vsc_monomorphize`, `vsc_codegen_obsidian`, etc.)
- `library/core/` — Dependency-free Vertex `core` library (primitives, `Option`, `Result`, `Iterator`, `Future`, `Pin`, `Task`, `Cell`, `Sync`, `Slice`, `Ptr`, `Mem`, `Ops`, `Intrinsics`)
- `src/doc/reference/` — Language grammar ([grammar.md](src/doc/reference/grammar.md)) and usage reference ([usage.md](src/doc/reference/usage.md))
- `src/tools/` — Repository tooling (`tidy`, `compiletest`, `bootstrap`)
- `tests/ui/` — End-to-end compiler UI and diagnostic tests

---

## Using `vsc`

### Building the Compiler

Build the `vsc` driver binary from the workspace root (requires a host toolchain and the sibling `../obsidian` backend repository):

```bash
cargo build --release --bin vsc
```

### Compiling Vertex Programs

```bash
# Compile an executable with optimizations (-O is equivalent to -C opt-level=2)
vsc main.vs -O -o app

# Compile with aggressive optimization (-O3) and external library packages
vsc src/main.vs -C opt-level=3 --extern net=./bin/libnet.rlib -o server

# Build a reusable library package (.rlib)
vsc src/lib.vs --crate-type=lib --crate-name=mylib -o bin/libmylib.rlib

# Type-check only (no codegen)
vsc main.vs --emit=metadata

# Cross-compile an object file for another target
vsc main.vs --target=x86_64-unknown-linux-gnu --emit=obj -o main.o

# Inspect intermediate stages or run in the MIR interpreter
vsc main.vs -Z dump-mir
vsc main.vs -Z run-mir
```

### Supported Native Targets

- **macOS:** `aarch64-apple-darwin`, `x86_64-apple-darwin`
- **Linux:** `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`
- **Windows:** `x86_64-pc-windows-msvc`, `aarch64-pc-windows-msvc`

### Running Tests & Checks

```bash
# Run the full compiler test suite
cargo test --workspace

# Run repository style and dependency checks
cargo run -p tidy
```
