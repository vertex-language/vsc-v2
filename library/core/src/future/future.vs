//! Asynchronous values (`core/future`).

package future

import { Pin } from "core/pin"
import { Context, Poll } from "core/task"
import { Unpin, Copy, Send, Sync } from "core/marker"
import { Clone } from "core/clone"
import { Deref, DerefMut } from "core/ops"
import { NonNull } from "core/ptr"

/// A future represents an asynchronous computation.
@lang("future")
public trait Future {
    /// The type of value produced on completion.
    @lang("futureOutput")
    type Output

    /// Attempts to resolve the future to a final value, registering
    /// the current task for wakeup if the value is not yet available.
    @lang("poll")
    func poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self.Output>
}

implement<a, F: Future + Unpin> Future for &a mut F {
    type Output = F.Output

    func poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<F.Output> {
        let this = unsafe { self.getUncheckedMut() }
        F.poll(Pin.new(&mut **this), cx)
    }
}

implement<P: DerefMut> Future for Pin<P> where <P as Deref>.Target: Future {
    type Output = <<P as Deref>.Target as Future>.Output

    func poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<<<P as Deref>.Target as Future>.Output> {
        let this = unsafe { self.getUncheckedMut() }
        <<P as Deref>.Target as Future>.poll(this.asMut(), cx)
    }
}

/// Compiler-internal carrier for `&mut Context` across coroutine yield points.
@lang("ResumeTy")
public struct ResumeTy {
    cx: NonNull<Context<static>>
}

implement Copy for ResumeTy {}

implement Clone for ResumeTy {
    func clone(&self) -> ResumeTy { *self }
}

unsafe implement Send for ResumeTy {}
unsafe implement Sync for ResumeTy {}

/// Extracts `&mut Context` from a `ResumeTy` inside desugared `.await` loops.
@lang("getContext")
public unsafe func getContext<a, b>(cx: ResumeTy) -> &a mut Context<b> {
    unsafe { &mut *(cx.cx.asPtr() as *mut Context<b>) }
}
