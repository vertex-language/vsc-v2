//! Manually manage memory through raw pointers (`core/ptr`).

package ptr

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Option } from "core/option"
import {
    copy as intrinsicCopy,
    copyNonoverlapping as intrinsicCopyNonoverlapping,
    writeBytes as intrinsicWriteBytes,
    volatileLoad,
    volatileStore
} from "core/intrinsics"

/// Creates a null raw pointer.
public func null<T>() -> *const T {
    0 as uint as *const T
}

/// Creates a null mutable raw pointer.
public func nullMut<T>() -> *mut T {
    0 as uint as *mut T
}

/// Executes the destructor (if any) of the pointed-to value.
@lang("dropInPlace")
public unsafe func dropInPlace<T>(_toDrop: *mut T) {}

/// Reads the value from `src` without moving it.
public unsafe func read<T>(src: *const T) -> T {
    unsafe { volatileLoad(src) }
}

/// Overwrites a memory location with the given value without reading or
/// dropping the old value.
public unsafe func write<T>(dst: *mut T, src: T) {
    unsafe { volatileStore(dst, src) }
}

/// Performs a volatile read of the value from `src` without moving it.
public unsafe func readVolatile<T>(src: *const T) -> T {
    unsafe { volatileLoad(src) }
}

/// Performs a volatile write of a memory location with the given value.
public unsafe func writeVolatile<T>(dst: *mut T, src: T) {
    unsafe { volatileStore(dst, src) }
}

/// Copies `count * sizeOf<T>()` bytes from `src` to `dst`. The source and
/// destination may overlap.
public unsafe func copy<T>(src: *const T, dst: *mut T, count: uint) {
    unsafe { intrinsicCopy(src, dst, count) }
}

/// Copies `count * sizeOf<T>()` bytes from `src` to `dst`. The source and
/// destination must *not* overlap.
public unsafe func copyNonoverlapping<T>(src: *const T, dst: *mut T, count: uint) {
    unsafe { intrinsicCopyNonoverlapping(src, dst, count) }
}

/// Sets `count * sizeOf<T>()` bytes of memory starting at `dst` to `val`.
public unsafe func writeBytes<T>(dst: *mut T, val: uint8, count: uint) {
    unsafe { intrinsicWriteBytes(dst, val, count) }
}

/// Swaps the values at two mutable locations of the same type.
public unsafe func swap<T>(x: *mut T, y: *mut T) {
    unsafe {
        let tmp = read(x as *const T)
        write(x, read(y as *const T))
        write(y, tmp)
    }
}

/// Moves `src` into the pointed `dst`, returning the previous `dst` value.
public unsafe func replace<T>(dst: *mut T, src: T) -> T {
    unsafe {
        let prev = read(dst as *const T)
        write(dst, src)
        prev
    }
}

/// `*mut T` that is guaranteed to be non-null.
public struct NonNull<T> {
    pointer: *mut T
}

implement<T> Copy for NonNull<T> {}

implement<T> Clone for NonNull<T> {
    func clone(&self) -> NonNull<T> { *self }
}

implement<T> NonNull<T> {
    public unsafe func newUnchecked(ptr: *mut T) -> NonNull<T> {
        NonNull { pointer: ptr }
    }

    public func new(ptr: *mut T) -> Option<NonNull<T>> {
        if (ptr as uint) == 0 {
            Option.None
        } else {
            Option.Some(NonNull { pointer: ptr })
        }
    }

    public func asPtr(self) -> *mut T {
        self.pointer
    }

    public unsafe func asRef<a>(&self) -> &a T {
        unsafe { &*self.pointer }
    }

    public unsafe func asMut<a>(&mut self) -> &a mut T {
        unsafe { &mut *self.pointer }
    }
}
