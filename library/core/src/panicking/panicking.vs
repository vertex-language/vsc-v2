//! Panic support for `core` (`core/panicking`).

package panicking

import { FormatArguments } from "core/fmt"
import { Option } from "core/option"

/// The kind of assertion that failed.
@lang("assertKind")
public enum AssertKind {
    Eq
    Ne
}

/// The entry point for panicking with a static message.
@lang("panic")
public func panicMsg(msg: &string) -> never {
    loop {}
}

/// The entry point for panicking with formatted arguments.
@lang("panicFmt")
public func panicFmt(args: FormatArguments) -> never {
    loop {}
}

/// Internal function for `assertEq` and `assertNe` failures.
@lang("assertFailed")
public func assertFailed<T, U>(
    kind: AssertKind,
    left: &T,
    right: &U,
    msg: Option<FormatArguments>
) -> never {
    loop {}
}
