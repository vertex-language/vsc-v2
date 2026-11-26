//! Utilities for the array primitive type `[T; N]` (`core/array`).

package array

import { sliceLen } from "core/intrinsics"

/// Returns the fixed length of an array.
public func len<T, const N: uint>(arr: &[T; N]) -> uint {
    sliceLen(arr)
}
