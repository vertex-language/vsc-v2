//! Asynchronous callable traits (`core/ops`).

package ops

import { Future } from "core/future"

/// An async-aware version of `FnOnce`.
@lang("asyncFnOnce")
public trait AsyncFnOnce<Args> {
    type CallOnceFuture: Future<Output = Self.Output>
    type Output

    func asyncCallOnce(self, args: Args) -> Self.CallOnceFuture
}

/// An async-aware version of `FnMut`.
@lang("asyncFnMut")
public trait AsyncFnMut<Args>: AsyncFnOnce<Args> {
    type CallRefFuture: Future<Output = <Self as AsyncFnOnce<Args>>.Output>

    func asyncCallMut(&mut self, args: Args) -> Self.CallRefFuture
}

/// An async-aware version of `Fn`.
@lang("asyncFn")
public trait AsyncFn<Args>: AsyncFnMut<Args> {
    func asyncCall(&self, args: Args) -> <Self as AsyncFnMut<Args>>.CallRefFuture
}
