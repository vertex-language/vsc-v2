//! Utilities for the `char` primitive type (`core/char`).

package char

import { Option } from "core/option"

/// The highest valid Unicode scalar value (`U+10FFFF`).
public const MAX: uint32 = 1114111

/// The Unicode replacement character (`U+FFFD`).
public const REPLACEMENT_CHARACTER: uint32 = 65533

/// Checks if a `uint32` is a valid Unicode scalar value.
public func isValidScalar(v: uint32) -> bool {
    v <= MAX && !(v >= 55296 && v <= 57343)
}

/// Returns the number of bytes this `char` would need if encoded in UTF-8.
public func lenUtf8(c: char) -> uint {
    let code = c as uint32
    if code < 128 {
        1
    } else if code < 2048 {
        2
    } else if code < 65536 {
        3
    } else {
        4
    }
}

/// Returns the number of 16-bit code units this `char` would need in UTF-16.
public func lenUtf16(c: char) -> uint {
    if (c as uint32) < 65536 { 1 } else { 2 }
}

/// Checks if the value is within the ASCII range.
public func isAscii(c: char) -> bool {
    (c as uint32) <= 127
}

/// Checks if the value is an ASCII decimal digit (`'0'..='9'`).
public func isAsciiDigit(c: char) -> bool {
    let v = c as uint32
    v >= 48 && v <= 57
}

/// Checks if the value is an ASCII alphabetic character.
public func isAsciiAlphabetic(c: char) -> bool {
    let v = c as uint32
    (v >= 65 && v <= 90) || (v >= 97 && v <= 122)
}

/// Checks if the value is an ASCII whitespace character.
public func isAsciiWhitespace(c: char) -> bool {
    let v = c as uint32
    v == 32 || v == 9 || v == 10 || v == 12 || v == 13
}

/// Converts a digit in the given radix (2..=36) to a `uint32`.
public func toDigit(c: char, radix: uint32) -> Option<uint32> {
    if radix < 2 || radix > 36 {
        return Option.None
    }
    let v = c as uint32
    let d: uint32 = if v >= 48 && v <= 57 {
        v - 48
    } else if v >= 65 && v <= 90 {
        v - 65 + 10
    } else if v >= 97 && v <= 122 {
        v - 97 + 10
    } else {
        return Option.None
    }
    if d < radix {
        Option.Some(d)
    } else {
        Option.None
    }
}
