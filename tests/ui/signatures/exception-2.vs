//@ check-pass
//@ compile-flags: -Z dump-signatures --target x86_64-unknown-linux-gnu
// Exception 2: `string` is `String`, `&string` is `&str`, `&mut string` is
// `&mut String`; `cstring` works the same way, and the C types are the
// target's.

package main

@lang("String")
struct String { len: uint }

@lang("CString")
struct CString { len: uint }

@lang("CStr")
struct CStr { len: uint }

func owned(s: string) -> string { s }
func borrowed(s: &string) -> &string { s }
func grow(s: &mut string) {}
func c(s: &cstring, owned: cstring) {}
func ctypes(a: c_char, b: c_int, d: c_long, e: c_ulonglong, f: c_double) -> never { loop {} }
func ints(a: int, b: uint, c: int8, d: uint128, e: float32) {}
