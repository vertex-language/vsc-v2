//! Allocation-free stack-polled future combinators (`core/future`).

package future

import { Pin } from "core/pin"
import { Context, Poll } from "core/task"
import { Option } from "core/option"
import { Result } from "core/result"
import { Unpin } from "core/marker"
import { replace } from "core/mem"
import { panicMsg } from "core/panicking"

/// Tracks a future that may already have completed and produced its output.
public enum MaybeDone<F: Future> {
    Future(F)
    Done(F.Output)
    Gone
}

implement<F: Future> MaybeDone<F> {
    public func takeOutput(self: Pin<&mut Self>) -> F.Output {
        let this = unsafe { self.getUncheckedMut() }
        match replace(this, MaybeDone.Gone) {
            MaybeDone.Done(v) => v
            _ => panicMsg("`MaybeDone.takeOutput` called before completion")
        }
    }

    public func pollDone(self: Pin<&mut Self>, cx: &mut Context) -> bool {
        let this = unsafe { self.getUncheckedMut() }
        match this {
            MaybeDone.Future(f) => {
                let pinned = unsafe { Pin.newUnchecked(f) }
                match F.poll(pinned, cx) {
                    Poll.Ready(v) => {
                        *this = MaybeDone.Done(v)
                        true
                    }
                    Poll.Pending => false
                }
            }
            MaybeDone.Done(_) => true
            MaybeDone.Gone => panicMsg("`MaybeDone` polled after output taken")
        }
    }
}

/// Polls two futures concurrently until both complete, returning `(A.Output, B.Output)`.
public struct Join2<A: Future, B: Future> {
    a: MaybeDone<A>
    b: MaybeDone<B>
}

implement<A: Future, B: Future> Future for Join2<A, B> {
    type Output = (A.Output, B.Output)

    func poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<(A.Output, B.Output)> {
        let this = unsafe { self.getUncheckedMut() }
        let aDone = MaybeDone.pollDone(unsafe { Pin.newUnchecked(&mut this.a) }, &mut *cx)
        let bDone = MaybeDone.pollDone(unsafe { Pin.newUnchecked(&mut this.b) }, &mut *cx)
        if aDone && bDone {
            let aVal = MaybeDone.takeOutput(unsafe { Pin.newUnchecked(&mut this.a) })
            let bVal = MaybeDone.takeOutput(unsafe { Pin.newUnchecked(&mut this.b) })
            Poll.Ready((aVal, bVal))
        } else {
            Poll.Pending
        }
    }
}

/// Polls two futures concurrently on the stack with zero heap allocation.
public func join2<A: Future, B: Future>(a: A, b: B) -> Join2<A, B> {
    Join2 {
        a: MaybeDone.Future(a),
        b: MaybeDone.Future(b)
    }
}

/// Polls three futures concurrently until all three complete.
public struct Join3<A: Future, B: Future, C: Future> {
    a: MaybeDone<A>
    b: MaybeDone<B>
    c: MaybeDone<C>
}

implement<A: Future, B: Future, C: Future> Future for Join3<A, B, C> {
    type Output = (A.Output, B.Output, C.Output)

    func poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<(A.Output, B.Output, C.Output)> {
        let this = unsafe { self.getUncheckedMut() }
        let aDone = MaybeDone.pollDone(unsafe { Pin.newUnchecked(&mut this.a) }, &mut *cx)
        let bDone = MaybeDone.pollDone(unsafe { Pin.newUnchecked(&mut this.b) }, &mut *cx)
        let cDone = MaybeDone.pollDone(unsafe { Pin.newUnchecked(&mut this.c) }, &mut *cx)
        if aDone && bDone && cDone {
            let aVal = MaybeDone.takeOutput(unsafe { Pin.newUnchecked(&mut this.a) })
            let bVal = MaybeDone.takeOutput(unsafe { Pin.newUnchecked(&mut this.b) })
            let cVal = MaybeDone.takeOutput(unsafe { Pin.newUnchecked(&mut this.c) })
            Poll.Ready((aVal, bVal, cVal))
        } else {
            Poll.Pending
        }
    }
}

/// Polls three futures concurrently on the stack with zero heap allocation.
public func join3<A: Future, B: Future, C: Future>(a: A, b: B, c: C) -> Join3<A, B, C> {
    Join3 {
        a: MaybeDone.Future(a),
        b: MaybeDone.Future(b),
        c: MaybeDone.Future(c)
    }
}

/// Polls two fallible futures concurrently, short-circuiting on the first `Err`.
public struct TryJoin2<A: Future, B: Future> {
    a: MaybeDone<A>
    b: MaybeDone<B>
}

