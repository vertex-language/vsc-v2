//! Boolean utilities (`core/bool`).

package bool

import { Option } from "core/option"
import { FnOnce } from "core/ops"

/// Returns `Option.Some(t)` if `cond` is `true`, or `Option.None` otherwise.
public func thenSome<T>(cond: bool, t: T) -> Option<T> {
    if cond { Option.Some(t) } else { Option.None }
}

/// Returns `Option.Some(f())` if `cond` is `true`, or `Option.None` otherwise.
public func then<T, F: FnOnce() -> T>(cond: bool, f: F) -> Option<T> {
    if cond { Option.Some(f()) } else { Option.None }
}
