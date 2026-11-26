//! Atomic types and memory orderings (`core/sync/atomic`).

package atomic

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Default } from "core/default"
import { Result } from "core/result"
import { UnsafeCell } from "core/cell"
import { panicMsg } from "core/panicking"
import {
    atomicFenceAcquire, atomicFenceRelease, atomicFenceAcqRel, atomicFenceSeqCst,
    atomicSingleThreadFenceAcquire, atomicSingleThreadFenceRelease,
    atomicSingleThreadFenceAcqRel, atomicSingleThreadFenceSeqCst,
    atomicLoadRelaxed, atomicLoadAcquire, atomicLoadSeqCst,
    atomicStoreRelaxed, atomicStoreRelease, atomicStoreSeqCst,
    atomicXchgRelaxed, atomicXchgAcquire, atomicXchgRelease, atomicXchgAcqRel, atomicXchgSeqCst,
    atomicXaddRelaxed, atomicXaddAcquire, atomicXaddRelease, atomicXaddAcqRel, atomicXaddSeqCst,
    atomicXsubRelaxed, atomicXsubAcquire, atomicXsubRelease, atomicXsubAcqRel, atomicXsubSeqCst,
    atomicAndRelaxed, atomicAndAcqRel, atomicAndSeqCst,
    atomicOrRelaxed, atomicOrAcqRel, atomicOrSeqCst,
    atomicXorRelaxed, atomicXorAcqRel, atomicXorSeqCst,
    atomicCxchgRelaxed, atomicCxchgAcquire, atomicCxchgRelease, atomicCxchgAcqRel, atomicCxchgSeqCst
} from "core/intrinsics"

/// Atomic memory orderings.
public enum Ordering {
    Relaxed
    Release
    Acquire
    AcqRel
    SeqCst
}

implement Copy for Ordering {}

implement Clone for Ordering {
    func clone(&self) -> Ordering { *self }
}

/// An atomic fence.
public func fence(order: Ordering) {
    match order {
        Ordering.Acquire => atomicFenceAcquire()
        Ordering.Release => atomicFenceRelease()
        Ordering.AcqRel => atomicFenceAcqRel()
        Ordering.SeqCst => atomicFenceSeqCst()
        Ordering.Relaxed => panicMsg("there is no such thing as a relaxed fence")
    }
}

/// A compiler memory fence.
public func compilerFence(order: Ordering) {
    match order {
        Ordering.Acquire => atomicSingleThreadFenceAcquire()
        Ordering.Release => atomicSingleThreadFenceRelease()
        Ordering.AcqRel => atomicSingleThreadFenceAcqRel()
        Ordering.SeqCst => atomicSingleThreadFenceSeqCst()
        Ordering.Relaxed => panicMsg("there is no such thing as a relaxed compiler fence")
    }
}

unsafe func atomicLoad<T>(ptr: *const T, order: Ordering) -> T {
    unsafe {
        match order {
            Ordering.Relaxed => atomicLoadRelaxed(ptr)
            Ordering.Acquire => atomicLoadAcquire(ptr)
            Ordering.SeqCst => atomicLoadSeqCst(ptr)
            Ordering.Release => panicMsg("there is no such thing as a release load")
            Ordering.AcqRel => panicMsg("there is no such thing as an acquire-release load")
        }
    }
}

unsafe func atomicStore<T>(ptr: *mut T, val: T, order: Ordering) {
    unsafe {
        match order {
            Ordering.Relaxed => atomicStoreRelaxed(ptr, val)
            Ordering.Release => atomicStoreRelease(ptr, val)
            Ordering.SeqCst => atomicStoreSeqCst(ptr, val)
            Ordering.Acquire => panicMsg("there is no such thing as an acquire store")
            Ordering.AcqRel => panicMsg("there is no such thing as an acquire-release store")
        }
    }
}

unsafe func atomicSwap<T>(ptr: *mut T, val: T, order: Ordering) -> T {
    unsafe {
        match order {
            Ordering.Relaxed => atomicXchgRelaxed(ptr, val)
            Ordering.Acquire => atomicXchgAcquire(ptr, val)
            Ordering.Release => atomicXchgRelease(ptr, val)
            Ordering.AcqRel => atomicXchgAcqRel(ptr, val)
            Ordering.SeqCst => atomicXchgSeqCst(ptr, val)
        }
    }
}

