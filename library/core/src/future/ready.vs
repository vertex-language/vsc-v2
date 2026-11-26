//! Immediately-ready future (`core/future`).

package future

import { Pin } from "core/pin"
import { Context, Poll } from "core/task"
import { Option } from "core/option"
import { Unpin } from "core/marker"
import { panicMsg } from "core/panicking"

/// A future that is immediately ready with a value.
public struct Ready<T> {
    val: Option<T>
}

implement<T> Unpin for Ready<T> {}

implement<T> Future for Ready<T> {
    type Output = T

    func poll(var self: Pin<&mut Self>, _cx: &mut Context) -> Poll<T> {
        let this = unsafe { self.getUncheckedMut() }
        match this.val.take() {
            Option.Some(v) => Poll.Ready(v)
            Option.None => panicMsg("`Ready` polled after completion")
        }
    }
}

/// Creates a future that is immediately ready with `t`.
public func ready<T>(t: T) -> Ready<T> {
    Ready { val: Option.Some(t) }
}
