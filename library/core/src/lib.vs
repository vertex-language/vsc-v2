//! The Vertex Core Library (`core`).
//!
//! Dependency-free foundation of the Vertex standard library, mirroring
//! Rust's `library/core` module layout in pure Vertex source.

package core

/// Compiler wrapper for heap-allocated `class` values (Vertex exception 3).
@lang("Class")
public struct Class<T> {
    inner: T
}

/// Compiler wrapper for `func(...)` function values (Vertex exception 14).
@lang("Func")
public struct Func<F> {
    f: F
}
