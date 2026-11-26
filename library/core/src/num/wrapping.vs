//! Wrapping numeric wrapper (`core/num`).

package num

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Default } from "core/default"
import { Ordering, PartialEq, Eq, PartialOrd, Ord } from "core/cmp"
import { Option } from "core/option"

/// Provides intentionally-wrapped arithmetic on `T`.
public struct Wrapping<T> {
    public value: T
}

implement<T> Wrapping<T> {
    public func new(value: T) -> Wrapping<T> {
        Wrapping { value }
    }

    public func intoInner(self) -> T {
        self.value
    }
}

implement<T: Copy> Copy for Wrapping<T> {}

implement<T: Clone> Clone for Wrapping<T> {
    func clone(&self) -> Wrapping<T> {
        Wrapping { value: (*(&self.value)).clone() }
    }
}

implement<T: Default> Default for Wrapping<T> {
    func default() -> Wrapping<T> {
        Wrapping { value: T.default() }
    }
}

implement<T: PartialEq<T>> PartialEq<Wrapping<T>> for Wrapping<T> {
    func eq(&self, other: &Wrapping<T>) -> bool {
        self.value.eq(&other.value)
    }
}

implement<T: Eq> Eq for Wrapping<T> {}

implement<T: PartialOrd<T>> PartialOrd<Wrapping<T>> for Wrapping<T> {
    func partialCmp(&self, other: &Wrapping<T>) -> Option<Ordering> {
        self.value.partialCmp(&other.value)
    }
}

implement<T: Ord> Ord for Wrapping<T> {
    func cmp(&self, other: &Wrapping<T>) -> Ordering {
        self.value.cmp(&other.value)
    }
}
