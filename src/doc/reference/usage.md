# Vertex v2 usage

Vertex is **Rust with fourteen exceptions**. Programs are written, checked and compiled
with Rust's meaning: ownership, borrowing, traits, generics, `Result` and `?`, async. The
fourteen exceptions change how some of it is spelled, using syntax taken from Go, Swift,
TypeScript and Scala.

This guide shows each feature as a pair: the Rust code first, then the same code in
Vertex. It covers `.vs` source files. The formal grammar is
[grammar.md](grammar.md), and Rust's own grammar is
[rust_grammar.md](rust_grammar.md).

---

## Contents

1. [The fourteen exceptions](#the-fourteen-exceptions)
2. [Hello, packages and imports](#hello-packages-and-imports)
3. [Variables and mutability](#variables-and-mutability)
4. [Types](#types)
5. [Functions](#functions)
6. [Line breaks, not semicolons](#line-breaks-not-semicolons)
7. [Control flow](#control-flow)
8. [Structs](#structs)
9. [Enums and `match`](#enums-and-match)
10. [Methods: `implement` and flat receivers](#methods-implement-and-flat-receivers)
11. [Traits](#traits)
12. [Ownership and borrowing](#ownership-and-borrowing)
13. [Lifetimes](#lifetimes)
14. [Errors](#errors)
15. [Function literals and `func` values](#function-literals-and-func-values)
16. [Classes](#classes)
17. [Collections and iterators](#collections-and-iterators)
18. [Strings and formatting](#strings-and-formatting)
19. [Attributes](#attributes)
20. [Visibility and naming](#visibility-and-naming)
21. [Threads and async](#threads-and-async)
22. [`unsafe` and C](#unsafe-and-c)
23. [Using Vertex from Rust](#using-vertex-from-rust)
24. [Built-ins](#built-ins)
25. [A complete program](#a-complete-program)
26. [Quick reference](#quick-reference)

---

## The fourteen exceptions

Everything not in this table is written as in Rust.

| # | Rust | Vertex | Source |
| --- | --- | --- | --- |
| 1 | `mod`, `use std::fs;`, `use a::B;` | `package main`, `import "std/fs"`, `import { B } from "a"` | Go, TypeScript |
| 2 | `i64`, `f64`, `usize`, `String`, `&str`, `!` | `int64`, `float64`, `uint`, `string`, `&string`, `never` | Go |
| 3 | `Rc<RefCell<T>>` | `class` | Swift |
| 4 | `fn` | `func` | Go, Swift |
| 5 | `let mut x` | `var x` | Swift, Go |
| 6 | `pub`, `pub(crate)` | `public`, `package` | Swift |
| 7 | `fs::read_to_string`, `Status::Todo` | `fs.readToString`, `Status.Todo` | Go, Swift |
| 8 | `impl Task { … }` | `implement Task { … }`, or flat `func (t: &Task) m()` | Rust spelled out, Go |
| 9 | `#[derive(Debug)]` | `@derive(Debug)` | Swift |
| 10 | `;`, commas after fields, variants and arms | line breaks (`;` only to share a line) | Swift, Scala, Go |
| 11 | `snake_case` | `camelCase` (`PascalCase` for types, traits, variants) | TypeScript, Swift |
| 12 | `println!`, `format!`, `vec!` | `println`, `format`, `vec` | Go |
| 13 | `'a`, `&'a T`, `&'static str`, `'outer:` | `a`, `&a T`, `&static string`, `outer:` | Go, Swift |
| 14 | `move \|x\| x + 1`, `Rc<dyn Fn(u32)>` | `func(x) { x + 1 }`, `func(uint32)` | Go |

---

## Hello, packages and imports

```rust
// Rust
use std::fs;

fn main() {
    let text = fs::read_to_string("notes.txt").unwrap();
    println!("{} bytes", text.len());
}
```

```rust
// Vertex
package main

import "std/fs"

func main() {
    let text = fs.readToString("notes.txt").unwrap()
    println("{} bytes", text.len())
}
```

A folder is a package, and every `.vs` file in it belongs to it. `func main()` in package
`main` is the entry point.

Imports use string paths in three forms:

1. **Package shorthand (`import "std/fs"`):** binds the last segment of the path (`fs`) as a package namespace. Best for free-function packages (`fs.readToString`, `task.join`).
2. **Named imports (`import { HashMap } from "std/collections"`):** brings specific types, traits, functions or `.vsx` components directly into the file's scope, optionally renamed with `as`.
3. **Aliased package or combined (`import geo from "./geometry"`, `import http, { Error as HttpError } from "net/http"`):** binds the package to a name you give, alone or alongside named imports.

```rust
// Rust
mod geometry;
use std::collections::HashMap;
use std::io::{Error as IoError, Read};
use std::sync::{Arc, Mutex};
use crate::geometry::Point;
```

```rust
// Vertex
package main

import { HashMap } from "std/collections"
import { Read, Error as IoError } from "std/io"
import { Arc, Mutex } from "std/sync"
import http, { Error as HttpError } from "net/http"
import geo, { Point } from "./geometry"

func main() {
    let scores: HashMap<string, int64> = HashMap.new()
    let shared = Arc.new(Mutex.new(0))
    let origin = Point { x: 0.0, y: 0.0 }
}
```

Rust's prelude needs no import: `Option`, `Some`, `None`, `Result`, `Ok`, `Err`, `Vec`,
`Box`, and traits such as `Clone`, `Copy`, `Default`, `Drop`, `From`, `Into`, `Iterator`
and `Fn`. Other traits (such as `Read` from `std/io`) are imported with `{ Read }` to call
their methods on a value.

---

## Variables and mutability

```rust
// Rust
let x = 5;
let mut count = 0;
count += 1;

let x = x * 2;                          // shadowing

let (mut a, b) = (1, 2);
a += b;

if let Some(mut n) = parse("41") {
    n += 1;
}
```

```rust
// Vertex
let x = 5
var count = 0
count += 1

let x = x * 2                           // shadowing

let (var a, b) = (1, 2)
a += b

if let Some(var n) = parse("41") {
    n += 1
}
```

`var` appears wherever Rust writes `mut` on a binding: `var x`, `Some(var n)`,
`func(var acc, x)`, `for var line in lines`. `var (a, b) = t` makes every binding in the
pattern mutable. `&mut`, `*mut` and `ref mut` keep their spelling, because they describe
a reference, not a binding.

---

## Types

| Rust | Vertex |
| --- | --- |
| `i8` `i16` `i32` `i64` | `int8` `int16` `int32` `int64` |
| `u8` `u16` `u32` `u64` | `uint8` `uint16` `uint32` `uint64` |
| `isize`, `usize` | `int`, `uint` |
| `f32`, `f64` | `float32`, `float64` |
| `bool`, `char` | `bool`, `char` |
| `String` | `string` |
| `&str` | `&string` |
| `()` | `()` |
| `!` | `never` |

```rust
// Rust
let age: u8 = 42;
let big: i64 = -9_000_000_000;
let ratio: f64 = 0.75;
let count: usize = items.len();
let name: String = "Ada".to_string();
let greeting: &str = "hello";
let pair: (i32, &str) = (1, "one");
let grid: [[u8; 3]; 2] = [[0; 3]; 2];
let part: &[u8] = &bytes[1..3];
fn fail() -> ! { panic!("stop") }
```

```rust
// Vertex
let age: uint8 = 42
let big: int64 = -9_000_000_000
let ratio: float64 = 0.75
let count: uint = items.len()
let name: string = "Ada".toString()
let greeting: &string = "hello"
let pair: (int32, &string) = (1, "one")
let grid: [[uint8; 3]; 2] = [[0; 3]; 2]
let part: &[uint8] = &bytes[1..3]
func fail() -> never { panic("stop") }
```

Casts use `as`, as in Rust: `let n = count as int64`.

---

## Functions

```rust
// Rust
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn greet(name: &str) {
    println!("hello, {name}");
}

fn largest<T: PartialOrd + Copy>(items: &[T]) -> T {
    let mut best = items[0];
    for &item in items {
        if item > best {
            best = item;
        }
    }
    best
}

fn describe<T>(value: T) -> String
where
    T: std::fmt::Debug,
{
    format!("{value:?}")
}
```

```rust
// Vertex (with import "std/fmt")
func add(a: int32, b: int32) -> int32 {
    a + b
}

func greet(name: &string) {
    println("hello, {name}")
}

func largest<T: PartialOrd + Copy>(items: &[T]) -> T {
    var best = items[0]
    for &item in items {
        if item > best {
            best = item
        }
    }
    best
}

func describe<T>(value: T) -> string
where
    T: fmt.Debug,
{
    format("{value:?}")
}
```

As in Rust, a function's last expression is its return value. Parameters are positional:
`add(1, 2)`.

Explicit generic arguments in an expression are written without Rust's `::`, on functions,
methods, types and variants alike. An annotation works too, as in Rust.

```rust
// Rust
let n = "5".parse::<i64>().unwrap();
let words = line.split(' ').collect::<Vec<_>>();
let size = std::mem::size_of::<u64>();
let empty = Vec::<i32>::new();
let none = None::<i32>;
```

```rust
// Vertex
let n = "5".parse<int64>().unwrap()
let words = line.split(' ').collect<Vec<_>>()
let size = mem.sizeOf<uint64>()
let empty = Vec<int32>.new()
let none = Option<int32>.None
```

`<` after a name starts generic arguments only where it can't be a comparison: what's
inside reads as types (`PascalCase` names, built-in types like `int32`, `&T`, `[T]`), and
the `>` is followed by `(`, `.`, `{` or the end of the expression. `a < b` and
`(a < b, c > (d))` stay comparisons, because values are `camelCase`. A constant argument
that is a bare name goes in braces: `count<{ n }>()`.

---

## Line breaks, not semicolons

A line break ends a statement, the way `;` does in Rust. A line that starts with `.` or a
binary operator continues the line before it, so method chains break before each call.

```rust
// Rust
let total = orders
    .iter()
    .filter(|o| o.paid)
    .map(|o| o.amount)
    .sum::<u64>();

let ok = has_name
    && has_email
    && age >= 18;

let a = 1; let b = 2;
```

```rust
// Vertex
let total: uint64 = orders
    .iter()
    .filter(func(o) { o.paid })
    .map(func(o) { o.amount })
    .sum()

let ok = hasName
    && hasEmail
    && age >= 18

let a = 1; let b = 2
```

`;` is only for putting two statements on one line. The same rule separates struct fields,
enum variants and `match` arms, which Rust separates with commas:

```rust
// Rust
struct Point { x: f64, y: f64 }

enum Shape {
    Circle(f64),
    Rect { w: f64, h: f64 },
}

let area = match shape {
    Shape::Circle(r) => 3.14 * r * r,
    Shape::Rect { w, h } => w * h,
};
```

```rust
// Vertex
struct Point { x: float64; y: float64 }

enum Shape {
    Circle(float64)
    Rect { w: float64; h: float64 }
}

let area = match shape {
    Shape.Circle(r) => 3.14 * r * r
    Shape.Rect { w, h } => w * h
}
```

Lists of values keep their commas: arguments, parameters, tuples, struct literals,
`vec[1, 2, 3]`, and the fields inside a pattern like `Shape.Rect { w, h }`.

**Block values.** A block's value is its last expression. Rust uses a trailing `;` to turn
that value into `()`. In Vertex, a block whose type is `()` simply discards the value:

```rust
// Rust
fn remember(map: &mut HashMap<u64, Task>, t: Task) {
    map.insert(t.id, t);                  // the `;` discards the Option<Task>
}
```

```rust
// Vertex
func remember(m: &mut HashMap<uint64, Task>, t: Task) {
    m.insert(t.id, t)                     // the function returns (), so the Option<Task> is discarded
}
```

---

## Control flow

```rust
// Rust
let label = if n < 0 { "negative" } else if n == 0 { "zero" } else { "positive" };

for i in 0..10 {
    println!("{i}");
}

for i in (1..=3).rev() {
    println!("{i}");
}

let mut tries = 0;
while tries < 3 {
    tries += 1;
}

let found = loop {
    if let Some(x) = next() {
        break x;
    }
};

'outer: for row in &grid {
    for &cell in row {
        if cell == 0 {
            break 'outer;
        }
    }
}
```

```rust
// Vertex
let label = if n < 0 { "negative" } else if n == 0 { "zero" } else { "positive" }

for i in 0..10 {
    println("{i}")
}

for i in (1..=3).rev() {
    println("{i}")
}

var tries = 0
while tries < 3 {
    tries += 1
}

let found = loop {
    if let Some(x) = next() {
        break x
    }
}

outer: for row in &grid {
    for &cell in row {
        if cell == 0 {
            break outer
        }
    }
}
```

`if`, `match`, `loop` and blocks are expressions, as in Rust. Rust has no `?:`; use `if`.

Without the `'`, `break name` reads as Rust's `break 'name` when an enclosing loop or block
is labeled `name`, and as `break name` (a value) otherwise. `break outer outer + 1` is the
label `outer` with a value. To break with a value that has the same name as an enclosing
label, parenthesize it: `break (outer)`. As in Rust, labels don't reach into function
literals.

---

## Structs

```rust
// Rust
#[derive(Debug, Clone, PartialEq)]
struct Task {
    id: u64,
    title: String,
    done: bool,
}

struct Meters(f64);                       // tuple struct
struct Marker;                            // unit struct

let id = 7;
let title = "write docs".to_string();
let t = Task { id, title, done: false };
let copy = Task { done: true, ..t.clone() };
let Task { id, title, .. } = t;
let m = Meters(3.5);
println!("{}", m.0);
```

```rust
// Vertex
@derive(Debug, Clone, PartialEq)
struct Task {
    id: uint64
    title: string
    done: bool
}

struct Meters(float64)                    // tuple struct
struct Marker                             // unit struct

let id = 7
let title = "write docs".toString()
let t = Task { id, title, done: false }
let copy = Task { done: true, ..t.clone() }
let Task { id, title, .. } = t
let m = Meters(3.5)
println("{}", m.0)
```

Fields are one per line, or separated by `;` on one line. Struct literals keep commas,
the `id` shorthand and `..base`, as in Rust.

`Marker.hello()` names the method `hello` on the type `Marker` (exception 7), the way
`Task.new()` does, so a method taking `self` is called on a value. For a unit struct, put
the value in parentheses: `(Marker).hello()`, or bind it first: `let m = Marker`.

---

## Enums and `match`

```rust
// Rust
#[derive(Debug)]
enum Event {
    Quit,
    Key(char),
    Click { x: i32, y: i32 },
}

fn describe(e: &Event) -> String {
    match e {
        Event::Quit => "quit".to_string(),
        Event::Key(c) if c.is_ascii_digit() => format!("digit {c}"),
        Event::Key(c) => format!("key {c}"),
        Event::Click { x, y } => format!("click at {x},{y}"),
    }
}

fn bucket(n: u32) -> &'static str {
    match n {
        0 => "none",
        small @ 1..=9 if small % 2 == 0 => "small even",
        1..=9 => "small odd",
        _ => "many",
    }
}

fn ends(v: &[i32]) -> Option<(i32, i32)> {
    match v {
        [first, .., last] => Some((*first, *last)),
        [only] => Some((*only, *only)),
        [] => None,
    }
}
```

```rust
// Vertex
@derive(Debug)
enum Event {
    Quit
    Key(char)
    Click { x: int32; y: int32 }
}

func describe(e: &Event) -> string {
    match e {
        Event.Quit => "quit".toString()
        Event.Key(c) if c.isAsciiDigit() => format("digit {c}")
        Event.Key(c) => format("key {c}")
        Event.Click { x, y } => format("click at {x},{y}")
    }
}

func bucket(n: uint32) -> &static string {
    match n {
        0 => "none"
        small @ 1..=9 if small % 2 == 0 => "small even"
        1..=9 => "small odd"
        _ => "many"
    }
}

func ends(v: &[int32]) -> Option<(int32, int32)> {
    match v {
        [first, .., last] => Some((*first, *last))
        [only] => Some((*only, *only))
        [] => None
    }
}
```

Discriminants and `if let` / `let … else` / `while let` work as in Rust:

```rust
// Rust
#[repr(i32)]
enum Whence { Set = 0, Cur = 1, End = 2 }

if let Event::Key(c) = event {
    println!("{c}");
}

let Some(user) = find(id) else {
    return Err(Error::NotFound);
};

while let Some(top) = stack.pop() {
    println!("{top}");
}
```

```rust
// Vertex
@repr(int32)
enum Whence { Set = 0; Cur = 1; End = 2 }

if let Event.Key(c) = event {
    println("{c}")
}

let Some(user) = find(id) else {
    return Err(Error.NotFound)
}

while let Some(top) = stack.pop() {
    println("{top}")
}
```

---

## Methods: `implement` and flat receivers

Methods go in an `implement` block, which is Rust's `impl` spelled out. A type's own
methods can also be written one at a time with a Go-style receiver. Both forms mean the
same thing.

```rust
// Rust
impl Task {
    pub fn new(id: u64, title: String) -> Task {
        Task { id, title, done: false }
    }

    pub fn is_open(&self) -> bool {
        !self.done
    }

    pub fn finish(&mut self) {
        self.done = true;
    }

    pub fn into_title(self) -> String {
        self.title
    }
}

let mut t = Task::new(1, "ship".to_string());
t.finish();
```

```rust
// Vertex: a block
implement Task {
    public func new(id: uint64, title: string) -> Task {
        Task { id, title, done: false }
    }

    public func isOpen(&self) -> bool {
        !self.done
    }

    public func finish(&mut self) {
        self.done = true
    }

    public func intoTitle(self) -> string {
        self.title
    }
}

var t = Task.new(1, "ship".toString())
t.finish()
```

```rust
// Vertex: flat receivers, the same methods
public func (t: &Task) isOpen() -> bool { !t.done }
public func (t: &mut Task) finish() { t.done = true }
public func (t: Task) intoTitle() -> string { t.title }
```

| Rust receiver | Vertex block | Vertex flat |
| --- | --- | --- |
| `&self` | `&self` | `(t: &Task)` |
| `&mut self` | `&mut self` | `(t: &mut Task)` |
| `self` | `self` | `(t: Task)` |
| `mut self` | `var self` | `(var t: Task)` |

The usual style: flat functions for a type's own methods, and a block when you have an
associated function such as `new` or a large type. A type can mix both, but each method
name is defined once.

Generic types work the same way. In a flat receiver, naming the type parameter introduces
it, as in Go. Bounds need a block:

```rust
// Rust
struct Stack<T> {
    items: Vec<T>,
}

impl<T> Stack<T> {
    fn push(&mut self, item: T) { self.items.push(item); }
    fn pop(&mut self) -> Option<T> { self.items.pop() }
}

impl<T: std::fmt::Display> Stack<T> {
    fn show(&self) {
        for item in &self.items {
            println!("{item}");
        }
    }
}
```

```rust
// Vertex (with import "std/fmt")
struct Stack<T> {
    items: Vec<T>
}

func (s: &mut Stack<T>) push(item: T) { s.items.push(item) }
func (s: &mut Stack<T>) pop() -> Option<T> { s.items.pop() }

implement<T: fmt.Display> Stack<T> {
    func show(&self) {
        for item in &self.items {
            println("{item}")
        }
    }
}
```

---

## Traits

```rust
// Rust
trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> String {
        "shape".to_string()
    }
}

struct Circle { r: f64 }

impl Shape for Circle {
    fn area(&self) -> f64 { 3.14 * self.r * self.r }
    fn name(&self) -> String { "circle".to_string() }
}

fn total_area(shapes: &[Box<dyn Shape>]) -> f64 {
    shapes.iter().map(|s| s.area()).sum()
}

fn print_area(s: &impl Shape) {
    println!("{} has area {}", s.name(), s.area());
}

fn biggest<S: Shape>(a: S, b: S) -> S {
    if a.area() >= b.area() { a } else { b }
}
```

```rust
// Vertex
trait Shape {
    func area(&self) -> float64
    func name(&self) -> string {
        "shape".toString()
    }
}

struct Circle { r: float64 }

implement Shape for Circle {
    func area(&self) -> float64 { 3.14 * self.r * self.r }
    func name(&self) -> string { "circle".toString() }
}

func totalArea(shapes: &[Box<dyn Shape>]) -> float64 {
    shapes.iter().map(func(s) { s.area() }).sum()
}

func printArea(s: &impl Shape) {
    println("{} has area {}", s.name(), s.area())
}

func biggest<S: Shape>(a: S, b: S) -> S {
    if a.area() >= b.area() { a } else { b }
}
```

A trait is always implemented in an explicit `implement Trait for Type` block. The block is
checked against the trait, so a missing or misspelled method is an error where it's written.

Standard traits work the same way. Their method names use `camelCase`:

```rust
// Rust
use std::fmt;
use std::ops::Add;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Point { x: f64, y: f64 }

impl Add for Point {
    type Output = Point;
    fn add(self, o: Point) -> Point {
        Point { x: self.x + o.x, y: self.y + o.y }
    }
}

impl fmt::Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

impl Default for Point {
    fn default() -> Point { Point { x: 0.0, y: 0.0 } }
}

struct Countdown(u32);

impl Iterator for Countdown {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.0 == 0 { None } else { self.0 -= 1; Some(self.0 + 1) }
    }
}
```

```rust
// Vertex
import "std/fmt"
import { Add } from "std/ops"

@derive(Clone, Copy, Debug, PartialEq)
struct Point { x: float64; y: float64 }

implement Add for Point {
    type Output = Point
    func add(self, o: Point) -> Point {
        Point { x: self.x + o.x, y: self.y + o.y }
    }
}

implement fmt.Display for Point {
    func fmt(&self, f: &mut fmt.Formatter) -> fmt.Result {
        f.writeStr(&format("({}, {})", self.x, self.y))
    }
}

implement Default for Point {
    func default() -> Point { Point { x: 0.0, y: 0.0 } }
}

struct Countdown(uint32)

implement Iterator for Countdown {
    type Item = uint32
    func next(&mut self) -> Option<uint32> {
        if self.0 == 0 { None } else { self.0 -= 1; Some(self.0 + 1) }
    }
}
```

`write!` isn't a built-in, so `fmt` writes a formatted string with `f.writeStr(&format(…))`.

---

## Ownership and borrowing

The rules are Rust's: values move by default, `Copy` types are copied, `.clone()` makes an
explicit copy, and a value can have one `&mut` borrow or any number of `&` borrows.

```rust
// Rust
let a = String::from("hi");
let b = a;                                // moved
// println!("{a}");                       // error[E0382]: borrow of moved value: `a`
let c = b.clone();

fn len(s: &str) -> usize { s.len() }
fn shout(s: &mut String) { s.push('!'); }
fn consume(s: String) -> usize { s.len() }

let mut msg = "hello".to_string();
shout(&mut msg);
let n = len(&msg);
let m = consume(msg);                     // msg is moved

let nums = vec![1, 2, 3, 4];
let middle: &[i32] = &nums[1..3];
for n in &nums { println!("{n}"); }       // borrow each
for n in nums { println!("{n}"); }        // consume the Vec
```

```rust
// Vertex
let a = "hi".toString()
let b = a                                 // moved
// println("{a}")                         // error[E0382]: borrow of moved value: `a`
let c = b.clone()

func len(s: &string) -> uint { s.len() }
func shout(s: &mut string) { s.push('!') }
func consume(s: string) -> uint { s.len() }

var msg = "hello".toString()
shout(&mut msg)
let n = len(&msg)
let m = consume(msg)                      // msg is moved

let nums = vec[1, 2, 3, 4]
let middle: &[int32] = &nums[1..3]
for n in &nums { println("{n}") }         // borrow each
for n in nums { println("{n}") }          // consume the Vec
```

Errors carry Rust's error codes (E0382 use after move, E0499 two `&mut` borrows, E0505
move while borrowed), and Rust's explanations for them apply.

---

## Lifetimes

Lifetimes and loop labels work exactly as in Rust, without the single quote `'`: `'a` is
`a`, `&'a T` is `&a T`, `&'static str` is `&static string`, and `'outer:` is `outer:`. As
in Rust, most functions need no lifetime annotation at all.

```rust
// Rust
fn first_word(s: &str) -> &str {                       // elided
    s.split(' ').next().unwrap_or("")
}

fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
    if a.len() >= b.len() { a } else { b }
}

fn pick_first<'a, 'b>(a: &'a str, _b: &'b str) -> &'a str {
    a
}

const GREETING: &'static str = "hello";
```

```rust
// Vertex
func firstWord(s: &string) -> &string {                // elided
    s.split(' ').next().unwrapOr("")
}

func longest<a>(a: &a string, b: &a string) -> &a string {
    if a.len() >= b.len() { a } else { b }
}

func pickFirst<a, b>(a: &a string, _b: &b string) -> &a string {
    a
}

const greeting: &static string = "hello"
```

A type that holds a borrow declares a lifetime parameter the same way:

```rust
// Rust
struct Highlight<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

impl<'a> Highlight<'a> {
    fn part(&self) -> &'a str {
        &self.text[self.start..self.end]
    }
}

let doc = String::from("hello world");
let h = Highlight { text: &doc, start: 0, end: 5 };
println!("{}", h.part());                // hello
```

```rust
// Vertex
struct Highlight<a> {
    text: &a string
    start: uint
    end: uint
}

implement<a> Highlight<a> {
    func part(&self) -> &a string {
        &self.text[self.start..self.end]
    }
}

let doc = string.from("hello world")
let h = Highlight { text: &doc, start: 0, end: 5 }
println("{}", h.part())                  // hello
```

| Rust | Vertex |
| --- | --- |
| `&'a T`, `&'a mut T` | `&a T`, `&a mut T` |
| `fn f<'a>(x: &'a T, y: &'a T) -> &'a T` | `func f<a>(x: &a T, y: &a T) -> &a T` |
| `struct S<'a> { r: &'a T }` | `struct S<a> { r: &a T }` |
| `impl<'a> S<'a>` | `implement<a> S<a>` |
| `'a: 'b` | `a: b` |
| `T: 'a` | `T: a` |
| `&'static str` | `&static string` |
| `for<'a> Fn(&'a T)` | `for<a> Fn(&a T)` |
| `&'_ T` | `&_ T` (or `&T`) |
| `'outer: loop { break 'outer }` | `outer: loop { break outer }` |

Lifetimes are `camelCase` (`a`, `src`), while type parameters are `PascalCase` (`T`, `Item`).

---

## Errors

Errors are `Result<T, E>` values, and `?` passes them up, as in Rust. `?` converts error
types through `From`.

```rust
// Rust
use std::{fs, io, num::ParseIntError};

#[derive(Debug)]
enum ConfigError {
    Io(io::Error),
    Parse(ParseIntError),
    Missing(String),
}

impl From<io::Error> for ConfigError {
    fn from(e: io::Error) -> Self { ConfigError::Io(e) }
}

impl From<ParseIntError> for ConfigError {
    fn from(e: ParseIntError) -> Self { ConfigError::Parse(e) }
}

fn read_port(path: &str) -> Result<u16, ConfigError> {
    let text = fs::read_to_string(path)?;
    let line = text
        .lines()
        .find(|l| l.starts_with("port="))
        .ok_or(ConfigError::Missing("port".to_string()))?;
    let port: u16 = line["port=".len()..].trim().parse()?;
    Ok(port)
}

fn main() -> Result<(), ConfigError> {
    let port = read_port("app.conf")?;
    println!("listening on {port}");

    let fallback = read_port("other.conf").unwrap_or(8080);
    let maybe = read_port("missing.conf").ok();
    match read_port("bad.conf") {
        Ok(p) => println!("{p}"),
        Err(ConfigError::Missing(key)) => println!("missing {key}"),
        Err(e) => return Err(e),
    }
    Ok(())
}
```

```rust
// Vertex
package main

import "std/fs"
import "std/io"
import { ParseIntError } from "std/num"

@derive(Debug)
enum ConfigError {
    Io(io.Error)
    Parse(ParseIntError)
    Missing(string)
}

implement From<io.Error> for ConfigError {
    func from(e: io.Error) -> Self { ConfigError.Io(e) }
}

implement From<ParseIntError> for ConfigError {
    func from(e: ParseIntError) -> Self { ConfigError.Parse(e) }
}

func readPort(path: &string) -> Result<uint16, ConfigError> {
    let text = fs.readToString(path)?
    let line = text
        .lines()
        .find(func(l) { l.startsWith("port=") })
        .okOr(ConfigError.Missing("port".toString()))?
    let port: uint16 = line["port=".len()..].trim().parse()?
    Ok(port)
}

func main() -> Result<(), ConfigError> {
    let port = readPort("app.conf")?
    println("listening on {port}")

    let fallback = readPort("other.conf").unwrapOr(8080)
    let maybe = readPort("missing.conf").ok()
    match readPort("bad.conf") {
        Ok(p) => println("{p}")
        Err(ConfigError.Missing(key)) => println("missing {key}")
        Err(e) => return Err(e)
    }
    Ok(())
}
```

`?` works on `Option` too, in a function that returns `Option`:

```rust
// Rust
fn middle_initial(name: &str) -> Option<char> {
    let middle = name.split(' ').nth(1)?;
    middle.chars().next()
}
```

```rust
// Vertex
func middleInitial(name: &string) -> Option<char> {
    let middle = name.split(' ').nth(1)?
    middle.chars().next()
}
```

A panic (`panic("…")`, `.unwrap()` on `None`, an out-of-bounds index) exits with code 101,
as in Rust.

---

## Function literals and `func` values

A closure is written like a function without a name: `func(params) { body }`. The meaning
is Rust's closure. Parameter and return types are inferred where they can be.

```rust
// Rust
let double = |x: i64| x * 2;
let add = |a, b| a + b;
let greet = || println!("hi");

let evens: Vec<i64> = nums.iter().copied().filter(|n| n % 2 == 0).collect();
names.sort_by(|a, b| a.len().cmp(&b.len()));
let shout = |s: &str| -> String { s.to_uppercase() };
```

```rust
// Vertex
let double = func(x: int64) { x * 2 }
let add = func(a, b) { a + b }
let greet = func() { println("hi") }

let evens: Vec<int64> = nums.iter().copied().filter(func(n) { n % 2 == 0 }).collect()
names.sortBy(func(a, b) { a.len().cmp(&b.len()) })
let shout = func(s: &string) -> string { s.toUppercase() }
```

**Captures.** There's no `move` keyword. A function literal that escapes the scope it was
created in takes ownership of what it captures: one that's stored, returned or spawned. A
literal that doesn't escape borrows, as Rust closures do.

```rust
// Rust
fn make_adder(n: i32) -> impl Fn(i32) -> i32 {
    move |x| x + n
}

let mut count = 0;
let mut inc = || count += 1;              // borrows `count` mutably
inc();
inc();
println!("{count}");                       // 2

let name = "Ada".to_string();
std::thread::spawn(move || println!("{name}"));
```

```rust
// Vertex
func makeAdder(n: int32) -> impl Fn(int32) -> int32 {
    func(x) { x + n }                     // returned, so it owns `n`
}

var count = 0
var inc = func() { count += 1 }           // borrows `count` mutably
inc()
inc()
println("{count}")                         // 2

let name = "Ada".toString()
thread.spawn(func() { println("{name}") }) // spawned, so it owns `name`
```

**Function types.** Every Rust closure has its own unnameable type, so storing one means
`Box<dyn Fn>` or `Rc<dyn Fn>`. Vertex's function type `func(T) -> U` is one nameable type
for any function or literal with that signature. It means Rust's `Rc<dyn Fn(T) -> U>`:
cheap to copy, and called directly.

```rust
// Rust
use std::rc::Rc;

struct Button {
    label: String,
    on_click: Rc<dyn Fn(u32)>,
}

fn log_click(id: u32) { println!("clicked {id}"); }

let b = Button { label: "Go".to_string(), on_click: Rc::new(log_click) };
let c = Button { label: "Stop".to_string(), on_click: Rc::new(|id| println!("stop {id}")) };
(b.on_click)(1);
```

```rust
// Vertex
struct Button {
    label: string
    onClick: func(uint32)
}

func logClick(id: uint32) { println("clicked {id}") }

let b = Button { label: "Go".toString(), onClick: logClick }
let c = Button { label: "Stop".toString(), onClick: func(id) { println("stop {id}") } }
(b.onClick)(1)
```

A `func(T)` value allocates once and calls indirectly. For zero cost, take a generic
parameter with Rust's `Fn` traits, which is specialized and inlined:

```rust
// Rust
fn apply_twice<F: Fn(i64) -> i64>(f: F, x: i64) -> i64 { f(f(x)) }
fn count_where<T, F>(items: &[T], pred: F) -> usize where F: Fn(&T) -> bool {
    items.iter().filter(|x| pred(x)).count()
}
```

```rust
// Vertex
func applyTwice<F: Fn(int64) -> int64>(f: F, x: int64) -> int64 { f(f(x)) }
func countWhere<T, F>(items: &[T], pred: F) -> uint where F: Fn(&T) -> bool {
    items.iter().filter(func(x) { pred(x) }).count()
}
```

Async literals mirror `async func`:

```rust
// Rust
server.route("/sync", async |req| { handle(req).await });
```

```rust
// Vertex
server.route("/sync", async func(req) { handle(req).await })
```

---

## Classes

A `class` is a shared, reference-counted object: what Rust writes as `Rc<RefCell<T>>`.
Copying a class value shares the object, and methods on `&Store` can change its fields.
Each access is checked at runtime, as with `RefCell`.

```rust
// Rust
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

struct StoreInner {
    tasks: HashMap<u64, String>,
    next_id: u64,
}

#[derive(Clone)]
struct Store(Rc<RefCell<StoreInner>>);

impl Store {
    fn new() -> Store {
        Store(Rc::new(RefCell::new(StoreInner { tasks: HashMap::new(), next_id: 1 })))
    }

    fn add(&self, title: &str) -> u64 {
        let mut s = self.0.borrow_mut();
        let id = s.next_id;
        s.next_id += 1;
        s.tasks.insert(id, title.to_string());
        id
    }

    fn count(&self) -> usize {
        self.0.borrow().tasks.len()
    }
}

let store = Store::new();
let shared = store.clone();               // same object
shared.add("write docs");
println!("{}", store.count());            // 1
```

```rust
// Vertex
import { HashMap } from "std/collections"

class Store {
    tasks: HashMap<uint64, string>
    nextId: uint64
}

implement Store {
    func new() -> Store {
        Store { tasks: HashMap.new(), nextId: 1 }
    }
}

func (s: &Store) add(title: &string) -> uint64 {
    let id = s.nextId
    s.nextId += 1
    s.tasks.insert(id, title.toString())
    id
}

func (s: &Store) count() -> uint { s.tasks.len() }

let store = Store.new()
let shared = store                        // same object
shared.add("write docs")
println("{}", store.count())              // 1
```

Weak references and cleanup work as in Rust:

```rust
// Rust
use std::rc::Weak;

struct Node {
    name: String,
    parent: RefCell<Weak<Node>>,
}

impl Drop for Node {
    fn drop(&mut self) {
        println!("dropping {}", self.name);
    }
}
```

```rust
// Vertex
import { Weak } from "std/rc"

class Node {
    name: string
    parent: Weak<Node>
}

implement Drop for Node {
    func drop(&mut self) {
        println("dropping {}", self.name)
    }
}
```

A class's reference count isn't atomic, so a class isn't `Send`. For state shared between
threads, use `Arc<Mutex<T>>` (see [Threads and async](#threads-and-async)).
Classes have no inheritance; shared behavior comes from traits.

---

## Collections and iterators

```rust
// Rust
use std::collections::HashMap;

let mut v: Vec<i32> = Vec::new();
v.push(3);
v.push(1);
v.extend([4, 1, 5]);
v.sort();
v.dedup();
let first = v.first().copied();
let has_four = v.contains(&4);

let squares: Vec<i32> = v.iter().map(|x| x * x).collect();
let total: i32 = v.iter().sum();
let (small, large): (Vec<i32>, Vec<i32>) = v.iter().copied().partition(|&x| x < 3);

let mut counts: HashMap<String, u32> = HashMap::new();
for word in "a b a c b a".split(' ') {
    *counts.entry(word.to_string()).or_insert(0) += 1;
}
let mut keys: Vec<_> = counts.keys().collect();
keys.sort();
for k in keys {
    println!("{k}: {}", counts[k]);
}

for (i, x) in v.iter().enumerate() {
    println!("{i} {x}");
}

let pairs: Vec<(i32, char)> = v.iter().copied().zip("abc".chars()).collect();
```

```rust
// Vertex
import { HashMap } from "std/collections"

var v: Vec<int32> = Vec.new()
v.push(3)
v.push(1)
v.extend([4, 1, 5])
v.sort()
v.dedup()
let first = v.first().copied()
let hasFour = v.contains(&4)

let squares: Vec<int32> = v.iter().map(func(x) { x * x }).collect()
let total: int32 = v.iter().sum()
let (small, large): (Vec<int32>, Vec<int32>) = v.iter().copied().partition(func(&x) { x < 3 })

var counts: HashMap<string, uint32> = HashMap.new()
for word in "a b a c b a".split(' ') {
    *counts.entry(word.toString()).orInsert(0) += 1
}
var keys: Vec<&string> = counts.keys().collect()
keys.sort()
for k in keys {
    println("{k}: {}", counts[k])
}

for (i, x) in v.iter().enumerate() {
    println("{i} {x}")
}

let pairs: Vec<(int32, char)> = v.iter().copied().zip("abc".chars()).collect()
```

`vec[…]` builds a `Vec` the way Rust's `vec![…]` does: `vec[]`, `vec[1, 2, 3]`,
`vec[0; n]`.

---

## Strings and formatting

```rust
// Rust
let name = "Ada";
let age = 36;
let s = format!("{name} is {age}");
let padded = format!("[{:>8}]", name);
let money = format!("{:.2}", 3.14159);
let debug = format!("{:?}", vec![1, 2]);
let pretty = format!("{:#?}", point);

let mut owned = String::from("hello");
owned.push_str(", world");
let upper = owned.to_uppercase();
let words: Vec<&str> = owned.split(", ").collect();
let trimmed = "  hi  ".trim();
let has = owned.contains("world");
let first_char = owned.chars().next();
let bytes = owned.as_bytes();
let joined = ["a", "b", "c"].join("-");

println!("{s}");
eprintln!("warning: {}", upper);
```

```rust
// Vertex
let name = "Ada"
let age = 36
let s = format("{name} is {age}")
let padded = format("[{:>8}]", name)
let money = format("{:.2}", 3.14159)
let debug = format("{:?}", vec[1, 2])
let pretty = format("{:#?}", point)

var owned = string.from("hello")
owned.pushStr(", world")
let upper = owned.toUppercase()
let words: Vec<&string> = owned.split(", ").collect()
let trimmed = "  hi  ".trim()
let has = owned.contains("world")
let firstChar = owned.chars().next()
let bytes = owned.asBytes()
let joined = ["a", "b", "c"].join("-")

println("{s}")
eprintln("warning: {}", upper)
```

Format strings are Rust's: checked when compiling, and able to capture local variables
(`{name}`). `{:?}` prints exactly as in Rust (`Ok(42)`, `Some("x")`, `[1, 2]`).

---

## Attributes

```rust
// Rust
#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Id(u64);

#[inline]
fn fast(x: u32) -> u32 { x << 1 }

#[cfg(target_os = "macos")]
fn platform() -> &'static str { "mac" }

#[cfg(not(target_os = "macos"))]
fn platform() -> &'static str { "other" }

#[deprecated(note = "use fast")]
fn slow(x: u32) -> u32 { x * 2 }

#[must_use]
fn compute() -> u32 { 42 }

#[test]
fn doubles() {
    assert_eq!(fast(2), 4);
}
```

```rust
// Vertex
@allow(deadCode)
package main

@derive(Debug, Clone, Copy, PartialEq, Eq, Hash)
struct Id(uint64)

@inline
func fast(x: uint32) -> uint32 { x << 1 }

@cfg(target_os = "macos")
func platform() -> &static string { "mac" }

@cfg(not(target_os = "macos"))
func platform() -> &static string { "other" }

@deprecated(note = "use fast")
func slow(x: uint32) -> uint32 { x * 2 }

@mustUse
func compute() -> uint32 { 42 }

@test
func doubles() {
    assertEq(fast(2), 4)
}
```

Attributes are written with `@` and keep Rust's arguments. Rust's file-wide attributes
(`#![…]`) go on the `package` declaration. Attribute names use `camelCase` (`deadCode`,
`mustUse`, `exportName`), while configuration keys such as `target_os` keep Rust's
spelling.

---

## Visibility and naming

| Rust | Vertex | Visible to |
| --- | --- | --- |
| `pub` | `public` | everyone |
| `pub(crate)` | `package` | every package in the same crate |
| (nothing) | (nothing) | this package |

```rust
// Rust
pub struct User {
    id: u64,                              // private
    pub name: String,
}

impl User {
    pub fn id(&self) -> u64 { self.id }
    pub(crate) fn reset_name(&mut self) { self.name.clear(); }
    fn checksum(&self) -> u64 { self.id * 31 }
}

pub const MAX_USERS: usize = 100;
```

```rust
// Vertex
public struct User {
    id: uint64                            // this package only
    public name: string
}

implement User {
    public func id(&self) -> uint64 { self.id }
    package func resetName(&mut self) { self.name.clear() }
    func checksum(&self) -> uint64 { self.id * 31 }
}

public const maxUsers: uint = 100
```

Types, traits and enum variants are `PascalCase` (`User`, `Display`, `Status.Todo`).
Everything else — functions, methods, fields, variables, constants, built-ins and
attributes — is `camelCase` (`resetName`, `maxUsers`, `nextId`, `readToString`, `unwrapOr`),
regardless of visibility. The keyword (`public`, `package`) decides visibility; a lint
checks that the casing matches.

---

## Threads and async

```rust
// Rust
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

let counter = Arc::new(Mutex::new(0));
let mut handles = vec![];
for _ in 0..4 {
    let counter = Arc::clone(&counter);
    handles.push(thread::spawn(move || {
        *counter.lock().unwrap() += 1;
    }));
}
for h in handles {
    h.join().unwrap();
}
println!("{}", *counter.lock().unwrap());     // 4

let (tx, rx) = mpsc::channel();
thread::spawn(move || {
    for i in 0..3 {
        tx.send(i).unwrap();
    }
});
for value in rx {
    println!("got {value}");
}
```

```rust
// Vertex
import { Arc, Mutex } from "std/sync"
import "std/sync/mpsc"
import "std/thread"

let counter = Arc.new(Mutex.new(0))
var handles = vec[]
for _ in 0..4 {
    let counter = Arc.clone(&counter)
    handles.push(thread.spawn(func() {
        *counter.lock().unwrap() += 1
    }))
}
for h in handles {
    h.join().unwrap()
}
println("{}", *counter.lock().unwrap())       // 4

let (tx, rx) = mpsc.channel()
thread.spawn(func() {
    for i in 0..3 {
        tx.send(i).unwrap()
    }
})
for value in rx {
    println("got {value}")
}
```

`async func` and `.await` are Rust's `async fn` and `.await`. The runtime runs
`async func main`, so it needs no attribute:

```rust
// Rust
async fn fetch_len(url: &str) -> Result<usize, reqwest::Error> {
    let body = reqwest::get(url).await?.text().await?;
    Ok(body.len())
}

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let (a, b) = tokio::join!(
        fetch_len("https://example.com/a"),
        fetch_len("https://example.com/b"),
    );
    println!("{} + {}", a?, b?);
    Ok(())
}
```

```rust
// Vertex
package main

import "net/http"
import "std/task"

async func fetchLen(url: &string) -> Result<uint, http.Error> {
    let body = http.get(url).await?.text().await?
    Ok(body.len())
}

async func main() -> Result<(), http.Error> {
    let (a, b) = task.join(
        fetchLen("https://example.com/a"),
        fetchLen("https://example.com/b"),
    ).await
    println("{} + {}", a?, b?)
    Ok(())
}
```

An async function compiles to a state machine (Rust's `Future`). `Send` and `Sync` mean
what they mean in Rust.

---

## `unsafe` and C

C's types keep Rust's names (`c_int`, `c_char`, `c_void`, …) and need no import. Sizes are
`uint` and `int` (`size_t`, `ssize_t`). C strings are `cstring` (owned, Rust's `CString`)
and `&cstring` (borrowed, Rust's `&CStr`). A `c"…"` literal is a `&cstring`.

```rust
// Rust
use std::ffi::{c_char, c_int, c_void, CString};

#[link(name = "c")]
unsafe extern "C" {
    safe fn abs(n: c_int) -> c_int;
    fn strlen(s: *const c_char) -> usize;
    fn printf(fmt: *const c_char, ...) -> c_int;
    fn qsort(base: *mut c_void, n: usize, size: usize, cmp: Compare);
    fn fopen(path: *const c_char, mode: *const c_char) -> *mut FILE;
    fn fclose(f: *mut FILE) -> c_int;
    type FILE;                            // extern types are unstable in Rust (RFC 1861)
}

type Compare = unsafe extern "C" fn(*const c_void, *const c_void) -> c_int;

extern "C" fn compare_int(a: *const c_void, b: *const c_void) -> c_int {
    let x = unsafe { *(a as *const c_int) };
    let y = unsafe { *(b as *const c_int) };
    x.cmp(&y) as c_int
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Point { x: f64, y: f64 }

#[repr(C)]
union Number { i: i64, f: f64 }

#[unsafe(export_name = "vx_add")]
pub extern "C" fn add(a: c_int, b: c_int) -> c_int { a + b }

fn main() -> Result<(), std::ffi::NulError> {
    println!("{}", abs(-5));
    let n = unsafe { strlen(c"hello".as_ptr()) };
    unsafe { printf(c"%d apples\n".as_ptr(), 3) };

    let mut nums = vec![5, 2, 9, 1];
    unsafe { qsort(nums.as_mut_ptr() as *mut c_void, nums.len(), 4, compare_int) };

    let path = CString::new("notes.txt")?;
    let f = unsafe { fopen(path.as_ptr(), c"r".as_ptr()) };
    if !f.is_null() {
        unsafe { fclose(f) };
    }

    let mut num = Number { f: 1.5 };
    num.i = 42;
    println!("{}", unsafe { num.i });
    Ok(())
}
```

```rust
// Vertex
package main

import "std/ffi"

@link(name = "c")
unsafe extern "C" {
    safe func abs(n: c_int) -> c_int
    func strlen(s: *const c_char) -> uint
    func printf(fmt: *const c_char, ...) -> c_int
    func qsort(base: *mut c_void, n: uint, size: uint, cmp: Compare)
    func fopen(path: *const c_char, mode: *const c_char) -> *mut FILE
    func fclose(f: *mut FILE) -> c_int
    type FILE
}

type Compare = unsafe extern "C" func(*const c_void, *const c_void) -> c_int

extern "C" func compareInt(a: *const c_void, b: *const c_void) -> c_int {
    let x = unsafe { *(a as *const c_int) }
    let y = unsafe { *(b as *const c_int) }
    x.cmp(&y) as c_int
}

@repr(c)
@derive(Clone, Copy)
struct Point { x: float64; y: float64 }

@repr(c)
union Number { i: int64; f: float64 }

@unsafe(exportName = "vx_add")
public extern "C" func add(a: c_int, b: c_int) -> c_int { a + b }

func main() -> Result<(), ffi.NulError> {
    println("{}", abs(-5))
    let n = unsafe { strlen(c"hello".asPtr()) }
    unsafe { printf(c"%d apples\n".asPtr(), 3) }

    var nums = vec[5, 2, 9, 1]
    unsafe { qsort(nums.asMutPtr() as *mut c_void, nums.len(), 4, compareInt) }

    let path = cstring.new("notes.txt")?
    let f = unsafe { fopen(path.asPtr(), c"r".asPtr()) }
    if !f.isNull() {
        unsafe { fclose(f) }
    }

    var num = Number { f: 1.5 }
    num.i = 42
    println("{}", unsafe { num.i })
    Ok(())
}
```

Notes:

- `unsafe extern "C" { … }` is required, as in Rust 2024. `safe func` declarations can be
  called without `unsafe`.
- `func(T)` with `unsafe` or `extern` is a raw function pointer, such as a C callback.
  Without a qualifier, it's a function value (see
  [Function literals and `func` values](#function-literals-and-func-values)).
- `exportName` gives C a custom symbol name (`vx_add`).
- The idiom is Rust's: keep the raw declarations private and wrap them in a safe type
  with `implement Drop`.

---

## Using Vertex from Rust

A Vertex package can be used from Rust as a crate. Names convert to Rust's conventions:

```rust
// Vertex: geo/src/lib.vs
package geo

@derive(Clone, Copy, Debug)
public struct Point {
    public x: float64
    public y: float64
}

public func distance(a: &Point, b: &Point) -> float64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

public func parse(text: &string) -> Result<Vec<Point>, ParseError> { … }
```

```rust
// Rust, with `geo` as a dependency
fn main() -> Result<(), geo::ParseError> {
    let pts = geo::parse("0,0;3,4")?;
    println!("{}", geo::distance(&pts[0], &pts[1]));    // 5
    Ok(())
}
```

`&Point` stays `&Point`, `Result` stays `Result`, and `?` works on both sides.

---

## Built-ins

Built-ins look like ordinary calls and keep Rust's compile-time checking. Their names are
reserved.

| Rust | Vertex |
| --- | --- |
| `println!("{x}")`, `print!`, `eprintln!`, `eprint!` | `println("{x}")`, `print`, `eprintln`, `eprint` |
| `format!("{x:?}")` | `format("{x:?}")` |
| `vec![]`, `vec![1, 2, 3]`, `vec![0; n]` | `vec[]`, `vec[1, 2, 3]`, `vec[0; n]` |
| `assert!(ok)`, `assert_eq!(a, b)` | `assert(ok)`, `assertEq(a, b)` |
| `panic!("…")`, `todo!()`, `unreachable!()` | `panic("…")`, `todo()`, `unreachable()` |
| `asm!(…)`, `naked_asm!(…)`, `global_asm!(…)` | `asm(…)`, `nakedAsm(…)`, `globalAsm(…)` |
| `futures::join!(a, b)` | `task.join(a, b).await` (an ordinary function) |
| `matches!(x, Pat)` | `if let` or `match` |
| `write!(f, "…", x)` | `f.writeStr(&format("…", x))` |

There are no user-defined macros.

---

## A complete program

A small task store that reads tasks from a file, adds some, syncs from two URLs at once and
prints the open ones. It uses most of what's above.

```rust
// Rust
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::{fs, io};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Status { Todo, Doing, Done }

#[derive(Debug, Clone)]
struct Task {
    id: i64,
    title: String,
    status: Status,
}

impl Task {
    pub fn new(id: i64, title: String) -> Task {
        Task { id, title, status: Status::Todo }
    }
    pub fn is_open(&self) -> bool { self.status != Status::Done }
    pub fn advance(&mut self) {
        self.status = match self.status {
            Status::Todo => Status::Doing,
            Status::Doing | Status::Done => Status::Done,
        };
    }
}

trait Renderer {
    fn render(&self) -> String;
}

impl Renderer for Task {
    fn render(&self) -> String {
        let mark = match self.status {
            Status::Todo => " ",
            Status::Doing => "~",
            Status::Done => "x",
        };
        format!("[{mark}] #{} {}", self.id, self.title)
    }
}

#[derive(Debug)]
enum StoreError {
    Io(io::Error),
    Http(reqwest::Error),
    Parse { line: i64, message: String },
    NotFound(i64),
}

impl From<io::Error> for StoreError {
    fn from(e: io::Error) -> Self { StoreError::Io(e) }
}
impl From<reqwest::Error> for StoreError {
    fn from(e: reqwest::Error) -> Self { StoreError::Http(e) }
}

fn parse_line(line: &str, n: i64) -> Result<Task, StoreError> {
    let (raw_id, title) = line.split_once('|').ok_or(StoreError::Parse {
        line: n,
        message: "expected id|title".to_string(),
    })?;
    let id: i64 = raw_id.trim().parse().map_err(|_| StoreError::Parse {
        line: n,
        message: format!("bad id {raw_id:?}"),
    })?;
    Ok(Task::new(id, title.trim().to_string()))
}

struct StoreInner {
    tasks: HashMap<i64, Task>,
    next_id: i64,
}

#[derive(Clone)]
struct Store(Rc<RefCell<StoreInner>>);

impl Store {
    pub fn open(path: &str) -> Result<Store, StoreError> {
        let text = fs::read_to_string(path)?;
        let mut tasks = HashMap::new();
        for (i, line) in text.lines().enumerate() {
            let task = parse_line(line, i as i64 + 1)?;
            tasks.insert(task.id, task);
        }
        let next_id = tasks.keys().max().copied().unwrap_or(0) + 1;
        Ok(Store(Rc::new(RefCell::new(StoreInner { tasks, next_id }))))
    }

    pub fn add(&self, title: String) -> i64 {
        let mut s = self.0.borrow_mut();
        let id = s.next_id;
        s.next_id += 1;
        s.tasks.insert(id, Task::new(id, title));
        id
    }

    pub fn advance(&self, id: i64) -> Result<(), StoreError> {
        let mut s = self.0.borrow_mut();
        let task = s.tasks.get_mut(&id).ok_or(StoreError::NotFound(id))?;
        task.advance();
        Ok(())
    }

    pub fn open_tasks(&self) -> Vec<Task> {
        let mut open: Vec<Task> = self.0.borrow().tasks.values()
            .filter(|t| t.is_open())
            .cloned()
            .collect();
        open.sort_by_key(|t| t.id);
        open
    }
}

fn print_all<T: Renderer>(items: &[T]) {
    for item in items {
        println!("{}", item.render());
    }
}

fn longest_title<'a>(a: &'a Task, b: &'a Task) -> &'a str {
    if a.title.len() >= b.title.len() { &a.title } else { &b.title }
}

async fn sync_remote(store: &Store, url: &str) -> Result<usize, StoreError> {
    let body = reqwest::get(url).await?.text().await?;
    let mut added = 0;
    for line in body.lines() {
        store.add(line.to_string());
        added += 1;
    }
    Ok(added)
}

#[tokio::main]
async fn main() -> Result<(), StoreError> {
    let store = Store::open("tasks.txt")?;
    let id = store.add("write the usage guide".to_string());
    store.advance(id)?;

    let (a, b) = tokio::join!(
        sync_remote(&store, "https://example.com/a.txt"),
        sync_remote(&store, "https://example.com/b.txt"),
    );
    println!("synced {} + {} tasks", a?, b?);

    let open = store.open_tasks();
    print_all(&open);

    let doing = open.iter().filter(|t| t.status == Status::Doing).count();
    println!("{doing} in progress");

    if let [first, second, ..] = open.as_slice() {
        println!("longest title: {}", longest_title(first, second));
    }

    match store.advance(999) {
        Ok(()) => println!("advanced"),
        Err(StoreError::NotFound(id)) => println!("no task #{id}"),
        Err(e) => return Err(e),
    }
    Ok(())
}
```

```rust
// Vertex
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
```

---

## Quick reference

| Rust | Vertex |
| --- | --- |
| `mod geo;` / `use std::fs;` / `use a::B;` | folder packages / `import "std/fs"` / `import { B } from "a"` |
| `fn f(x: i32) -> i32` | `func f(x: int32) -> int32` |
| `let mut x = 0;` | `var x = 0` |
| `pub`, `pub(crate)` | `public`, `package` |
| `a::b::C` | `a.b.C` |
| `"5".parse::<i64>()`, `Vec::<i32>::new()` | `"5".parse<int64>()`, `Vec<int32>.new()` |
| `impl T { … }` | `implement T { … }` or `func (t: &T) m()` |
| `impl Tr for T { … }` | `implement Tr for T { … }` |
| `T::new()` | `T.new()` |
| `&self`, `&mut self`, `self`, `mut self` | the same, and `var self` |
| `#[derive(Debug)]` | `@derive(Debug)` |
| `#![allow(dead_code)]` | `@allow(deadCode)` on `package` |
| `struct P { x: f64, y: f64 }` | `struct P { x: float64; y: float64 }` |
| `enum E { A, B(i32) }` | `enum E { A; B(int32) }` |
| `A => 1,` | `A => 1` |
| `println!`, `format!`, `vec![…]` | `println`, `format`, `vec[…]` |
| `fn f<'a>(x: &'a str) -> &'a str` | `func f<a>(x: &a string) -> &a string` |
| `struct S<'a> { r: &'a str }` | `struct S<a> { r: &a string }` |
| `&'static str` | `&static string` |
| `'outer: loop { break 'outer }` | `outer: loop { break outer }` |
| `\|x\| x + 1`, `move \|x\| …` | `func(x) { x + 1 }` |
| `async move { … }` | `async { … }` |
| `Rc<dyn Fn(u32)>` | `func(uint32)` |
| `Rc<RefCell<T>>` | `class` |
| `i64`, `usize`, `f64`, `String`, `&str`, `!` | `int64`, `uint`, `float64`, `string`, `&string`, `never` |
| `c_int`, `CString`, `&CStr` | `c_int`, `cstring`, `&cstring` |
| `unsafe extern "C" { fn f(); type T; }` | `unsafe extern "C" { func f(); type T }` |
| `#[unsafe(export_name = "x")]` | `@unsafe(exportName = "x")` |
| `snake_case` | `camelCase` (`PascalCase` for types, traits, variants) |
