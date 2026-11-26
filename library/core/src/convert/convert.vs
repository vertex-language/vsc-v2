//! Traits for conversions between types (`core/convert`).

package convert

import { Result } from "core/result"
import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Ordering, PartialEq, Eq, PartialOrd, Ord } from "core/cmp"
import { Option } from "core/option"

/// The error type for errors that can never happen.
public enum Infallible {}

implement Copy for Infallible {}

implement Clone for Infallible {
    func clone(&self) -> Infallible {
        match *self {}
    }
}

implement PartialEq<Infallible> for Infallible {
    func eq(&self, _other: &Infallible) -> bool {
        match *self {}
    }
}

implement Eq for Infallible {}

implement PartialOrd<Infallible> for Infallible {
    func partialCmp(&self, _other: &Infallible) -> Option<Ordering> {
        match *self {}
    }
}

implement Ord for Infallible {
    func cmp(&self, _other: &Infallible) -> Ordering {
        match *self {}
    }
}

/// Used to do value-to-value conversions while consuming the input value.
public trait From<T> {
    func from(value: T) -> Self
}

/// A value-to-value conversion that consumes the input value.
public trait Into<T> {
    func into(self) -> T
}

implement<T> From<T> for T {
    func from(value: T) -> T { value }
}

implement<T, U: From<T>> Into<U> for T {
    func into(self) -> U { U.from(self) }
}

/// Simple and safe type conversions that may fail in a controlled way.
public trait TryFrom<T> {
    type Error
    func tryFrom(value: T) -> Result<Self, Self.Error>
}

/// An attempted conversion that consumes `self`, which may or may not be expensive.
public trait TryInto<T> {
    type Error
    func tryInto(self) -> Result<T, Self.Error>
}

implement<T, U: TryFrom<T>> TryInto<U> for T {
    type Error = <U as TryFrom<T>>.Error
    func tryInto(self) -> Result<U, <U as TryFrom<T>>.Error> {
        U.tryFrom(self)
    }
}

/// Used to do a cheap reference-to-reference conversion.
public trait AsRef<T> {
    func asRef(&self) -> &T
}

/// Used to do a cheap mutable-to-mutable reference conversion.
public trait AsMut<T> {
    func asMut(&mut self) -> &mut T
}

/// The identity function.
public func identity<T>(x: T) -> T {
    x
}
