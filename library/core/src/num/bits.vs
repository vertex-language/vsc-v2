//! Bit manipulation utilities backed by hardware intrinsics (`core/num`).

package num

import {
    ctpopU8, ctpopU16, ctpopU32, ctpopU64, ctpopUsize,
    ctlzU8, ctlzU16, ctlzU32, ctlzU64, ctlzUsize,
    cttzU8, cttzU16, cttzU32, cttzU64, cttzUsize,
    bswapU16, bswapU32, bswapU64,
    bitreverseU8, bitreverseU16, bitreverseU32, bitreverseU64,
    rotateLeftU32, rotateLeftU64,
    rotateRightU32, rotateRightU64
} from "core/intrinsics"

public func countOnesU8(x: uint8) -> uint32 { ctpopU8(x) }
public func countOnesU16(x: uint16) -> uint32 { ctpopU16(x) }
public func countOnesU32(x: uint32) -> uint32 { ctpopU32(x) }
public func countOnesU64(x: uint64) -> uint32 { ctpopU64(x) }
public func countOnesUint(x: uint) -> uint32 { ctpopUsize(x) }

public func leadingZerosU8(x: uint8) -> uint32 { ctlzU8(x) }
public func leadingZerosU16(x: uint16) -> uint32 { ctlzU16(x) }
public func leadingZerosU32(x: uint32) -> uint32 { ctlzU32(x) }
public func leadingZerosU64(x: uint64) -> uint32 { ctlzU64(x) }
public func leadingZerosUint(x: uint) -> uint32 { ctlzUsize(x) }

public func trailingZerosU8(x: uint8) -> uint32 { cttzU8(x) }
public func trailingZerosU16(x: uint16) -> uint32 { cttzU16(x) }
public func trailingZerosU32(x: uint32) -> uint32 { cttzU32(x) }
public func trailingZerosU64(x: uint64) -> uint32 { cttzU64(x) }
public func trailingZerosUint(x: uint) -> uint32 { cttzUsize(x) }

public func swapBytesU16(x: uint16) -> uint16 { bswapU16(x) }
public func swapBytesU32(x: uint32) -> uint32 { bswapU32(x) }
public func swapBytesU64(x: uint64) -> uint64 { bswapU64(x) }

public func reverseBitsU8(x: uint8) -> uint8 { bitreverseU8(x) }
public func reverseBitsU16(x: uint16) -> uint16 { bitreverseU16(x) }
public func reverseBitsU32(x: uint32) -> uint32 { bitreverseU32(x) }
public func reverseBitsU64(x: uint64) -> uint64 { bitreverseU64(x) }

public func rotateLeftU32Val(x: uint32, n: uint32) -> uint32 { rotateLeftU32(x, n) }
public func rotateLeftU64Val(x: uint64, n: uint32) -> uint64 { rotateLeftU64(x, n) }
public func rotateRightU32Val(x: uint32, n: uint32) -> uint32 { rotateRightU32(x, n) }
public func rotateRightU64Val(x: uint64, n: uint32) -> uint64 { rotateRightU64(x, n) }

/// Returns `true` if `x` is an integer power of two.
public func isPowerOfTwoU32(x: uint32) -> bool {
    x != 0 && (x & (x - 1)) == 0
}

/// Returns `true` if `x` is an integer power of two.
public func isPowerOfTwoU64(x: uint64) -> bool {
    x != 0 && (x & (x - 1)) == 0
}

/// Returns `true` if `x` is an integer power of two.
public func isPowerOfTwoUint(x: uint) -> bool {
    x != 0 && (x & (x - 1)) == 0
}
