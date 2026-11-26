//! Composable external iteration traits (`core/iter`).

package iter

import { Option } from "core/option"
import { FnMut } from "core/ops"

/// A trait for dealing with iterators.
public trait Iterator {
    type Item

    @lang("next")
    func next(&mut self) -> Option<Self.Item>

    func count(var self) -> uint {
        var n: uint = 0
        loop {
            match self.next() {
                Option.Some(_) => { n += 1 }
                Option.None => break
            }
        }
        n
    }

    func last(var self) -> Option<Self.Item> {
        var last: Option<Self.Item> = Option.None
        loop {
            match self.next() {
                Option.Some(x) => { last = Option.Some(x) }
                Option.None => break
            }
        }
        last
    }

    func nth(&mut self, var n: uint) -> Option<Self.Item> {
        loop {
            match self.next() {
                Option.Some(x) => {
                    if n == 0 {
                        return Option.Some(x)
                    }
                    n -= 1
                }
                Option.None => return Option.None
            }
        }
    }

    func fold<B, F: FnMut(B, Self.Item) -> B>(var self, init: B, var f: F) -> B {
        var accum = init
        loop {
            match self.next() {
                Option.Some(x) => { accum = f(accum, x) }
                Option.None => break
            }
        }
        accum
    }

    func forEach<F: FnMut(Self.Item)>(var self, var f: F) {
        loop {
            match self.next() {
                Option.Some(x) => f(x)
                Option.None => break
            }
        }
    }

    func any<F: FnMut(Self.Item) -> bool>(&mut self, var f: F) -> bool {
        loop {
            match self.next() {
                Option.Some(x) => {
                    if f(x) {
                        return true
                    }
                }
                Option.None => return false
            }
        }
    }

    func all<F: FnMut(Self.Item) -> bool>(&mut self, var f: F) -> bool {
        loop {
            match self.next() {
                Option.Some(x) => {
                    if !f(x) {
                        return false
                    }
                }
                Option.None => return true
            }
        }
    }

    func find<P: for<a> FnMut(&a Self.Item) -> bool>(&mut self, var predicate: P) -> Option<Self.Item> {
        loop {
            match self.next() {
                Option.Some(x) => {
                    if predicate(&x) {
                        return Option.Some(x)
                    }
                }
                Option.None => return Option.None
            }
        }
    }

    func position<P: FnMut(Self.Item) -> bool>(&mut self, var predicate: P) -> Option<uint> {
        var i: uint = 0
        loop {
            match self.next() {
                Option.Some(x) => {
                    if predicate(x) {
                        return Option.Some(i)
                    }
                    i += 1
                }
                Option.None => return Option.None
            }
        }
    }

    func take(self, n: uint) -> Take<Self> {
        Take { iter: self, n }
    }

    func skip(self, n: uint) -> Skip<Self> {
        Skip { iter: self, n }
    }

    func stepBy(self, step: uint) -> StepBy<Self> {
        StepBy.new(self, step)
    }

    func enumerate(self) -> Enumerate<Self> {
        Enumerate { iter: self, count: 0 }
    }

    func chain<U: IntoIterator>(self, other: U) -> Chain<Self, <U as IntoIterator>.IntoIter> {
        Chain { a: Option.Some(self), b: other.intoIter() }
    }

    func zip<U: IntoIterator>(self, other: U) -> Zip<Self, <U as IntoIterator>.IntoIter> {
        Zip { a: self, b: other.intoIter() }
    }

    func map<B, F: FnMut(Self.Item) -> B>(self, f: F) -> Map<Self, F> {
        Map { iter: self, f }
    }

    func filter<P: for<a> FnMut(&a Self.Item) -> bool>(self, predicate: P) -> Filter<Self, P> {
        Filter { iter: self, predicate }
    }
}

/// Conversion into an `Iterator`.
public trait IntoIterator {
    type Item
    type IntoIter

    @lang("intoIter")
    func intoIter(self) -> Self.IntoIter
}

implement<I: Iterator> IntoIterator for I {
    type Item = <I as Iterator>.Item
    type IntoIter = I

    func intoIter(self) -> I { self }
}

/// An iterator able to yield elements from both ends.
public trait DoubleEndedIterator: Iterator {
    func nextBack(&mut self) -> Option<<Self as Iterator>.Item>
}

/// An iterator that knows its exact length.
public trait ExactSizeIterator: Iterator {
    func len(&self) -> uint
}

/// An iterator that always continues to yield `None` when exhausted.
public trait FusedIterator: Iterator {}
