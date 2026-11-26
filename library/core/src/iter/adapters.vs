//! Iterator adapters (`core/iter`).

package iter

import { Option } from "core/option"
import { FnMut } from "core/ops"
import { panicMsg } from "core/panicking"

/// An iterator that only iterates over the first `n` iterations of `iter`.
public struct Take<I> {
    package iter: I
    package n: uint
}

implement<I: Iterator> Iterator for Take<I> {
    type Item = <I as Iterator>.Item

    func next(&mut self) -> Option<<I as Iterator>.Item> {
        if self.n != 0 {
            self.n -= 1
            self.iter.next()
        } else {
            Option.None
        }
    }
}

/// An iterator that skips over `n` elements of `iter`.
public struct Skip<I> {
    package iter: I
    package n: uint
}

implement<I: Iterator> Iterator for Skip<I> {
    type Item = <I as Iterator>.Item

    func next(&mut self) -> Option<<I as Iterator>.Item> {
        while self.n > 0 {
            self.n -= 1
            match self.iter.next() {
                Option.Some(_) => {}
                Option.None => return Option.None
            }
        }
        self.iter.next()
    }
}

/// An iterator for stepping iterators by a custom amount.
public struct StepBy<I> {
    iter: I
    stepMinusOne: uint
    firstTake: bool
}

implement<I> StepBy<I> {
    package func new(iter: I, step: uint) -> StepBy<I> {
        if step == 0 {
            panicMsg("assertion failed: step != 0")
        }
        StepBy { iter, stepMinusOne: step - 1, firstTake: true }
    }
}

implement<I: Iterator> Iterator for StepBy<I> {
    type Item = <I as Iterator>.Item

    func next(&mut self) -> Option<<I as Iterator>.Item> {
        if self.firstTake {
            self.firstTake = false
            self.iter.next()
        } else {
            self.iter.nth(self.stepMinusOne)
        }
    }
}

/// An iterator that yields the current count and the element during iteration.
public struct Enumerate<I> {
    package iter: I
    package count: uint
}

implement<I: Iterator> Iterator for Enumerate<I> {
    type Item = (uint, <I as Iterator>.Item)

    func next(&mut self) -> Option<(uint, <I as Iterator>.Item)> {
        match self.iter.next() {
            Option.Some(a) => {
                let i = self.count
                self.count += 1
                Option.Some((i, a))
            }
            Option.None => Option.None
        }
    }
}

/// An iterator that links two iterators together, in a chain.
public struct Chain<A, B> {
    package a: Option<A>
    package b: B
}

implement<A: Iterator, B: Iterator<Item = <A as Iterator>.Item>> Iterator for Chain<A, B> {
    type Item = <A as Iterator>.Item

    func next(&mut self) -> Option<<A as Iterator>.Item> {
        match &mut self.a {
            Option.Some(iterA) => match iterA.next() {
                Option.Some(v) => return Option.Some(v)
                Option.None => {
                    self.a = Option.None
                }
            }
            Option.None => {}
        }
        self.b.next()
    }
}

/// An iterator that iterates two other iterators simultaneously.
public struct Zip<A, B> {
    package a: A
    package b: B
}

implement<A: Iterator, B: Iterator> Iterator for Zip<A, B> {
    type Item = (<A as Iterator>.Item, <B as Iterator>.Item)

    func next(&mut self) -> Option<(<A as Iterator>.Item, <B as Iterator>.Item)> {
        match self.a.next() {
            Option.Some(x) => match self.b.next() {
                Option.Some(y) => Option.Some((x, y))
                Option.None => Option.None
            }
            Option.None => Option.None
        }
    }
}

/// An iterator that maps the values of `iter` with `f`.
public struct Map<I, F> {
    package iter: I
    package f: F
}

implement<B, I: Iterator, F: FnMut(<I as Iterator>.Item) -> B> Iterator for Map<I, F> {
    type Item = B

    func next(&mut self) -> Option<B> {
        match self.iter.next() {
            Option.Some(x) => Option.Some((self.f)(x))
            Option.None => Option.None
        }
    }
}

/// An iterator that filters the elements of `iter` with `predicate`.
public struct Filter<I, P> {
    package iter: I
    package predicate: P
}

implement<I: Iterator, P: for<a> FnMut(&a I.Item) -> bool> Iterator for Filter<I, P> {
    type Item = I.Item

    func next(&mut self) -> Option<I.Item> {
        loop {
            match self.iter.next() {
                Option.Some(x) => {
                    if (self.predicate)(&x) {
                        return Option.Some(x)
                    }
                }
                Option.None => return Option.None
            }
        }
    }
}
