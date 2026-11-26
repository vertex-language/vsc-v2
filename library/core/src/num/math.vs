//! Floating-point math utilities backed by hardware intrinsics (`core/num`).

package num

import {
    sqrtf32, sqrtf64,
    fabsf32, fabsf64,
    floorf32, floorf64,
    ceilf32, ceilf64,
    truncf32, truncf64,
    roundevenf32, roundevenf64
} from "core/intrinsics"

public func sqrtF32(x: float32) -> float32 { sqrtf32(x) }
public func sqrtF64(x: float64) -> float64 { sqrtf64(x) }

public func absF32(x: float32) -> float32 { fabsf32(x) }
public func absF64(x: float64) -> float64 { fabsf64(x) }

public func floorF32(x: float32) -> float32 { floorf32(x) }
public func floorF64(x: float64) -> float64 { floorf64(x) }

public func ceilF32(x: float32) -> float32 { ceilf32(x) }
public func ceilF64(x: float64) -> float64 { ceilf64(x) }

public func truncF32(x: float32) -> float32 { truncf32(x) }
public func truncF64(x: float64) -> float64 { truncf64(x) }

public func roundEvenF32(x: float32) -> float32 { roundevenf32(x) }
public func roundEvenF64(x: float64) -> float64 { roundevenf64(x) }
