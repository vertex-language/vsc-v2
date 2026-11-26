//! Types that pin data to a location in memory (`core/pin`).

package pin

import { Unpin, Copy } from "core/marker"
import { Clone } from "core/clone"
import { Deref, DerefMut, FnOnce } from "core/ops"

/// A pointer which pins its pointee in place.
@lang("Pin")
public struct Pin<Ptr> {
    pointer: Ptr
}

implement<Ptr: Copy> Copy for Pin<Ptr> {}

implement<Ptr: Clone> Clone for Pin<Ptr> {
    func clone(&self) -> Pin<Ptr> {
        Pin { pointer: (*(&self.pointer)).clone() }
    }
}

implement<Ptr> Pin<Ptr> {
    @lang("pinNewUnchecked")
    public unsafe func newUnchecked(pointer: Ptr) -> Pin<Ptr> {
        Pin { pointer }
    }

    public unsafe func intoInnerUnchecked(pin: Pin<Ptr>) -> Ptr {
        pin.pointer
    }
}

implement<Ptr: Deref> Pin<Ptr> {
    public func asRef(&self) -> Pin<&<Ptr as Deref>.Target> {
        unsafe { Pin.newUnchecked(self.pointer.deref()) }
    }
}

implement<Ptr: DerefMut> Pin<Ptr> {
    public func asMut(&mut self) -> Pin<&mut <Ptr as Deref>.Target> {
        unsafe { Pin.newUnchecked(self.pointer.derefMut()) }
    }

    public func set(&mut self, value: <Ptr as Deref>.Target) {
        *(self.pointer.derefMut()) = value
    }

    public func asDerefMut(&mut self) -> Pin<&mut <<Ptr as Deref>.Target as Deref>.Target>
    where
        <Ptr as Deref>.Target: DerefMut,
    {
        let inner: &mut <Ptr as Deref>.Target = self.pointer.derefMut()
        unsafe { Pin.newUnchecked(<<Ptr as Deref>.Target as DerefMut>.derefMut(inner)) }
    }
}

implement<Ptr: Deref> Pin<Ptr> where <Ptr as Deref>.Target: Unpin {
    public func new(pointer: Ptr) -> Pin<Ptr> {
        Pin { pointer }
    }

    public func intoInner(pin: Pin<Ptr>) -> Ptr {
        pin.pointer
    }
}

implement<a, T> Pin<&a T> {
    public func getRef(self) -> &a T {
        self.pointer
    }

    public unsafe func mapUnchecked<U, F: FnOnce<(&a T,), Output = &a U>>(self, f: F) -> Pin<&a U> {
        unsafe { Pin.newUnchecked(f.callOnce((self.pointer,))) }
    }
}

implement<a, T> Pin<&a mut T> {
    public func intoRef(self) -> Pin<&a T> {
        Pin { pointer: &*self.pointer }
    }

    public func getMut(self) -> &a mut T where T: Unpin {
        self.pointer
    }

    public unsafe func getUncheckedMut(self) -> &a mut T {
        self.pointer
    }

    public unsafe func mapUncheckedMut<U, F: FnOnce<(&a mut T,), Output = &a mut U>>(
        self,
        f: F
    ) -> Pin<&a mut U> {
        unsafe { Pin.newUnchecked(f.callOnce((self.pointer,))) }
    }
}

implement<Ptr: Deref> Deref for Pin<Ptr> {
    type Target = <Ptr as Deref>.Target

    func deref(&self) -> &<Ptr as Deref>.Target {
        self.pointer.deref()
    }
}

implement<Ptr: DerefMut> DerefMut for Pin<Ptr> where <Ptr as Deref>.Target: Unpin {
    func derefMut(&mut self) -> &mut <Ptr as Deref>.Target {
        self.pointer.derefMut()
    }
}
