//! Saturating numeric wrapper (`core/num`).

package num

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Default } from "core/default"
import { Ordering, PartialEq, Eq, PartialOrd, Ord } from "core/cmp"
import { Option } from "core/option"

/// Provides intentionally-saturating arithmetic on `T`.
public struct Saturating<T> {
    public value: T
}

implement<T> Saturating<T> {
    public func new(value: T) -> Saturating<T> {
        Saturating { value }
    }

    public func intoInner(self) -> T {
        self.value
    }
}

implement<T: Copy> Copy for Saturating<T> {}

implement<T: Clone> Clone for Saturating<T> {
    func clone(&self) -> Saturating<T> {
        Saturating { value: (*(&self.value)).clone() }
    }
}

implement<T: Default> Default for Saturating<T> {
    func default() -> Saturating<T> {
        Saturating { value: T.default() }
    }
}

implement<T: PartialEq<T>> PartialEq<Saturating<T>> for Saturating<T> {
    func eq(&self, other: &Saturating<T>) -> bool {
        self.value.eq(&other.value)
    }
}

implement<T: Eq> Eq for Saturating<T> {}

implement<T: PartialOrd<T>> PartialOrd<Saturating<T>> for Saturating<T> {
    func partialCmp(&self, other: &Saturating<T>) -> Option<Ordering> {
        self.value.partialCmp(&other.value)
    }
}

implement<T: Ord> Ord for Saturating<T> {
    func cmp(&self, other: &Saturating<T>) -> Ordering {
        self.value.cmp(&other.value)
    }
}
