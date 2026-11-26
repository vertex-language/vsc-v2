//! The `Default` trait for giving a type a useful default value (`core/default`).

package default

/// A trait for giving a type a useful default value.
public trait Default {
    func default() -> Self
}

implement Default for bool {
    func default() -> bool { false }
}

implement Default for char {
    func default() -> char { '\0' }
}

implement Default for int8 {
    func default() -> int8 { 0 }
}

implement Default for int16 {
    func default() -> int16 { 0 }
}

implement Default for int32 {
    func default() -> int32 { 0 }
}

implement Default for int64 {
    func default() -> int64 { 0 }
}

implement Default for int128 {
    func default() -> int128 { 0 }
}

implement Default for int {
    func default() -> int { 0 }
}

implement Default for uint8 {
    func default() -> uint8 { 0 }
}

implement Default for uint16 {
    func default() -> uint16 { 0 }
}

implement Default for uint32 {
    func default() -> uint32 { 0 }
}

implement Default for uint64 {
    func default() -> uint64 { 0 }
}

implement Default for uint128 {
    func default() -> uint128 { 0 }
}

implement Default for uint {
    func default() -> uint { 0 }
}

implement Default for float32 {
    func default() -> float32 { 0.0 }
}

implement Default for float64 {
    func default() -> float64 { 0.0 }
}

implement Default for () {
    func default() -> () { () }
}
