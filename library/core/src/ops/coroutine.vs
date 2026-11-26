//! Coroutine traits (`core/ops`).

package ops

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Pin } from "core/pin"

/// The result of a coroutine resumption.
@lang("coroutineState")
public enum CoroutineState<Y, R> {
    Yielded(Y)
    Complete(R)
}

implement<Y: Copy, R: Copy> Copy for CoroutineState<Y, R> {}

implement<Y: Clone, R: Clone> Clone for CoroutineState<Y, R> {
    func clone(&self) -> CoroutineState<Y, R> {
        match self {
            CoroutineState.Yielded(y) => CoroutineState.Yielded((*y).clone())
            CoroutineState.Complete(r) => CoroutineState.Complete((*r).clone())
        }
    }
}

/// The trait implemented by compiler-generated coroutines.
@lang("coroutine")
public trait Coroutine<R> {
    @lang("coroutineYield")
    type Yield

    @lang("coroutineReturn")
    type Return

    @lang("coroutineResume")
    func resume(self: Pin<&mut Self>, arg: R) -> CoroutineState<Self.Yield, Self.Return>
}
