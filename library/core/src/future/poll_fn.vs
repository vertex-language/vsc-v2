//! Function-backed future (`core/future`).

package future

import { Pin } from "core/pin"
import { Context, Poll } from "core/task"
import { Unpin } from "core/marker"
import { FnMut } from "core/ops"

/// A future that wraps a closure/function returning `Poll<T>`.
public struct PollFn<F> {
    f: F
}

implement<F: Unpin> Unpin for PollFn<F> {}

implement<a, b, T, F: FnMut<(&a mut Context<b>,), Output = Poll<T>>> Future for PollFn<F> {
    type Output = T

    func poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<T> {
        let this = unsafe { self.getUncheckedMut() }
        this.f.callMut((cx,))
    }
}

/// Creates a future that polls by calling `f(cx)`.
public func pollFn<a, b, T, F: FnMut<(&a mut Context<b>,), Output = Poll<T>>>(f: F) -> PollFn<F> {
    PollFn { f }
}
