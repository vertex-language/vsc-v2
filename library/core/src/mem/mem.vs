//! Basic functions for dealing with memory (`core/mem`).

package mem

import { Default } from "core/default"
import { Deref, DerefMut } from "core/ops"
import {
    sizeOf as intrinsicSizeOf,
    alignOf as intrinsicAlignOf,
    needsDrop as intrinsicNeedsDrop,
    transmute as intrinsicTransmute
} from "core/intrinsics"
import { read as ptrRead, write as ptrWrite } from "core/ptr"

/// A wrapper to inhibit the compiler from automatically calling `T`'s destructor.
@lang("ManuallyDrop")
public struct ManuallyDrop<T> {
    value: T
}

implement<T> ManuallyDrop<T> {
    public func new(value: T) -> ManuallyDrop<T> {
        ManuallyDrop { value }
    }

    public func intoInner(slot: ManuallyDrop<T>) -> T {
        slot.value
    }

    public unsafe func take(slot: &mut ManuallyDrop<T>) -> T {
        unsafe { ptrRead(&slot.value as *const T) }
    }
}

implement<T> Deref for ManuallyDrop<T> {
    type Target = T
    func deref(&self) -> &T { &self.value }
}

implement<T> DerefMut for ManuallyDrop<T> {
    func derefMut(&mut self) -> &mut T { &mut self.value }
}

/// Returns the size of a type in bytes.
public func sizeOf<T>() -> uint {
    intrinsicSizeOf<T>()
}

/// Returns the ABI-required minimum alignment of a type in bytes.
public func alignOf<T>() -> uint {
    intrinsicAlignOf<T>()
}

/// Returns `true` if dropping values of type `T` matters.
public func needsDrop<T>() -> bool {
    intrinsicNeedsDrop<T>()
}

/// Reinterprets the bits of a value of one type as another type.
public unsafe func transmute<Src, Dst>(src: Src) -> Dst {
    unsafe { intrinsicTransmute<Src, Dst>(src) }
}

/// Disposes of a value.
public func drop<T>(_x: T) {}

/// Takes ownership and "forgets" about the value without running its destructor.
public func forget<T>(t: T) {
    let _ = ManuallyDrop.new(t)
}

/// Swaps the values at two mutable locations, without deinitializing either one.
public func swap<T>(x: &mut T, y: &mut T) {
    unsafe {
        let px = x as *mut T
        let py = y as *mut T
        let tmp = ptrRead(px as *const T)
        ptrWrite(px, ptrRead(py as *const T))
        ptrWrite(py, tmp)
    }
}

/// Moves `src` into the referenced `dest`, returning the previous `dest` value.
public func replace<T>(dest: &mut T, src: T) -> T {
    unsafe {
        let pd = dest as *mut T
        let prev = ptrRead(pd as *const T)
        ptrWrite(pd, src)
        prev
    }
}

/// Replaces `dest` with the default value of type `T`, returning the previous value.
public func take<T: Default>(dest: &mut T) -> T {
    replace(dest, T.default())
}