unsafe func atomicAdd<T>(ptr: *mut T, val: T, order: Ordering) -> T {
    unsafe {
        match order {
            Ordering.Relaxed => atomicXaddRelaxed(ptr, val)
            Ordering.Acquire => atomicXaddAcquire(ptr, val)
            Ordering.Release => atomicXaddRelease(ptr, val)
            Ordering.AcqRel => atomicXaddAcqRel(ptr, val)
            Ordering.SeqCst => atomicXaddSeqCst(ptr, val)
        }
    }
}

unsafe func atomicSub<T>(ptr: *mut T, val: T, order: Ordering) -> T {
    unsafe {
        match order {
            Ordering.Relaxed => atomicXsubRelaxed(ptr, val)
            Ordering.Acquire => atomicXsubAcquire(ptr, val)
            Ordering.Release => atomicXsubRelease(ptr, val)
            Ordering.AcqRel => atomicXsubAcqRel(ptr, val)
            Ordering.SeqCst => atomicXsubSeqCst(ptr, val)
        }
    }
}

unsafe func atomicAnd<T>(ptr: *mut T, val: T, order: Ordering) -> T {
    unsafe {
        match order {
            Ordering.Relaxed => atomicAndRelaxed(ptr, val)
            Ordering.AcqRel | Ordering.Acquire | Ordering.Release => atomicAndAcqRel(ptr, val)
            Ordering.SeqCst => atomicAndSeqCst(ptr, val)
        }
    }
}

unsafe func atomicOr<T>(ptr: *mut T, val: T, order: Ordering) -> T {
    unsafe {
        match order {
            Ordering.Relaxed => atomicOrRelaxed(ptr, val)
            Ordering.AcqRel | Ordering.Acquire | Ordering.Release => atomicOrAcqRel(ptr, val)
            Ordering.SeqCst => atomicOrSeqCst(ptr, val)
        }
    }
}

unsafe func atomicXor<T>(ptr: *mut T, val: T, order: Ordering) -> T {
    unsafe {
        match order {
            Ordering.Relaxed => atomicXorRelaxed(ptr, val)
            Ordering.AcqRel | Ordering.Acquire | Ordering.Release => atomicXorAcqRel(ptr, val)
            Ordering.SeqCst => atomicXorSeqCst(ptr, val)
        }
    }
}

unsafe func atomicCompareExchange<T>(
    ptr: *mut T,
    current: T,
    newVal: T,
    success: Ordering
) -> Result<T, T> {
    let pair = unsafe {
        match success {
            Ordering.Relaxed => atomicCxchgRelaxed(ptr, current, newVal)
            Ordering.Acquire => atomicCxchgAcquire(ptr, current, newVal)
            Ordering.Release => atomicCxchgRelease(ptr, current, newVal)
            Ordering.AcqRel => atomicCxchgAcqRel(ptr, current, newVal)
            Ordering.SeqCst => atomicCxchgSeqCst(ptr, current, newVal)
        }
    }
    if pair.1 {
        Result.Ok(pair.0)
    } else {
        Result.Err(pair.0)
    }
}

/// A boolean type which can be safely shared between threads.
public struct AtomicBool {
    v: UnsafeCell<uint8>
}

implement AtomicBool {
    public func new(v: bool) -> AtomicBool {
        AtomicBool { v: UnsafeCell.new(if v { 1 } else { 0 }) }
    }

    public func intoInner(self) -> bool {
        self.v.intoInner() != 0
    }

    public func load(&self, order: Ordering) -> bool {
        unsafe { atomicLoad(self.v.get() as *const uint8, order) != 0 }
    }

    public func store(&self, val: bool, order: Ordering) {
        unsafe { atomicStore(self.v.get(), if val { 1 } else { 0 }, order) }
    }

    public func swap(&self, val: bool, order: Ordering) -> bool {
        unsafe { atomicSwap(self.v.get(), if val { 1 } else { 0 }, order) != 0 }
    }

    public func compareExchange(
        &self,
        current: bool,
        newVal: bool,
        success: Ordering,
        _failure: Ordering
    ) -> Result<bool, bool> {
        let cur: uint8 = if current { 1 } else { 0 }
        let nxt: uint8 = if newVal { 1 } else { 0 }
        match unsafe { atomicCompareExchange(self.v.get(), cur, nxt, success) } {
            Result.Ok(prev) => Result.Ok(prev != 0)
            Result.Err(prev) => Result.Err(prev != 0)
        }
    }
}

implement Default for AtomicBool {
    func default() -> AtomicBool { AtomicBool.new(false) }
}

/// An `int32` type which can be safely shared between threads.
public struct AtomicInt32 {
    v: UnsafeCell<int32>
}

