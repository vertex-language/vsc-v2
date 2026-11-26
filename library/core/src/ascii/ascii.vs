//! Operations on ASCII strings and characters (`core/ascii`).

package ascii

/// Checks if the byte is within the ASCII range (`0x00..=0x7F`).
public func isAscii(b: uint8) -> bool {
    b <= 127
}

/// Checks if the byte is an ASCII alphabetic character (`A-Z`, `a-z`).
public func isAsciiAlphabetic(b: uint8) -> bool {
    isAsciiUppercase(b) || isAsciiLowercase(b)
}

/// Checks if the byte is an ASCII uppercase character (`A-Z`).
public func isAsciiUppercase(b: uint8) -> bool {
    b >= 65 && b <= 90
}

/// Checks if the byte is an ASCII lowercase character (`a-z`).
public func isAsciiLowercase(b: uint8) -> bool {
    b >= 97 && b <= 122
}

/// Checks if the byte is an ASCII decimal digit (`0-9`).
public func isAsciiDigit(b: uint8) -> bool {
    b >= 48 && b <= 57
}

/// Checks if the byte is an ASCII alphanumeric character (`0-9`, `A-Z`, `a-z`).
public func isAsciiAlphanumeric(b: uint8) -> bool {
    isAsciiAlphabetic(b) || isAsciiDigit(b)
}

/// Checks if the byte is an ASCII hexadecimal digit (`0-9`, `A-F`, `a-f`).
public func isAsciiHexdigit(b: uint8) -> bool {
    isAsciiDigit(b) || (b >= 65 && b <= 70) || (b >= 97 && b <= 102)
}

/// Checks if the byte is an ASCII whitespace character (`' '`, `'\t'`, `'\n'`, `'\x0C'`, `'\r'`).
public func isAsciiWhitespace(b: uint8) -> bool {
    b == 32 || b == 9 || b == 10 || b == 12 || b == 13
}

/// Checks if the byte is an ASCII control character (`0x00..=0x1F` or `0x7F`).
public func isAsciiControl(b: uint8) -> bool {
    b <= 31 || b == 127
}

/// Converts an ASCII lowercase letter to its uppercase equivalent.
public func toAsciiUppercase(b: uint8) -> uint8 {
    if isAsciiLowercase(b) { b - 32 } else { b }
}

/// Converts an ASCII uppercase letter to its lowercase equivalent.
public func toAsciiLowercase(b: uint8) -> uint8 {
    if isAsciiUppercase(b) { b + 32 } else { b }
}

/// Checks that two bytes are an ASCII case-insensitive match.
public func eqIgnoreAsciiCase(a: uint8, b: uint8) -> bool {
    toAsciiLowercase(a) == toAsciiLowercase(b)
}
