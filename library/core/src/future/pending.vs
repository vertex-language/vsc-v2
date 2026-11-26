//! Never-ready future (`core/future`).

package future

import { Pin } from "core/pin"
import { Context, Poll } from "core/task"
import { PhantomData, Unpin } from "core/marker"

/// A future that never resolves.
public struct Pending<T> {
    _marker: PhantomData<T>
}

implement<T> Unpin for Pending<T> {}

implement<T> Future for Pending<T> {
    type Output = T

    func poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<T> {
        Poll.Pending
    }
}

/// Creates a future that never resolves.
public func pending<T>() -> Pending<T> {
    Pending { _marker: PhantomData }
}