implement AtomicInt32 {
    public func new(v: int32) -> AtomicInt32 { AtomicInt32 { v: UnsafeCell.new(v) } }
    public func intoInner(self) -> int32 { self.v.intoInner() }
    public func getMut(&mut self) -> &mut int32 { self.v.getMut() }
    public func load(&self, order: Ordering) -> int32 { unsafe { atomicLoad(self.v.get() as *const int32, order) } }
    public func store(&self, val: int32, order: Ordering) { unsafe { atomicStore(self.v.get(), val, order) } }
    public func swap(&self, val: int32, order: Ordering) -> int32 { unsafe { atomicSwap(self.v.get(), val, order) } }
    public func fetchAdd(&self, val: int32, order: Ordering) -> int32 { unsafe { atomicAdd(self.v.get(), val, order) } }
    public func fetchSub(&self, val: int32, order: Ordering) -> int32 { unsafe { atomicSub(self.v.get(), val, order) } }
    public func fetchAnd(&self, val: int32, order: Ordering) -> int32 { unsafe { atomicAnd(self.v.get(), val, order) } }
    public func fetchOr(&self, val: int32, order: Ordering) -> int32 { unsafe { atomicOr(self.v.get(), val, order) } }
    public func fetchXor(&self, val: int32, order: Ordering) -> int32 { unsafe { atomicXor(self.v.get(), val, order) } }
    public func compareExchange(&self, current: int32, newVal: int32, success: Ordering, _failure: Ordering) -> Result<int32, int32> {
        unsafe { atomicCompareExchange(self.v.get(), current, newVal, success) }
    }
}

implement Default for AtomicInt32 {
    func default() -> AtomicInt32 { AtomicInt32.new(0) }
}

/// An `int64` type which can be safely shared between threads.
public struct AtomicInt64 {
    v: UnsafeCell<int64>
}

implement AtomicInt64 {
    public func new(v: int64) -> AtomicInt64 { AtomicInt64 { v: UnsafeCell.new(v) } }
    public func intoInner(self) -> int64 { self.v.intoInner() }
    public func getMut(&mut self) -> &mut int64 { self.v.getMut() }
    public func load(&self, order: Ordering) -> int64 { unsafe { atomicLoad(self.v.get() as *const int64, order) } }
    public func store(&self, val: int64, order: Ordering) { unsafe { atomicStore(self.v.get(), val, order) } }
    public func swap(&self, val: int64, order: Ordering) -> int64 { unsafe { atomicSwap(self.v.get(), val, order) } }
    public func fetchAdd(&self, val: int64, order: Ordering) -> int64 { unsafe { atomicAdd(self.v.get(), val, order) } }
    public func fetchSub(&self, val: int64, order: Ordering) -> int64 { unsafe { atomicSub(self.v.get(), val, order) } }
    public func fetchAnd(&self, val: int64, order: Ordering) -> int64 { unsafe { atomicAnd(self.v.get(), val, order) } }
    public func fetchOr(&self, val: int64, order: Ordering) -> int64 { unsafe { atomicOr(self.v.get(), val, order) } }
    public func fetchXor(&self, val: int64, order: Ordering) -> int64 { unsafe { atomicXor(self.v.get(), val, order) } }
    public func compareExchange(&self, current: int64, newVal: int64, success: Ordering, _failure: Ordering) -> Result<int64, int64> {
        unsafe { atomicCompareExchange(self.v.get(), current, newVal, success) }
    }
}

implement Default for AtomicInt64 {
    func default() -> AtomicInt64 { AtomicInt64.new(0) }
}

/// A `uint32` type which can be safely shared between threads.
public struct AtomicUint32 {
    v: UnsafeCell<uint32>
}

implement AtomicUint32 {
    public func new(v: uint32) -> AtomicUint32 { AtomicUint32 { v: UnsafeCell.new(v) } }
    public func intoInner(self) -> uint32 { self.v.intoInner() }
    public func getMut(&mut self) -> &mut uint32 { self.v.getMut() }
    public func load(&self, order: Ordering) -> uint32 { unsafe { atomicLoad(self.v.get() as *const uint32, order) } }
    public func store(&self, val: uint32, order: Ordering) { unsafe { atomicStore(self.v.get(), val, order) } }
    public func swap(&self, val: uint32, order: Ordering) -> uint32 { unsafe { atomicSwap(self.v.get(), val, order) } }
    public func fetchAdd(&self, val: uint32, order: Ordering) -> uint32 { unsafe { atomicAdd(self.v.get(), val, order) } }
    public func fetchSub(&self, val: uint32, order: Ordering) -> uint32 { unsafe { atomicSub(self.v.get(), val, order) } }
    public func fetchAnd(&self, val: uint32, order: Ordering) -> uint32 { unsafe { atomicAnd(self.v.get(), val, order) } }
    public func fetchOr(&self, val: uint32, order: Ordering) -> uint32 { unsafe { atomicOr(self.v.get(), val, order) } }
    public func fetchXor(&self, val: uint32, order: Ordering) -> uint32 { unsafe { atomicXor(self.v.get(), val, order) } }
    public func compareExchange(&self, current: uint32, newVal: uint32, success: Ordering, _failure: Ordering) -> Result<uint32, uint32> {
        unsafe { atomicCompareExchange(self.v.get(), current, newVal, success) }
    }
}

