//! Asynchronous polling state (`core/task`).

package task

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Result } from "core/result"
import { FnOnce, ControlFlow, Try, FromResidual } from "core/ops"

/// Indicates whether a value is available or if the current task has been
/// scheduled to receive a wakeup instead.
@lang("Poll")
public enum Poll<T> {
    @lang("Ready")
    Ready(T)
    @lang("Pending")
    Pending
}

implement<T: Copy> Copy for Poll<T> {}

implement<T: Clone> Clone for Poll<T> {
    func clone(&self) -> Poll<T> {
        match self {
            Poll.Ready(v) => Poll.Ready((*v).clone())
            Poll.Pending => Poll.Pending
        }
    }
}

implement<T> Poll<T> {
    public func isReady(&self) -> bool {
        match self {
            Poll.Ready(_) => true
            Poll.Pending => false
        }
    }

    public func isPending(&self) -> bool {
        !self.isReady()
    }

    public func map<U, F: FnOnce<(T,), Output = U>>(self, f: F) -> Poll<U> {
        match self {
            Poll.Ready(v) => Poll.Ready(f.callOnce((v,)))
            Poll.Pending => Poll.Pending
        }
    }
}

implement<T, E> Poll<Result<T, E>> {
    public func mapOk<U, F: FnOnce<(T,), Output = U>>(self, f: F) -> Poll<Result<U, E>> {
        match self {
            Poll.Ready(Result.Ok(v)) => Poll.Ready(Result.Ok(f.callOnce((v,))))
            Poll.Ready(Result.Err(e)) => Poll.Ready(Result.Err(e))
            Poll.Pending => Poll.Pending
        }
    }

    public func mapErr<U, F: FnOnce<(E,), Output = U>>(self, f: F) -> Poll<Result<T, U>> {
        match self {
            Poll.Ready(Result.Ok(v)) => Poll.Ready(Result.Ok(v))
            Poll.Ready(Result.Err(e)) => Poll.Ready(Result.Err(f.callOnce((e,))))
            Poll.Pending => Poll.Pending
        }
    }
}

implement<T> FromResidual<Poll<never>> for Poll<T> {
    func fromResidual(_r: Poll<never>) -> Poll<T> {
        Poll.Pending
    }
}

implement<T> Try for Poll<T> {
    type Output = T
    type Residual = Poll<never>

    func branch(self) -> ControlFlow<Poll<never>, T> {
        match self {
            Poll.Ready(v) => ControlFlow.Continue(v)
            Poll.Pending => ControlFlow.Break(Poll.Pending)
        }
    }
}
