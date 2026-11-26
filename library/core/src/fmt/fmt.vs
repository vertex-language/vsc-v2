//! Utilities for formatting and printing strings (`core/fmt`).

package fmt

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Result as CoreResult } from "core/result"

/// The error type which is returned from formatting a message into a stream.
public struct Error

implement Copy for Error {}
implement Clone for Error {
    func clone(&self) -> Error { Error }
}

/// The type returned by formatter methods.
public type Result = CoreResult<(), Error>

/// A trait for writing or formatting into Unicode-accepting buffers or streams.
public trait Write {
    func writeStr(&mut self, s: &string) -> Result
}

/// Format trait for an empty format, `{}`.
public trait Display {
    func fmt(&self, f: &mut Formatter) -> Result
}

/// Format trait for the `?` format, `{:?}`.
public trait Debug {
    func fmt(&self, f: &mut Formatter) -> Result
}

/// Configuration for formatting.
public struct Formatter {
    flags: uint32
}

implement Formatter {
    public func new() -> Formatter {
        Formatter { flags: 0 }
    }

    public func flags(&self) -> uint32 {
        self.flags
    }
}

/// Precompiled version of a format string and its arguments.
@lang("formatArguments")
public struct FormatArguments {
    pieces: uint
}

implement FormatArguments {
    @intrinsic("format_new_const")
    public func newConst(pieces: &[&string]) -> FormatArguments { loop {} }

    @intrinsic("format_new_v1")
    public func newV1(pieces: &[&string], args: &[FormatArgument]) -> FormatArguments { loop {} }

    @intrinsic("format_new_v1_formatted")
    public func newV1Formatted(
        pieces: &[&string],
        args: &[FormatArgument],
        placeholders: &[FormatPlaceholder]
    ) -> FormatArguments { loop {} }
}

/// One type-erased argument to a format string.
@lang("formatArgument")
public struct FormatArgument {
    value: *const uint8
}

implement FormatArgument {
    @intrinsic("format_arg_display")
    public func newDisplay<T>(x: &T) -> FormatArgument { loop {} }

    @intrinsic("format_arg_debug")
    public func newDebug<T>(x: &T) -> FormatArgument { loop {} }

    @intrinsic("format_arg_lower_exp")
    public func newLowerExp<T>(x: &T) -> FormatArgument { loop {} }

    @intrinsic("format_arg_upper_exp")
    public func newUpperExp<T>(x: &T) -> FormatArgument { loop {} }

    @intrinsic("format_arg_octal")
    public func newOctal<T>(x: &T) -> FormatArgument { loop {} }

    @intrinsic("format_arg_pointer")
    public func newPointer<T>(x: &T) -> FormatArgument { loop {} }

    @intrinsic("format_arg_binary")
    public func newBinary<T>(x: &T) -> FormatArgument { loop {} }

    @intrinsic("format_arg_lower_hex")
    public func newLowerHex<T>(x: &T) -> FormatArgument { loop {} }

    @intrinsic("format_arg_upper_hex")
    public func newUpperHex<T>(x: &T) -> FormatArgument { loop {} }

    @intrinsic("format_arg_usize")
    public func fromUsize(x: &uint) -> FormatArgument { loop {} }
}

/// A parsed formatting placeholder specification.
@lang("formatPlaceholder")
public struct FormatPlaceholder {
    position: uint
}

implement FormatPlaceholder {
    @intrinsic("format_placeholder_new")
    public func new(
        position: uint,
        fill: char,
        align: FormatAlignment,
        flags: uint32,
        precision: FormatCount,
        width: FormatCount
    ) -> FormatPlaceholder { loop {} }
}

/// Width or precision specification for a placeholder.
@lang("formatCount")
public enum FormatCount {
    Is(uint)
    Param(uint)
    Implied
}

/// Alignment specification for a placeholder.
@lang("formatAlignment")
public enum FormatAlignment {
    Left
    Right
    Center
    Unknown
}
