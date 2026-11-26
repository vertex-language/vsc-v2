//! Utilities for comparing and ordering values (`core/cmp`).

package cmp

import { Option } from "core/option"
import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Default } from "core/default"
import { FnOnce } from "core/ops"

/// An `Ordering` is the result of a comparison between two values.
@lang("Ordering")
public enum Ordering {
    Less
    Equal
    Greater
}

implement Copy for Ordering {}

implement Clone for Ordering {
    func clone(&self) -> Ordering { *self }
}

implement Ordering {
    public func isEq(self) -> bool {
        match self {
            Ordering.Equal => true
            _ => false
        }
    }

    public func isNe(self) -> bool {
        !self.isEq()
    }

    public func isLt(self) -> bool {
        match self {
            Ordering.Less => true
            _ => false
        }
    }

    public func isGt(self) -> bool {
        match self {
            Ordering.Greater => true
            _ => false
        }
    }

    public func isLe(self) -> bool {
        match self {
            Ordering.Less | Ordering.Equal => true
            Ordering.Greater => false
        }
    }

    public func isGe(self) -> bool {
        match self {
            Ordering.Greater | Ordering.Equal => true
            Ordering.Less => false
        }
    }

    public func reverse(self) -> Ordering {
        match self {
            Ordering.Less => Ordering.Greater
            Ordering.Equal => Ordering.Equal
            Ordering.Greater => Ordering.Less
        }
    }

    public func then(self, other: Ordering) -> Ordering {
        match self {
            Ordering.Equal => other
            _ => self
        }
    }

    public func thenWith<F: FnOnce() -> Ordering>(self, f: F) -> Ordering {
        match self {
            Ordering.Equal => f()
            _ => self
        }
    }
}

/// Trait for equality comparisons which are partial equivalence relations.
@lang("eq")
public trait PartialEq<Rhs = Self> {
    func eq(&self, other: &Rhs) -> bool

    func ne(&self, other: &Rhs) -> bool {
        !<Self as PartialEq<Rhs>>.eq(self, other)
    }
}

/// Trait for equality comparisons which are equivalence relations.
public trait Eq: PartialEq<Self> {}

/// Trait for values that can be compared for a sort-order.
@lang("partialOrd")
public trait PartialOrd<Rhs = Self>: PartialEq<Rhs> {
    func partialCmp(&self, other: &Rhs) -> Option<Ordering>

    func lt(&self, other: &Rhs) -> bool {
        match self.partialCmp(other) {
            Option.Some(Ordering.Less) => true
            _ => false
        }
    }

    func le(&self, other: &Rhs) -> bool {
        match self.partialCmp(other) {
            Option.Some(Ordering.Less | Ordering.Equal) => true
            _ => false
        }
    }

    func gt(&self, other: &Rhs) -> bool {
        match self.partialCmp(other) {
            Option.Some(Ordering.Greater) => true
            _ => false
        }
    }

    func ge(&self, other: &Rhs) -> bool {
        match self.partialCmp(other) {
            Option.Some(Ordering.Greater | Ordering.Equal) => true
            _ => false
        }
    }
}

/// Trait for types that form a total order.
public trait Ord: Eq + PartialOrd<Self> {
    func cmp(&self, other: &Self) -> Ordering

    func max(self, other: Self) -> Self {
        match self.cmp(&other) {
            Ordering.Less => other
            _ => self
        }
    }

    func min(self, other: Self) -> Self {
        match self.cmp(&other) {
            Ordering.Greater => other
            _ => self
        }
    }
}

implement PartialEq<Ordering> for Ordering {
    func eq(&self, other: &Ordering) -> bool {
        match (self, other) {
            (Ordering.Less, Ordering.Less) => true
            (Ordering.Equal, Ordering.Equal) => true
            (Ordering.Greater, Ordering.Greater) => true
            _ => false
        }
    }
}

implement Eq for Ordering {}

implement PartialOrd<Ordering> for Ordering {
    func partialCmp(&self, other: &Ordering) -> Option<Ordering> {
        Option.Some(self.cmp(other))
    }
}

