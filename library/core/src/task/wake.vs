//! Task wakeup primitives (`core/task`).

package task

import { Copy, Send, Sync, PhantomData } from "core/marker"
import { Clone } from "core/clone"
import { Drop } from "core/ops"
import { forget } from "core/mem"
import { null } from "core/ptr"

/// A virtual function pointer table (vtable) that specifies the behavior
/// of a `RawWaker`.
public struct RawWakerVTable {
    clone: unsafe func(*const ()) -> RawWaker
    wake: unsafe func(*const ())
    wakeByRef: unsafe func(*const ())
    drop: unsafe func(*const ())
}

implement Copy for RawWakerVTable {}

implement Clone for RawWakerVTable {
    func clone(&self) -> RawWakerVTable { *self }
}

implement RawWakerVTable {
    public func new(
        clone: unsafe func(*const ()) -> RawWaker,
        wake: unsafe func(*const ()),
        wakeByRef: unsafe func(*const ()),
        drop: unsafe func(*const ())
    ) -> RawWakerVTable {
        RawWakerVTable { clone, wake, wakeByRef, drop }
    }
}

/// A type-erased waker handle consisting of a data pointer and a vtable.
public struct RawWaker {
    data: *const ()
    vtable: &static RawWakerVTable
}

implement RawWaker {
    public func new(data: *const (), vtable: &static RawWakerVTable) -> RawWaker {
        RawWaker { data, vtable }
    }

    public func data(&self) -> *const () {
        self.data
    }

    public func vtable(&self) -> &static RawWakerVTable {
        self.vtable
    }
}

unsafe func noopClone(_data: *const ()) -> RawWaker {
    RawWaker { data: null(), vtable: &NOOP_VTABLE }
}

unsafe func noopWake(_data: *const ()) {}
unsafe func noopWakeByRef(_data: *const ()) {}
unsafe func noopDrop(_data: *const ()) {}

static NOOP_VTABLE: RawWakerVTable = RawWakerVTable {
    clone: noopClone,
    wake: noopWake,
    wakeByRef: noopWakeByRef,
    drop: noopDrop
}

/// A handle for waking up a task by notifying its executor that it is ready to be run.
public struct Waker {
    waker: RawWaker
}

unsafe implement Send for Waker {}
unsafe implement Sync for Waker {}

implement Waker {
    public unsafe func fromRaw(waker: RawWaker) -> Waker {
        Waker { waker }
    }

    public func noop() -> Waker {
        Waker { waker: RawWaker { data: null(), vtable: &NOOP_VTABLE } }
    }

    public func wake(self) {
        let data = self.waker.data
        let wakeFn = self.waker.vtable.wake
        forget(self)
        unsafe { wakeFn(data) }
    }

    public func wakeByRef(&self) {
        unsafe { (self.waker.vtable.wakeByRef)(self.waker.data) }
    }

    public func willWake(&self, other: &Waker) -> bool {
        (self.waker.data as uint) == (other.waker.data as uint)
            && (self.waker.vtable as *const RawWakerVTable as uint)
                == (other.waker.vtable as *const RawWakerVTable as uint)
    }

    public func data(&self) -> *const () {
        self.waker.data
    }

    public func vtable(&self) -> &static RawWakerVTable {
        self.waker.vtable
    }
}

implement Clone for Waker {
    func clone(&self) -> Waker {
        Waker { waker: unsafe { (self.waker.vtable.clone)(self.waker.data) } }
    }
}

implement Drop for Waker {
    func drop(&mut self) {
        unsafe { (self.waker.vtable.drop)(self.waker.data) }
    }
}

/// A non-thread-safe waker for single-threaded executors (`LocalSet`).
public struct LocalWaker {
    waker: RawWaker
}

implement !Send for LocalWaker {}
implement !Sync for LocalWaker {}

implement LocalWaker {
    public unsafe func fromRaw(waker: RawWaker) -> LocalWaker {
        LocalWaker { waker }
    }

    public func noop() -> LocalWaker {
        LocalWaker { waker: RawWaker { data: null(), vtable: &NOOP_VTABLE } }
    }

    public func wake(self) {
        let data = self.waker.data
        let wakeFn = self.waker.vtable.wake
        forget(self)
        unsafe { wakeFn(data) }
    }

    public func wakeByRef(&self) {
        unsafe { (self.waker.vtable.wakeByRef)(self.waker.data) }
    }
}

implement Clone for LocalWaker {
    func clone(&self) -> LocalWaker {
        LocalWaker { waker: unsafe { (self.waker.vtable.clone)(self.waker.data) } }
    }
}

implement Drop for LocalWaker {
    func drop(&mut self) {
        unsafe { (self.waker.vtable.drop)(self.waker.data) }
    }
}

/// The context of an asynchronous task, carrying its `Waker`.
public struct Context<a> {
    waker: &a Waker
    _marker: PhantomData<&a Waker>
}

implement<a> Context<a> {
    public func fromWaker(waker: &a Waker) -> Context<a> {
        Context { waker, _marker: PhantomData }
    }

    public func waker(&self) -> &a Waker {
        self.waker
    }
}
