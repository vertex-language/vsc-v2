//! Utilities related to FFI bindings (`core/ffi`).

package ffi

/// Equivalent to C's `void` type when used as a pointer.
@lang("c_void")
public enum CVoid {
    Variant1
    Variant2
}

/// Representation of a borrowed C string.
@lang("CStr")
public struct CStr {
    len: uint
}
