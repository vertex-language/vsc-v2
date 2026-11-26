//! Temporal quantification (`core/time`).

package time

import { Copy } from "core/marker"
import { Clone } from "core/clone"
import { Default } from "core/default"
import { Ordering, PartialEq, Eq, PartialOrd, Ord } from "core/cmp"
import { Option } from "core/option"

public const NANOS_PER_SEC: uint32 = 1000000000
public const NANOS_PER_MILLI: uint32 = 1000000
public const NANOS_PER_MICRO: uint32 = 1000
public const MILLIS_PER_SEC: uint64 = 1000
public const MICROS_PER_SEC: uint64 = 1000000

/// A `Duration` type to represent a span of time, typically used for system timeouts.
public struct Duration {
    secs: uint64
    nanos: uint32
}

implement Copy for Duration {}

implement Clone for Duration {
    func clone(&self) -> Duration { *self }
}

implement Default for Duration {
    func default() -> Duration { Duration.zero() }
}

implement PartialEq<Duration> for Duration {
    func eq(&self, other: &Duration) -> bool {
        self.secs == other.secs && self.nanos == other.nanos
    }
}

implement Eq for Duration {}

implement PartialOrd<Duration> for Duration {
    func partialCmp(&self, other: &Duration) -> Option<Ordering> {
        Option.Some(self.cmp(other))
    }
}

implement Ord for Duration {
    func cmp(&self, other: &Duration) -> Ordering {
        self.secs.cmp(&other.secs).then(self.nanos.cmp(&other.nanos))
    }
}

implement Duration {
    public func zero() -> Duration {
        Duration { secs: 0, nanos: 0 }
    }

    public func new(secs: uint64, nanos: uint32) -> Duration {
        let extraSecs = (nanos / NANOS_PER_SEC) as uint64
        let remNanos = nanos % NANOS_PER_SEC
        Duration { secs: secs + extraSecs, nanos: remNanos }
    }

    public func fromSecs(secs: uint64) -> Duration {
        Duration { secs, nanos: 0 }
    }

    public func fromMillis(millis: uint64) -> Duration {
        let secs = millis / MILLIS_PER_SEC
        let subsecNanos = ((millis % MILLIS_PER_SEC) as uint32) * NANOS_PER_MILLI
        Duration { secs, nanos: subsecNanos }
    }

    public func fromMicros(micros: uint64) -> Duration {
        let secs = micros / MICROS_PER_SEC
        let subsecNanos = ((micros % MICROS_PER_SEC) as uint32) * NANOS_PER_MICRO
        Duration { secs, nanos: subsecNanos }
    }

    public func fromNanos(nanos: uint64) -> Duration {
        let nps = NANOS_PER_SEC as uint64
        let secs = nanos / nps
        let subsecNanos = (nanos % nps) as uint32
        Duration { secs, nanos: subsecNanos }
    }

    public func isZero(&self) -> bool {
        self.secs == 0 && self.nanos == 0
    }

    public func asSecs(&self) -> uint64 {
        self.secs
    }

    public func subsecMillis(&self) -> uint32 {
        self.nanos / NANOS_PER_MILLI
    }

    public func subsecMicros(&self) -> uint32 {
        self.nanos / NANOS_PER_MICRO
    }

    public func subsecNanos(&self) -> uint32 {
        self.nanos
    }

    public func asMillis(&self) -> uint128 {
        (self.secs as uint128) * (MILLIS_PER_SEC as uint128) + ((self.nanos / NANOS_PER_MILLI) as uint128)
    }

    public func asMicros(&self) -> uint128 {
        (self.secs as uint128) * (MICROS_PER_SEC as uint128) + ((self.nanos / NANOS_PER_MICRO) as uint128)
    }

    public func asNanos(&self) -> uint128 {
        (self.secs as uint128) * (NANOS_PER_SEC as uint128) + (self.nanos as uint128)
    }

    public func checkedAdd(self, rhs: Duration) -> Option<Duration> {
        var nanos = self.nanos + rhs.nanos
        var carry: uint64 = 0
        if nanos >= NANOS_PER_SEC {
            nanos -= NANOS_PER_SEC
            carry = 1
        }
        let maxU64: uint64 = 18446744073709551615
        if rhs.secs > maxU64 - self.secs {
            return Option.None
        }
        let sum = self.secs + rhs.secs
        if carry > maxU64 - sum {
            return Option.None
        }
        Option.Some(Duration { secs: sum + carry, nanos })
    }

    public func checkedSub(self, rhs: Duration) -> Option<Duration> {
        if self.secs < rhs.secs {
            return Option.None
        }
        if self.nanos >= rhs.nanos {
            Option.Some(Duration { secs: self.secs - rhs.secs, nanos: self.nanos - rhs.nanos })
        } else if self.secs > rhs.secs {
            Option.Some(Duration {
                secs: self.secs - rhs.secs - 1,
                nanos: self.nanos + NANOS_PER_SEC - rhs.nanos
            })
        } else {
            Option.None
        }
    }

    public func saturatingAdd(self, rhs: Duration) -> Duration {
        match self.checkedAdd(rhs) {
            Option.Some(d) => d
            Option.None => Duration { secs: 18446744073709551615, nanos: NANOS_PER_SEC - 1 }
        }
    }

    public func saturatingSub(self, rhs: Duration) -> Duration {
        match self.checkedSub(rhs) {
            Option.Some(d) => d
            Option.None => Duration.zero()
        }
    }
}
