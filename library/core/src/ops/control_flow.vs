//! Control flow enum used by `?` and short-circuiting operations (`core/ops`).

package ops

import { Option } from "core/option"

/// Used to tell an operation whether it should exit early or go on as usual.
public enum ControlFlow<B, C> {
    @lang("Continue")
    Continue(C)
    @lang("Break")
    Break(B)
}

implement<B, C> ControlFlow<B, C> {
    public func isBreak(&self) -> bool {
        match self {
            ControlFlow.Break(_) => true
            ControlFlow.Continue(_) => false
        }
    }

    public func isContinue(&self) -> bool {
        !self.isBreak()
    }

    public func breakValue(self) -> Option<B> {
        match self {
            ControlFlow.Break(b) => Option.Some(b)
            ControlFlow.Continue(_) => Option.None
        }
    }

    public func continueValue(self) -> Option<C> {
        match self {
            ControlFlow.Continue(c) => Option.Some(c)
            ControlFlow.Break(_) => Option.None
        }
    }

    public func mapBreak<T, F: FnOnce(B) -> T>(self, f: F) -> ControlFlow<T, C> {
        match self {
            ControlFlow.Break(b) => ControlFlow.Break(f(b))
            ControlFlow.Continue(c) => ControlFlow.Continue(c)
        }
    }

    public func mapContinue<T, F: FnOnce(C) -> T>(self, f: F) -> ControlFlow<B, T> {
        match self {
            ControlFlow.Continue(c) => ControlFlow.Continue(f(c))
            ControlFlow.Break(b) => ControlFlow.Break(b)
        }
    }
}
