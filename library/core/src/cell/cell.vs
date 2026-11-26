//! Shareable mutable containers (`core/cell`).

package cell

import { Copy } from "core/marker"
import { Default } from "core/default"
import { read as ptrRead, write as ptrWrite } from "core/ptr"

/// The core primitive for interior mutability in Vertex.
@lang("UnsafeCell")
public struct UnsafeCell<T> {
    value: T
}

implement<T> UnsafeCell<T> {
    public func new(value: T) -> UnsafeCell<T> {
        UnsafeCell { value }
    }

    public func intoInner(self) -> T {
        self.value
    }

    public func get(&self) -> *mut T {
        &self.value as *const T as *mut T
    }

    public func getMut(&mut self) -> &mut T {
        &mut self.value
    }
}

implement<T: Default> Default for UnsafeCell<T> {
    func default() -> UnsafeCell<T> {
        UnsafeCell.new(T.default())
    }
}

/// A mutable memory location.
public struct Cell<T> {
    value: UnsafeCell<T>
}

implement<T> Cell<T> {
    public func new(value: T) -> Cell<T> {
        Cell { value: UnsafeCell.new(value) }
    }

    public func set(&self, val: T) {
        unsafe { ptrWrite(self.value.get(), val) }
    }

    public func replace(&self, val: T) -> T {
        unsafe {
            let p = self.value.get()
            let old = ptrRead(p as *const T)
            ptrWrite(p, val)
            old
        }
    }

    public func intoInner(self) -> T {
        self.value.intoInner()
    }

    public func asPtr(&self) -> *mut T {
        self.value.get()
    }

    public func getMut(&mut self) -> &mut T {
        self.value.getMut()
    }
}

implement<T: Copy> Cell<T> {
    public func get(&self) -> T {
        unsafe { ptrRead(self.value.get() as *const T) }
    }
}

implement<T: Default> Cell<T> {
    public func take(&self) -> T {
        self.replace(T.default())
    }
}

implement<T: Default> Default for Cell<T> {
    func default() -> Cell<T> {
        Cell.new(T.default())
    }
}
