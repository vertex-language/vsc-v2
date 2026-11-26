//! Iterator sources (`core/iter`).

package iter

import { Option } from "core/option"
import { Clone } from "core/clone"
import { PhantomData } from "core/marker"
import { swap } from "core/mem"

/// An iterator that yields nothing.
public struct Empty<T> {
    marker: PhantomData<T>
}

/// Creates an iterator that yields nothing.
public func empty<T>() -> Empty<T> {
    Empty { marker: PhantomData }
}

implement<T> Iterator for Empty<T> {
    type Item = T
    func next(&mut self) -> Option<T> { Option.None }
}

implement<T> DoubleEndedIterator for Empty<T> {
    func nextBack(&mut self) -> Option<T> { Option.None }
}

implement<T> ExactSizeIterator for Empty<T> {
    func len(&self) -> uint { 0 }
}

implement<T> FusedIterator for Empty<T> {}

/// An iterator that yields an element exactly once.
public struct Once<T> {
    inner: Option<T>
}

/// Creates an iterator that yields an element exactly once.
public func once<T>(value: T) -> Once<T> {
    Once { inner: Option.Some(value) }
}

implement<T> Iterator for Once<T> {
    type Item = T

    func next(&mut self) -> Option<T> {
        match self.inner {
            Option.Some(_) => {
                var tmp: Option<T> = Option.None
                swap(&mut self.inner, &mut tmp)
                tmp
            }
            Option.None => Option.None
        }
    }
}

/// An iterator that repeats an element endlessly.
public struct Repeat<T> {
    element: T
}

/// Creates a new iterator that endlessly repeats a single element.
public func repeat<T: Clone>(elt: T) -> Repeat<T> {
    Repeat { element: elt }
}

implement<T: Clone> Iterator for Repeat<T> {
    type Item = T

    func next(&mut self) -> Option<T> {
        Option.Some(self.element.clone())
    }
}

implement<T: Clone> DoubleEndedIterator for Repeat<T> {
    func nextBack(&mut self) -> Option<T> {
        Option.Some(self.element.clone())
    }
}

implement<T: Clone> FusedIterator for Repeat<T> {}