implement<T1, T2, E, A: Future<Output = Result<T1, E>>, B: Future<Output = Result<T2, E>>> Future for TryJoin2<A, B> {
    type Output = Result<(T1, T2), E>

    func poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(T1, T2), E>> {
        let this = unsafe { self.getUncheckedMut() }
        let aDone = MaybeDone.pollDone(unsafe { Pin.newUnchecked(&mut this.a) }, &mut *cx)
        if aDone {
            if let MaybeDone.Done(Result.Err(_)) = &this.a {
                match MaybeDone.takeOutput(unsafe { Pin.newUnchecked(&mut this.a) }) {
                    Result.Err(e) => return Poll.Ready(Result.Err(e))
                    Result.Ok(_) => {}
                }
            }
        }
        let bDone = MaybeDone.pollDone(unsafe { Pin.newUnchecked(&mut this.b) }, &mut *cx)
        if bDone {
            if let MaybeDone.Done(Result.Err(_)) = &this.b {
                match MaybeDone.takeOutput(unsafe { Pin.newUnchecked(&mut this.b) }) {
                    Result.Err(e) => return Poll.Ready(Result.Err(e))
                    Result.Ok(_) => {}
                }
            }
        }
        if aDone && bDone {
            let aVal = match MaybeDone.takeOutput(unsafe { Pin.newUnchecked(&mut this.a) }) {
                Result.Ok(v) => v
                Result.Err(e) => return Poll.Ready(Result.Err(e))
            }
            let bVal = match MaybeDone.takeOutput(unsafe { Pin.newUnchecked(&mut this.b) }) {
                Result.Ok(v) => v
                Result.Err(e) => return Poll.Ready(Result.Err(e))
            }
            Poll.Ready(Result.Ok((aVal, bVal)))
        } else {
            Poll.Pending
        }
    }
}

public func tryJoin2<T1, T2, E, A: Future<Output = Result<T1, E>>, B: Future<Output = Result<T2, E>>>(
    a: A,
    b: B
) -> TryJoin2<A, B> {
    TryJoin2 {
        a: MaybeDone.Future(a),
        b: MaybeDone.Future(b)
    }
}

/// Output of `select2`: whichever future completed first.
public enum Either<L, R> {
    Left(L)
    Right(R)
}

/// Polls two futures and resolves with the first one that completes.
public struct Select2<A: Future, B: Future> {
    a: Option<A>
    b: Option<B>
}

implement<A: Future, B: Future> Future for Select2<A, B> {
    type Output = Either<A.Output, B.Output>

    func poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Either<A.Output, B.Output>> {
        let this = unsafe { self.getUncheckedMut() }
        if let Option.Some(ref mut a) = this.a {
            if let Poll.Ready(v) = A.poll(unsafe { Pin.newUnchecked(a) }, &mut *cx) {
                this.a = Option.None
                this.b = Option.None
                return Poll.Ready(Either.Left(v))
            }
        }
        if let Option.Some(ref mut b) = this.b {
            if let Poll.Ready(v) = B.poll(unsafe { Pin.newUnchecked(b) }, &mut *cx) {
                this.a = Option.None
                this.b = Option.None
                return Poll.Ready(Either.Right(v))
            }
        }
        Poll.Pending
    }
}

/// Races two futures with potentially different output types.
public func select2<A: Future, B: Future>(a: A, b: B) -> Select2<A, B> {
    Select2 {
        a: Option.Some(a),
        b: Option.Some(b)
    }
}

/// Races two futures of the same output type, returning whichever completes first.
public struct Race2<T, A: Future<Output = T>, B: Future<Output = T>> {
    a: Option<A>
    b: Option<B>
}

implement<T, A: Future<Output = T>, B: Future<Output = T>> Future for Race2<T, A, B> {
    type Output = T

    func poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<T> {
        let this = unsafe { self.getUncheckedMut() }
        if let Option.Some(ref mut a) = this.a {
            if let Poll.Ready(v) = A.poll(unsafe { Pin.newUnchecked(a) }, &mut *cx) {
                this.a = Option.None
                this.b = Option.None
                return Poll.Ready(v)
            }
        }
        if let Option.Some(ref mut b) = this.b {
            if let Poll.Ready(v) = B.poll(unsafe { Pin.newUnchecked(b) }, &mut *cx) {
                this.a = Option.None
                this.b = Option.None
                return Poll.Ready(v)
            }
        }
        Poll.Pending
    }
}

public func race2<T, A: Future<Output = T>, B: Future<Output = T>>(a: A, b: B) -> Race2<T, A, B> {
    Race2 { a: Option.Some(a), b: Option.Some(b) }
}

/// A future that yields once back to the executor, then completes on the next poll.
public struct YieldNow {
    yielded: bool
}

implement Unpin for YieldNow {}

implement Future for YieldNow {
    type Output = ()

    func poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        let this = unsafe { self.getUncheckedMut() }
        if this.yielded {
            Poll.Ready(())
        } else {
            this.yielded = true
            cx.waker().wakeByRef()
            Poll.Pending
        }
    }
}

/// Cooperatively yields control back to the executor once.
public func yieldNow() -> YieldNow {
    YieldNow { yielded: false }
}