implement Default for AtomicUint32 {
    func default() -> AtomicUint32 { AtomicUint32.new(0) }
}

/// A `uint64` type which can be safely shared between threads.
public struct AtomicUint64 {
    v: UnsafeCell<uint64>
}

implement AtomicUint64 {
    public func new(v: uint64) -> AtomicUint64 { AtomicUint64 { v: UnsafeCell.new(v) } }
    public func intoInner(self) -> uint64 { self.v.intoInner() }
    public func getMut(&mut self) -> &mut uint64 { self.v.getMut() }
    public func load(&self, order: Ordering) -> uint64 { unsafe { atomicLoad(self.v.get() as *const uint64, order) } }
    public func store(&self, val: uint64, order: Ordering) { unsafe { atomicStore(self.v.get(), val, order) } }
    public func swap(&self, val: uint64, order: Ordering) -> uint64 { unsafe { atomicSwap(self.v.get(), val, order) } }
    public func fetchAdd(&self, val: uint64, order: Ordering) -> uint64 { unsafe { atomicAdd(self.v.get(), val, order) } }
    public func fetchSub(&self, val: uint64, order: Ordering) -> uint64 { unsafe { atomicSub(self.v.get(), val, order) } }
    public func fetchAnd(&self, val: uint64, order: Ordering) -> uint64 { unsafe { atomicAnd(self.v.get(), val, order) } }
    public func fetchOr(&self, val: uint64, order: Ordering) -> uint64 { unsafe { atomicOr(self.v.get(), val, order) } }
    public func fetchXor(&self, val: uint64, order: Ordering) -> uint64 { unsafe { atomicXor(self.v.get(), val, order) } }
    public func compareExchange(&self, current: uint64, newVal: uint64, success: Ordering, _failure: Ordering) -> Result<uint64, uint64> {
        unsafe { atomicCompareExchange(self.v.get(), current, newVal, success) }
    }
}

implement Default for AtomicUint64 {
    func default() -> AtomicUint64 { AtomicUint64.new(0) }
}

/// A `uint` (pointer-sized unsigned integer) type which can be safely shared between threads.
public struct AtomicUint {
    v: UnsafeCell<uint>
}

implement AtomicUint {
    public func new(v: uint) -> AtomicUint { AtomicUint { v: UnsafeCell.new(v) } }
    public func intoInner(self) -> uint { self.v.intoInner() }
    public func getMut(&mut self) -> &mut uint { self.v.getMut() }
    public func load(&self, order: Ordering) -> uint { unsafe { atomicLoad(self.v.get() as *const uint, order) } }
    public func store(&self, val: uint, order: Ordering) { unsafe { atomicStore(self.v.get(), val, order) } }
    public func swap(&self, val: uint, order: Ordering) -> uint { unsafe { atomicSwap(self.v.get(), val, order) } }
    public func fetchAdd(&self, val: uint, order: Ordering) -> uint { unsafe { atomicAdd(self.v.get(), val, order) } }
    public func fetchSub(&self, val: uint, order: Ordering) -> uint { unsafe { atomicSub(self.v.get(), val, order) } }
    public func fetchAnd(&self, val: uint, order: Ordering) -> uint { unsafe { atomicAnd(self.v.get(), val, order) } }
    public func fetchOr(&self, val: uint, order: Ordering) -> uint { unsafe { atomicOr(self.v.get(), val, order) } }
    public func fetchXor(&self, val: uint, order: Ordering) -> uint { unsafe { atomicXor(self.v.get(), val, order) } }
    public func compareExchange(&self, current: uint, newVal: uint, success: Ordering, _failure: Ordering) -> Result<uint, uint> {
        unsafe { atomicCompareExchange(self.v.get(), current, newVal, success) }
    }
}

implement Default for AtomicUint {
    func default() -> AtomicUint { AtomicUint.new(0) }
}
