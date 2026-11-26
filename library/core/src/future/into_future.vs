//! Conversion into a `Future` (`core/future`).

package future

/// Conversion into a `Future`.
@lang("intoFuture")
public trait IntoFuture {
    type Output
    type IntoFuture: Future<Output = Self.Output>

    @lang("intoFutureMethod")
    func intoFuture(self) -> Self.IntoFuture
}

implement<F: Future> IntoFuture for F {
    type Output = F.Output
    type IntoFuture = F

    func intoFuture(self) -> F { self }
}
