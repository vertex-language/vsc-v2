//! The `Clone` trait for types that cannot be implicitly copied (`core/clone`).

package clone

/// A common trait that allows explicit creation of a duplicate value.
public trait Clone {
    func clone(&self) -> Self

    func cloneFrom(&mut self, source: &Self) {
        *self = (*source).clone()
    }
}

implement Clone for bool {
    func clone(&self) -> bool { *self }
}

implement Clone for char {
    func clone(&self) -> char { *self }
}

implement Clone for int8 {
    func clone(&self) -> int8 { *self }
}

implement Clone for int16 {
    func clone(&self) -> int16 { *self }
}

implement Clone for int32 {
    func clone(&self) -> int32 { *self }
}

implement Clone for int64 {
    func clone(&self) -> int64 { *self }
}

implement Clone for int128 {
    func clone(&self) -> int128 { *self }
}

implement Clone for int {
    func clone(&self) -> int { *self }
}

implement Clone for uint8 {
    func clone(&self) -> uint8 { *self }
}

implement Clone for uint16 {
    func clone(&self) -> uint16 { *self }
}

implement Clone for uint32 {
    func clone(&self) -> uint32 { *self }
}

implement Clone for uint64 {
    func clone(&self) -> uint64 { *self }
}

implement Clone for uint128 {
    func clone(&self) -> uint128 { *self }
}

implement Clone for uint {
    func clone(&self) -> uint { *self }
}

implement Clone for float32 {
    func clone(&self) -> float32 { *self }
}

implement Clone for float64 {
    func clone(&self) -> float64 { *self }
}

implement Clone for () {
    func clone(&self) -> () { () }
}

implement<a, T> Clone for &a T {
    func clone(&self) -> &a T { *self }
}
