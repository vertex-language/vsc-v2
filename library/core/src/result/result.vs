//! Error handling with the `Result` type (`core/result`).

package result

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Default } from "core/default"
import { Ordering, PartialEq, Eq, PartialOrd, Ord } from "core/cmp"
import { Infallible, From } from "core/convert"
import { ControlFlow, FromResidual, Try, FnOnce } from "core/ops"
import { Option } from "core/option"
import { panicMsg } from "core/panicking"

/// `Result` is a type that represents either success (`Ok`) or failure (`Err`).
public enum Result<T, E> {
    Ok(T)
    Err(E)
}

implement<T, E> Result<T, E> {
    public func isOk(&self) -> bool {
        match self {
            Result.Ok(_) => true
            Result.Err(_) => false
        }
    }

    public func isErr(&self) -> bool {
        !self.isOk()
    }

    public func isOkAnd<F: FnOnce(T) -> bool>(self, f: F) -> bool {
        match self {
            Result.Ok(x) => f(x)
            Result.Err(_) => false
        }
    }

    public func isErrAnd<F: FnOnce(E) -> bool>(self, f: F) -> bool {
        match self {
            Result.Ok(_) => false
            Result.Err(e) => f(e)
        }
    }

    public func ok(self) -> Option<T> {
        match self {
            Result.Ok(x) => Option.Some(x)
            Result.Err(_) => Option.None
        }
    }

    public func err(self) -> Option<E> {
        match self {
            Result.Ok(_) => Option.None
            Result.Err(e) => Option.Some(e)
        }
    }

    public func asRef(&self) -> Result<&T, &E> {
        match self {
            Result.Ok(x) => Result.Ok(x)
            Result.Err(e) => Result.Err(e)
        }
    }

    public func asMut(&mut self) -> Result<&mut T, &mut E> {
        match self {
            Result.Ok(x) => Result.Ok(x)
            Result.Err(e) => Result.Err(e)
        }
    }

    public func map<U, F: FnOnce(T) -> U>(self, op: F) -> Result<U, E> {
        match self {
            Result.Ok(t) => Result.Ok(op(t))
            Result.Err(e) => Result.Err(e)
        }
    }

    public func mapOr<U, F: FnOnce(T) -> U>(self, default: U, f: F) -> U {
        match self {
            Result.Ok(t) => f(t)
            Result.Err(_) => default
        }
    }

    public func mapOrElse<U, D: FnOnce(E) -> U, F: FnOnce(T) -> U>(self, default: D, f: F) -> U {
        match self {
            Result.Ok(t) => f(t)
            Result.Err(e) => default(e)
        }
    }

    public func mapErr<F, O: FnOnce(E) -> F>(self, op: O) -> Result<T, F> {
        match self {
            Result.Ok(t) => Result.Ok(t)
            Result.Err(e) => Result.Err(op(e))
        }
    }

    public func and<U>(self, res: Result<U, E>) -> Result<U, E> {
        match self {
            Result.Ok(_) => res
            Result.Err(e) => Result.Err(e)
        }
    }

    public func andThen<U, F: FnOnce(T) -> Result<U, E>>(self, op: F) -> Result<U, E> {
        match self {
            Result.Ok(t) => op(t)
            Result.Err(e) => Result.Err(e)
        }
    }

    public func or<F>(self, res: Result<T, F>) -> Result<T, F> {
        match self {
            Result.Ok(v) => Result.Ok(v)
            Result.Err(_) => res
        }
    }

    public func orElse<F, O: FnOnce(E) -> Result<T, F>>(self, op: O) -> Result<T, F> {
        match self {
            Result.Ok(t) => Result.Ok(t)
            Result.Err(e) => op(e)
        }
    }

    public func unwrap(self) -> T {
        match self {
            Result.Ok(t) => t
            Result.Err(_) => panicMsg("called `Result.unwrap()` on an `Err` value")
        }
    }

    public func expect(self, msg: &string) -> T {
        match self {
            Result.Ok(t) => t
            Result.Err(_) => panicMsg(msg)
        }
    }

    public func unwrapErr(self) -> E {
        match self {
            Result.Ok(_) => panicMsg("called `Result.unwrapErr()` on an `Ok` value")
            Result.Err(e) => e
        }
    }

    public func unwrapOr(self, default: T) -> T {
        match self {
            Result.Ok(t) => t
            Result.Err(_) => default
        }
    }

    public func unwrapOrElse<F: FnOnce(E) -> T>(self, op: F) -> T {
        match self {
            Result.Ok(t) => t
            Result.Err(e) => op(e)
        }
    }
}

implement<T: Default, E> Result<T, E> {
    public func unwrapOrDefault(self) -> T {
        match self {
            Result.Ok(t) => t
            Result.Err(_) => T.default()
        }
    }
}

implement<T: Copy, E: Copy> Copy for Result<T, E> {}

implement<T: Clone, E: Clone> Clone for Result<T, E> {
    func clone(&self) -> Result<T, E> {
        match self {
            Result.Ok(x) => Result.Ok((*x).clone())
            Result.Err(e) => Result.Err((*e).clone())
        }
    }
}

implement<T: PartialEq<T>, E: PartialEq<E>> PartialEq<Result<T, E>> for Result<T, E> {
    func eq(&self, other: &Result<T, E>) -> bool {
        match (self, other) {
            (Result.Ok(a), Result.Ok(b)) => (*a).eq(b)
            (Result.Err(a), Result.Err(b)) => (*a).eq(b)
            _ => false
        }
    }
}

implement<T: Eq, E: Eq> Eq for Result<T, E> {}

implement<T: PartialOrd<T>, E: PartialOrd<E>> PartialOrd<Result<T, E>> for Result<T, E> {
    func partialCmp(&self, other: &Result<T, E>) -> Option<Ordering> {
        match (self, other) {
            (Result.Ok(a), Result.Ok(b)) => a.partialCmp(b)
            (Result.Err(a), Result.Err(b)) => a.partialCmp(b)
            (Result.Ok(_), Result.Err(_)) => Option.Some(Ordering.Less)
            (Result.Err(_), Result.Ok(_)) => Option.Some(Ordering.Greater)
        }
    }
}

implement<T: Ord, E: Ord> Ord for Result<T, E> {
    func cmp(&self, other: &Result<T, E>) -> Ordering {
        match (self, other) {
            (Result.Ok(a), Result.Ok(b)) => a.cmp(b)
            (Result.Err(a), Result.Err(b)) => a.cmp(b)
            (Result.Ok(_), Result.Err(_)) => Ordering.Less
            (Result.Err(_), Result.Ok(_)) => Ordering.Greater
        }
    }
}

implement<T, E> Try for Result<T, E> {
    type Output = T
    type Residual = Result<Infallible, E>

    func branch(self) -> ControlFlow<Result<Infallible, E>, T> {
        match self {
            Result.Ok(v) => ControlFlow.Continue(v)
            Result.Err(e) => ControlFlow.Break(Result.Err(e))
        }
    }
}

implement<T, E, F: From<E>> FromResidual<Result<Infallible, E>> for Result<T, F> {
    func fromResidual(residual: Result<Infallible, E>) -> Result<T, F> {
        if let Result.Err(e) = residual {
            Result.Err(F.from(e))
        } else {
            loop {}
        }
    }
}
