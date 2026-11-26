//! Optional values (`core/option`).

package option

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Default } from "core/default"
import { Ordering, PartialEq, Eq, PartialOrd, Ord } from "core/cmp"
import { Infallible } from "core/convert"
import { ControlFlow, FromResidual, Try, FnOnce } from "core/ops"
import { Result } from "core/result"
import { panicMsg } from "core/panicking"
import { replace as memReplace } from "core/mem"

/// The `Option` type represents an optional value: every `Option` is either
/// `Some` and contains a value, or `None`, and does not.
public enum Option<T> {
    @lang("None")
    None
    @lang("Some")
    Some(T)
}

implement<T> Option<T> {
    public func isSome(&self) -> bool {
        match self {
            Option.Some(_) => true
            Option.None => false
        }
    }

    public func isNone(&self) -> bool {
        !self.isSome()
    }

    public func isSomeAnd<F: FnOnce(T) -> bool>(self, f: F) -> bool {
        match self {
            Option.Some(x) => f(x)
            Option.None => false
        }
    }

    public func asRef(&self) -> Option<&T> {
        match self {
            Option.Some(x) => Option.Some(x)
            Option.None => Option.None
        }
    }

    public func asMut(&mut self) -> Option<&mut T> {
        match self {
            Option.Some(x) => Option.Some(x)
            Option.None => Option.None
        }
    }

    public func expect(self, msg: &string) -> T {
        match self {
            Option.Some(x) => x
            Option.None => panicMsg(msg)
        }
    }

    public func unwrap(self) -> T {
        match self {
            Option.Some(x) => x
            Option.None => panicMsg("called `Option.unwrap()` on a `None` value")
        }
    }

    public func unwrapOr(self, default: T) -> T {
        match self {
            Option.Some(x) => x
            Option.None => default
        }
    }

    public func unwrapOrElse<F: FnOnce() -> T>(self, f: F) -> T {
        match self {
            Option.Some(x) => x
            Option.None => f()
        }
    }

    public func map<U, F: FnOnce(T) -> U>(self, f: F) -> Option<U> {
        match self {
            Option.Some(x) => Option.Some(f(x))
            Option.None => Option.None
        }
    }

    public func mapOr<U, F: FnOnce(T) -> U>(self, default: U, f: F) -> U {
        match self {
            Option.Some(x) => f(x)
            Option.None => default
        }
    }

    public func mapOrElse<U, D: FnOnce() -> U, F: FnOnce(T) -> U>(self, default: D, f: F) -> U {
        match self {
            Option.Some(x) => f(x)
            Option.None => default()
        }
    }

    public func okOr<E>(self, err: E) -> Result<T, E> {
        match self {
            Option.Some(v) => Result.Ok(v)
            Option.None => Result.Err(err)
        }
    }

    public func okOrElse<E, F: FnOnce() -> E>(self, err: F) -> Result<T, E> {
        match self {
            Option.Some(v) => Result.Ok(v)
            Option.None => Result.Err(err())
        }
    }

    public func and<U>(self, optb: Option<U>) -> Option<U> {
        match self {
            Option.Some(_) => optb
            Option.None => Option.None
        }
    }

    public func andThen<U, F: FnOnce(T) -> Option<U>>(self, f: F) -> Option<U> {
        match self {
            Option.Some(x) => f(x)
            Option.None => Option.None
        }
    }

    public func filter<P: for<a> FnOnce(&a T) -> bool>(self, predicate: P) -> Option<T> {
        match self {
            Option.Some(x) => {
                if predicate(&x) {
                    Option.Some(x)
                } else {
                    Option.None
                }
            }
            Option.None => Option.None
        }
    }

    public func or(self, optb: Option<T>) -> Option<T> {
        match self {
            Option.Some(x) => Option.Some(x)
            Option.None => optb
        }
    }

    public func orElse<F: FnOnce() -> Option<T>>(self, f: F) -> Option<T> {
        match self {
            Option.Some(x) => Option.Some(x)
            Option.None => f()
        }
    }

    public func xor(self, optb: Option<T>) -> Option<T> {
        match (self, optb) {
            (Option.Some(a), Option.None) => Option.Some(a)
            (Option.None, Option.Some(b)) => Option.Some(b)
            _ => Option.None
        }
    }

    public func zip<U>(self, other: Option<U>) -> Option<(T, U)> {
        match (self, other) {
            (Option.Some(a), Option.Some(b)) => Option.Some((a, b))
            _ => Option.None
        }
    }

    public func take(&mut self) -> Option<T> {
        memReplace(self, Option.None)
    }

    public func replace(&mut self, value: T) -> Option<T> {
        memReplace(self, Option.Some(value))
    }
}

implement<T: Default> Option<T> {
    public func unwrapOrDefault(self) -> T {
        match self {
            Option.Some(x) => x
            Option.None => T.default()
        }
    }
}

implement<a, T: Copy> Option<&a T> {
    public func copied(self) -> Option<T> {
        match self {
            Option.Some(x) => Option.Some(*x)
            Option.None => Option.None
        }
    }
}

implement<a, T: Clone> Option<&a T> {
    public func cloned(self) -> Option<T> {
        match self {
            Option.Some(x) => Option.Some((*x).clone())
            Option.None => Option.None
        }
    }
}

implement<T> Option<Option<T>> {
    public func flatten(self) -> Option<T> {
        match self {
            Option.Some(inner) => inner
            Option.None => Option.None
        }
    }
}

implement<T: Copy> Copy for Option<T> {}

implement<T: Clone> Clone for Option<T> {
    func clone(&self) -> Option<T> {
        match self {
            Option.Some(x) => Option.Some((*x).clone())
            Option.None => Option.None
        }
    }
}

implement<T> Default for Option<T> {
    func default() -> Option<T> {
        Option.None
    }
}

implement<T: PartialEq<T>> PartialEq<Option<T>> for Option<T> {
    func eq(&self, other: &Option<T>) -> bool {
        match (self, other) {
            (Option.Some(a), Option.Some(b)) => (*a).eq(b)
            (Option.None, Option.None) => true
            _ => false
        }
    }
}

implement<T: Eq> Eq for Option<T> {}

implement<T: PartialOrd<T>> PartialOrd<Option<T>> for Option<T> {
    func partialCmp(&self, other: &Option<T>) -> Option<Ordering> {
        match (self, other) {
            (Option.Some(a), Option.Some(b)) => a.partialCmp(b)
            (Option.None, Option.None) => Option.Some(Ordering.Equal)
            (Option.None, Option.Some(_)) => Option.Some(Ordering.Less)
            (Option.Some(_), Option.None) => Option.Some(Ordering.Greater)
        }
    }
}

implement<T: Ord> Ord for Option<T> {
    func cmp(&self, other: &Option<T>) -> Ordering {
        match (self, other) {
            (Option.Some(a), Option.Some(b)) => a.cmp(b)
            (Option.None, Option.None) => Ordering.Equal
            (Option.None, Option.Some(_)) => Ordering.Less
            (Option.Some(_), Option.None) => Ordering.Greater
        }
    }
}

implement<T> Try for Option<T> {
    type Output = T
    type Residual = Option<Infallible>

    func branch(self) -> ControlFlow<Option<Infallible>, T> {
        match self {
            Option.Some(v) => ControlFlow.Continue(v)
            Option.None => ControlFlow.Break(Option.None)
        }
    }
}

implement<T> FromResidual<Option<Infallible>> for Option<T> {
    func fromResidual(_residual: Option<Infallible>) -> Option<T> {
        Option.None
    }
}
