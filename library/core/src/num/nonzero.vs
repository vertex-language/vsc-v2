//! Non-zero integer wrapper types (`core/num`).

package num

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Ordering, PartialEq, Eq, PartialOrd, Ord } from "core/cmp"
import { Option } from "core/option"

/// A `uint8` that is known not to equal zero.
public struct NonZeroUint8 {
    inner: uint8
}

implement Copy for NonZeroUint8 {}
implement Clone for NonZeroUint8 {
    func clone(&self) -> NonZeroUint8 { *self }
}
implement PartialEq<NonZeroUint8> for NonZeroUint8 {
    func eq(&self, other: &NonZeroUint8) -> bool { self.inner == other.inner }
}
implement Eq for NonZeroUint8 {}
implement PartialOrd<NonZeroUint8> for NonZeroUint8 {
    func partialCmp(&self, other: &NonZeroUint8) -> Option<Ordering> { Option.Some(self.cmp(other)) }
}
implement Ord for NonZeroUint8 {
    func cmp(&self, other: &NonZeroUint8) -> Ordering { self.inner.cmp(&other.inner) }
}
implement NonZeroUint8 {
    public func new(n: uint8) -> Option<NonZeroUint8> {
        if n == 0 { Option.None } else { Option.Some(NonZeroUint8 { inner: n }) }
    }
    public unsafe func newUnchecked(n: uint8) -> NonZeroUint8 {
        NonZeroUint8 { inner: n }
    }
    public func get(self) -> uint8 { self.inner }
}

/// A `uint16` that is known not to equal zero.
public struct NonZeroUint16 {
    inner: uint16
}

implement Copy for NonZeroUint16 {}
implement Clone for NonZeroUint16 {
    func clone(&self) -> NonZeroUint16 { *self }
}
implement PartialEq<NonZeroUint16> for NonZeroUint16 {
    func eq(&self, other: &NonZeroUint16) -> bool { self.inner == other.inner }
}
implement Eq for NonZeroUint16 {}
implement PartialOrd<NonZeroUint16> for NonZeroUint16 {
    func partialCmp(&self, other: &NonZeroUint16) -> Option<Ordering> { Option.Some(self.cmp(other)) }
}
implement Ord for NonZeroUint16 {
    func cmp(&self, other: &NonZeroUint16) -> Ordering { self.inner.cmp(&other.inner) }
}
implement NonZeroUint16 {
    public func new(n: uint16) -> Option<NonZeroUint16> {
        if n == 0 { Option.None } else { Option.Some(NonZeroUint16 { inner: n }) }
    }
    public unsafe func newUnchecked(n: uint16) -> NonZeroUint16 {
        NonZeroUint16 { inner: n }
    }
    public func get(self) -> uint16 { self.inner }
}

/// A `uint32` that is known not to equal zero.
public struct NonZeroUint32 {
    inner: uint32
}

implement Copy for NonZeroUint32 {}
implement Clone for NonZeroUint32 {
    func clone(&self) -> NonZeroUint32 { *self }
}
implement PartialEq<NonZeroUint32> for NonZeroUint32 {
    func eq(&self, other: &NonZeroUint32) -> bool { self.inner == other.inner }
}
implement Eq for NonZeroUint32 {}
implement PartialOrd<NonZeroUint32> for NonZeroUint32 {
    func partialCmp(&self, other: &NonZeroUint32) -> Option<Ordering> { Option.Some(self.cmp(other)) }
}
implement Ord for NonZeroUint32 {
    func cmp(&self, other: &NonZeroUint32) -> Ordering { self.inner.cmp(&other.inner) }
}
implement NonZeroUint32 {
    public func new(n: uint32) -> Option<NonZeroUint32> {
        if n == 0 { Option.None } else { Option.Some(NonZeroUint32 { inner: n }) }
    }
    public unsafe func newUnchecked(n: uint32) -> NonZeroUint32 {
        NonZeroUint32 { inner: n }
    }
    public func get(self) -> uint32 { self.inner }
}

/// A `uint64` that is known not to equal zero.
public struct NonZeroUint64 {
    inner: uint64
}

implement Copy for NonZeroUint64 {}
implement Clone for NonZeroUint64 {
    func clone(&self) -> NonZeroUint64 { *self }
}
implement PartialEq<NonZeroUint64> for NonZeroUint64 {
    func eq(&self, other: &NonZeroUint64) -> bool { self.inner == other.inner }
}
implement Eq for NonZeroUint64 {}
implement PartialOrd<NonZeroUint64> for NonZeroUint64 {
    func partialCmp(&self, other: &NonZeroUint64) -> Option<Ordering> { Option.Some(self.cmp(other)) }
}
implement Ord for NonZeroUint64 {
    func cmp(&self, other: &NonZeroUint64) -> Ordering { self.inner.cmp(&other.inner) }
}
implement NonZeroUint64 {
    public func new(n: uint64) -> Option<NonZeroUint64> {
        if n == 0 { Option.None } else { Option.Some(NonZeroUint64 { inner: n }) }
    }
    public unsafe func newUnchecked(n: uint64) -> NonZeroUint64 {
        NonZeroUint64 { inner: n }
    }
    public func get(self) -> uint64 { self.inner }
}

