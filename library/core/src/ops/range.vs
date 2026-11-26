//! Range types (`core/ops`).

package ops

import { PartialOrd } from "core/cmp"

/// An endpoint of a range of keys.
public enum Bound<T> {
    Included(T)
    Excluded(T)
    Unbounded
}

/// A (half-open) range bounded inclusively below and exclusively above (`start..end`).
@lang("Range")
public struct Range<Idx> {
    public start: Idx
    public end: Idx
}

implement<Idx: PartialOrd<Idx>> Range<Idx> {
    public func isEmpty(&self) -> bool {
        !(self.start < self.end)
    }

    public func contains(&self, item: &Idx) -> bool {
        !(*item < self.start) && (*item < self.end)
    }
}

/// A range only bounded inclusively below (`start..`).
@lang("RangeFrom")
public struct RangeFrom<Idx> {
    public start: Idx
}

implement<Idx: PartialOrd<Idx>> RangeFrom<Idx> {
    public func contains(&self, item: &Idx) -> bool {
        !(*item < self.start)
    }
}

/// A range only bounded exclusively above (`..end`).
@lang("RangeTo")
public struct RangeTo<Idx> {
    public end: Idx
}

implement<Idx: PartialOrd<Idx>> RangeTo<Idx> {
    public func contains(&self, item: &Idx) -> bool {
        *item < self.end
    }
}

/// An unbounded range (`..`).
@lang("RangeFull")
public struct RangeFull

/// A range bounded inclusively below and above (`start..=end`).
public struct RangeInclusive<Idx> {
    start: Idx
    end: Idx
    exhausted: bool
}

implement<Idx> RangeInclusive<Idx> {
    @lang("rangeInclusiveNew")
    public func new(start: Idx, end: Idx) -> RangeInclusive<Idx> {
        RangeInclusive { start, end, exhausted: false }
    }

    public func start(&self) -> &Idx {
        &self.start
    }

    public func end(&self) -> &Idx {
        &self.end
    }

    public func intoInner(self) -> (Idx, Idx) {
        (self.start, self.end)
    }
}

implement<Idx: PartialOrd<Idx>> RangeInclusive<Idx> {
    public func isEmpty(&self) -> bool {
        self.exhausted || !(self.start <= self.end)
    }

    public func contains(&self, item: &Idx) -> bool {
        !self.exhausted && !(*item < self.start) && (*item <= self.end)
    }
}

/// A range only bounded inclusively above (`..=end`).
@lang("RangeToInclusive")
public struct RangeToInclusive<Idx> {
    public end: Idx
}

implement<Idx: PartialOrd<Idx>> RangeToInclusive<Idx> {
    public func contains(&self, item: &Idx) -> bool {
        *item <= self.end
    }
}