implement Ord for Ordering {
    func cmp(&self, other: &Ordering) -> Ordering {
        match (self, other) {
            (Ordering.Less, Ordering.Less) => Ordering.Equal
            (Ordering.Less, _) => Ordering.Less
            (Ordering.Equal, Ordering.Less) => Ordering.Greater
            (Ordering.Equal, Ordering.Equal) => Ordering.Equal
            (Ordering.Equal, Ordering.Greater) => Ordering.Less
            (Ordering.Greater, Ordering.Greater) => Ordering.Equal
            (Ordering.Greater, _) => Ordering.Greater
        }
    }
}

implement Eq for bool {}
implement Eq for char {}
implement Eq for int8 {}
implement Eq for int16 {}
implement Eq for int32 {}
implement Eq for int64 {}
implement Eq for int128 {}
implement Eq for int {}
implement Eq for uint8 {}
implement Eq for uint16 {}
implement Eq for uint32 {}
implement Eq for uint64 {}
implement Eq for uint128 {}
implement Eq for uint {}

implement Ord for bool {
    func cmp(&self, other: &bool) -> Ordering {
        if *self == *other { Ordering.Equal } else if !*self { Ordering.Less } else { Ordering.Greater }
    }
}

implement Ord for char {
    func cmp(&self, other: &char) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for int8 {
    func cmp(&self, other: &int8) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for int16 {
    func cmp(&self, other: &int16) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for int32 {
    func cmp(&self, other: &int32) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for int64 {
    func cmp(&self, other: &int64) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for int128 {
    func cmp(&self, other: &int128) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for int {
    func cmp(&self, other: &int) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for uint8 {
    func cmp(&self, other: &uint8) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for uint16 {
    func cmp(&self, other: &uint16) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for uint32 {
    func cmp(&self, other: &uint32) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for uint64 {
    func cmp(&self, other: &uint64) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for uint128 {
    func cmp(&self, other: &uint128) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement Ord for uint {
    func cmp(&self, other: &uint) -> Ordering {
        if *self < *other { Ordering.Less } else if *self > *other { Ordering.Greater } else { Ordering.Equal }
    }
}

implement<a, b, A: PartialEq<B>, B> PartialEq<&b B> for &a A {
    func eq(&self, other: &&b B) -> bool {
        (**self).eq(*other)
    }
}

/// A helper struct for reverse ordering.
public struct Reverse<T> {
    public value: T
}

implement<T: Copy> Copy for Reverse<T> {}

implement<T: Clone> Clone for Reverse<T> {
    func clone(&self) -> Reverse<T> {
        Reverse { value: self.value.clone() }
    }
}

implement<T: Default> Default for Reverse<T> {
    func default() -> Reverse<T> {
        Reverse { value: T.default() }
    }
}

implement<T: PartialEq<T>> PartialEq<Reverse<T>> for Reverse<T> {
    func eq(&self, other: &Reverse<T>) -> bool {
        self.value.eq(&other.value)
    }
}

implement<T: Eq> Eq for Reverse<T> {}

implement<T: PartialOrd<T>> PartialOrd<Reverse<T>> for Reverse<T> {
    func partialCmp(&self, other: &Reverse<T>) -> Option<Ordering> {
        other.value.partialCmp(&self.value)
    }
}

implement<T: Ord> Ord for Reverse<T> {
    func cmp(&self, other: &Reverse<T>) -> Ordering {
        other.value.cmp(&self.value)
    }
}

/// Compares and returns the minimum of two values.
public func min<T: Ord>(v1: T, v2: T) -> T {
    match v1.cmp(&v2) {
        Ordering.Greater => v2
        _ => v1
    }
}

/// Compares and returns the maximum of two values.
public func max<T: Ord>(v1: T, v2: T) -> T {
    match v1.cmp(&v2) {
        Ordering.Less => v2
        _ => v1
    }
}

/// Restricts a value to a certain interval `[lo, hi]`.
public func clamp<T: Ord>(value: T, lo: T, hi: T) -> T {
    if value.cmp(&lo).isLt() {
        lo
    } else if value.cmp(&hi).isGt() {
        hi
    } else {
        value
    }
}