/// A `uint` that is known not to equal zero.
public struct NonZeroUint {
    inner: uint
}

implement Copy for NonZeroUint {}
implement Clone for NonZeroUint {
    func clone(&self) -> NonZeroUint { *self }
}
implement PartialEq<NonZeroUint> for NonZeroUint {
    func eq(&self, other: &NonZeroUint) -> bool { self.inner == other.inner }
}
implement Eq for NonZeroUint {}
implement PartialOrd<NonZeroUint> for NonZeroUint {
    func partialCmp(&self, other: &NonZeroUint) -> Option<Ordering> { Option.Some(self.cmp(other)) }
}
implement Ord for NonZeroUint {
    func cmp(&self, other: &NonZeroUint) -> Ordering { self.inner.cmp(&other.inner) }
}
implement NonZeroUint {
    public func new(n: uint) -> Option<NonZeroUint> {
        if n == 0 { Option.None } else { Option.Some(NonZeroUint { inner: n }) }
    }
    public unsafe func newUnchecked(n: uint) -> NonZeroUint {
        NonZeroUint { inner: n }
    }
    public func get(self) -> uint { self.inner }
}

/// An `int32` that is known not to equal zero.
public struct NonZeroInt32 {
    inner: int32
}

implement Copy for NonZeroInt32 {}
implement Clone for NonZeroInt32 {
    func clone(&self) -> NonZeroInt32 { *self }
}
implement PartialEq<NonZeroInt32> for NonZeroInt32 {
    func eq(&self, other: &NonZeroInt32) -> bool { self.inner == other.inner }
}
implement Eq for NonZeroInt32 {}
implement PartialOrd<NonZeroInt32> for NonZeroInt32 {
    func partialCmp(&self, other: &NonZeroInt32) -> Option<Ordering> { Option.Some(self.cmp(other)) }
}
implement Ord for NonZeroInt32 {
    func cmp(&self, other: &NonZeroInt32) -> Ordering { self.inner.cmp(&other.inner) }
}
implement NonZeroInt32 {
    public func new(n: int32) -> Option<NonZeroInt32> {
        if n == 0 { Option.None } else { Option.Some(NonZeroInt32 { inner: n }) }
    }
    public unsafe func newUnchecked(n: int32) -> NonZeroInt32 {
        NonZeroInt32 { inner: n }
    }
    public func get(self) -> int32 { self.inner }
}

/// An `int64` that is known not to equal zero.
public struct NonZeroInt64 {
    inner: int64
}

implement Copy for NonZeroInt64 {}
implement Clone for NonZeroInt64 {
    func clone(&self) -> NonZeroInt64 { *self }
}
implement PartialEq<NonZeroInt64> for NonZeroInt64 {
    func eq(&self, other: &NonZeroInt64) -> bool { self.inner == other.inner }
}
implement Eq for NonZeroInt64 {}
implement PartialOrd<NonZeroInt64> for NonZeroInt64 {
    func partialCmp(&self, other: &NonZeroInt64) -> Option<Ordering> { Option.Some(self.cmp(other)) }
}
implement Ord for NonZeroInt64 {
    func cmp(&self, other: &NonZeroInt64) -> Ordering { self.inner.cmp(&other.inner) }
}
implement NonZeroInt64 {
    public func new(n: int64) -> Option<NonZeroInt64> {
        if n == 0 { Option.None } else { Option.Some(NonZeroInt64 { inner: n }) }
    }
    public unsafe func newUnchecked(n: int64) -> NonZeroInt64 {
        NonZeroInt64 { inner: n }
    }
    public func get(self) -> int64 { self.inner }
}

/// An `int` that is known not to equal zero.
public struct NonZeroInt {
    inner: int
}

implement Copy for NonZeroInt {}
implement Clone for NonZeroInt {
    func clone(&self) -> NonZeroInt { *self }
}
implement PartialEq<NonZeroInt> for NonZeroInt {
    func eq(&self, other: &NonZeroInt) -> bool { self.inner == other.inner }
}
implement Eq for NonZeroInt {}
implement PartialOrd<NonZeroInt> for NonZeroInt {
    func partialCmp(&self, other: &NonZeroInt) -> Option<Ordering> { Option.Some(self.cmp(other)) }
}
implement Ord for NonZeroInt {
    func cmp(&self, other: &NonZeroInt) -> Ordering { self.inner.cmp(&other.inner) }
}
implement NonZeroInt {
    public func new(n: int) -> Option<NonZeroInt> {
        if n == 0 { Option.None } else { Option.Some(NonZeroInt { inner: n }) }
    }
    public unsafe func newUnchecked(n: int) -> NonZeroInt {
        NonZeroInt { inner: n }
    }
    public func get(self) -> int { self.inner }
}
