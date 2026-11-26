//! Utilities for the slice primitive type `[T]` (`core/slice`).

package slice

import { Option } from "core/option"
import { Iterator, IntoIterator, ExactSizeIterator, DoubleEndedIterator, FusedIterator } from "core/iter"
import { sliceLen } from "core/intrinsics"

/// Returns the number of elements in the slice.
public func len<T>(s: &[T]) -> uint {
    sliceLen(s)
}

/// Returns `true` if the slice has a length of 0.
public func isEmpty<T>(s: &[T]) -> bool {
    sliceLen(s) == 0
}

/// Returns the first element of the slice, or `None` if it is empty.
public func first<a, T>(s: &a [T]) -> Option<&a T> {
    if sliceLen(s) == 0 {
        Option.None
    } else {
        Option.Some(&s[0])
    }
}

/// Returns the last element of the slice, or `None` if it is empty.
public func last<a, T>(s: &a [T]) -> Option<&a T> {
    let n = sliceLen(s)
    if n == 0 {
        Option.None
    } else {
        Option.Some(&s[n - 1])
    }
}

/// Returns a reference to the element at `index`, or `None` if out of bounds.
public func get<a, T>(s: &a [T], index: uint) -> Option<&a T> {
    if index < sliceLen(s) {
        Option.Some(&s[index])
    } else {
        Option.None
    }
}

/// Immutable slice iterator.
public struct Iter<a, T> {
    s: &a [T]
    start: uint
    end: uint
}

/// Creates an iterator over the elements of a slice.
public func iter<a, T>(s: &a [T]) -> Iter<a, T> {
    Iter { s, start: 0, end: sliceLen(s) }
}

implement<a, T> Iterator for Iter<a, T> {
    type Item = &a T

    func next(&mut self) -> Option<&a T> {
        if self.start < self.end {
            let x = &self.s[self.start]
            self.start += 1
            Option.Some(x)
        } else {
            Option.None
        }
    }
}

implement<a, T> DoubleEndedIterator for Iter<a, T> {
    func nextBack(&mut self) -> Option<&a T> {
        if self.start < self.end {
            self.end -= 1
            Option.Some(&self.s[self.end])
        } else {
            Option.None
        }
    }
}

implement<a, T> ExactSizeIterator for Iter<a, T> {
    func len(&self) -> uint {
        self.end - self.start
    }
}

implement<a, T> FusedIterator for Iter<a, T> {}

implement<a, T, const N: uint> IntoIterator for &a [T; N] {
    type Item = &a T
    type IntoIter = Iter<a, T>

    func intoIter(self) -> Iter<a, T> {
        Iter { s: self, start: 0, end: sliceLen(self) }
    }
}

implement<a, T> IntoIterator for &a [T] {
    type Item = &a T
    type IntoIter = Iter<a, T>

    func intoIter(self) -> Iter<a, T> {
        Iter { s: self, start: 0, end: sliceLen(self) }
    }
}
