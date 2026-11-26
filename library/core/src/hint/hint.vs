//! Hints to the compiler that affect how code should be emitted or optimized (`core/hint`).

package hint

import { blackBox as intrinsicBlackBox, spinLoop as intrinsicSpinLoop, unreachableHint } from "core/intrinsics"

/// Signals the processor that it is running in a busy-wait spin-loop.
public func spinLoop() {
    intrinsicSpinLoop()
}

/// An identity function that hints to the compiler to be maximally pessimistic
/// about what `blackBox` could do.
public func blackBox<T>(dummy: T) -> T {
    intrinsicBlackBox(dummy)
}

/// Informs the optimizer that this code path is never reached.
public unsafe func unreachableUnchecked() -> never {
    unsafe { unreachableHint() }
}
