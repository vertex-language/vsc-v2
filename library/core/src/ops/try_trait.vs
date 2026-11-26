//! The `?` operator traits (`core/ops`).

package ops

/// Used to specify which residuals can be converted into a `Try` type.
public trait FromResidual<R> {
    @lang("fromResidual")
    func fromResidual(residual: R) -> Self
}

/// The `?` operator and `try` blocks.
public trait Try {
    type Output
    type Residual
    @lang("branch")
    func branch(self) -> ControlFlow<Self.Residual, Self.Output>
}
