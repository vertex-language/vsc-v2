//@ run-pass
//@ no-core-stubs

import { Option } from "core/option"
import { Iterator, empty, repeat } from "core/iter"
import { sizeOf, alignOf, needsDrop } from "core/mem"
import { NonZeroUint32, Wrapping, Saturating, countOnesU32, leadingZerosU32, trailingZerosU32, swapBytesU32, rotateLeftU32Val, isPowerOfTwoU32, sqrtF64, absF64, floorF64, ceilF64 } from "core/num"
import { isAsciiDigit, isAsciiAlphabetic, toAsciiUppercase, toAsciiLowercase, eqIgnoreAsciiCase } from "core/ascii"
import { Duration } from "core/time"
import { Hasher, Hash, FnvHasher } from "core/hash"
import { thenSome } from "core/bool"
import { lenUtf8, toDigit } from "core/char"
import { len as arrayLen } from "core/array"
import { len as sliceLen, first, last, get, iter as sliceIter } from "core/slice"
import { blackBox, spinLoop } from "core/hint"
import { RangeInclusive } from "core/ops"

func main() {
    // Range and iterator adapters
    var sum: int64 = 0
    for i in 1..=5 {
        sum += i
    }
    assertEq(sum, 15)

    let r0: int64 = 0
    let r10: int64 = 10
    var adapted: int64 = 0
    for x in (r0..r10).stepBy(2).take(4) {
        adapted += x * x
    }
    assertEq(adapted, 0 + 4 + 16 + 36)

    assertEq((r0..r10).stepBy(3).count(), 4)
    assertEq(empty<int32>().count(), 0)
    var repSum: int32 = 0
    let seven: int32 = 7
    for v in repeat(seven).take(5) {
        repSum += v
    }
    assertEq(repSum, 35)

    // Slice and array helpers
    let arr: [int64; 4] = [10, 20, 30, 40]
    assertEq(arrayLen(&arr), 4)
    assertEq(sliceLen(&arr), 4)
    assertEq(*first(&arr).unwrap(), 10)
    assertEq(*last(&arr).unwrap(), 40)
    assertEq(*get(&arr, 2).unwrap(), 30)
    assert(get(&arr, 9).isNone())

    var ssum: int64 = 0
    for x in &arr {
        ssum += *x
    }
    assertEq(ssum, 100)
    var midSum: int64 = 0
    for x in sliceIter(&arr).skip(1).take(2) {
        midSum += *x
    }
    assertEq(midSum, 50)

    // Memory layout & hints
    assertEq(sizeOf<int64>(), 8)
    assertEq(alignOf<int64>(), 8)
    assertEq(sizeOf<uint32>(), 4)
    assert(!needsDrop<int64>())
    assertEq(blackBox(42), 42)
    spinLoop()

    // Numbers & bit/math intrinsics
    assert(NonZeroUint32.new(0).isNone())
    assertEq(NonZeroUint32.new(9).unwrap().get(), 9)
    assertEq(Wrapping.new(5).intoInner(), 5)
    assertEq(Saturating.new(8).intoInner(), 8)
    assertEq(countOnesU32(0b10110100), 4)
    assertEq(leadingZerosU32(1), 31)
    assertEq(trailingZerosU32(16), 4)
    assertEq(swapBytesU32(0x12345678), 0x78563412)
    assertEq(rotateLeftU32Val(0x80000001, 1), 0x00000003)
    assert(isPowerOfTwoU32(64))
    assert(!isPowerOfTwoU32(63))
    assertEq(sqrtF64(9.0), 3.0)
    assertEq(absF64(-4.5), 4.5)
    assertEq(floorF64(3.7), 3.0)
    assertEq(ceilF64(3.2), 4.0)

    // ASCII, char, bool
    assert(isAsciiDigit(53))
    assert(isAsciiAlphabetic(97))
    assertEq(toAsciiUppercase(97), 65)
    assertEq(toAsciiLowercase(90), 122)
    assert(eqIgnoreAsciiCase(65, 97))
    assertEq(thenSome(true, 11).unwrap(), 11)
    assert(thenSome(false, 11).isNone())
    assertEq(lenUtf8('A'), 1)
    assertEq(toDigit('f', 16).unwrap(), 15)

    // Time Duration
    let d1 = Duration.fromMillis(1500)
    let d2 = Duration.fromMillis(750)
    let d3 = d1.checkedAdd(d2).unwrap()
    assertEq(d3.asSecs(), 2)
    assertEq(d3.subsecMillis(), 250)
    let d4 = d3.checkedSub(d2).unwrap()
    assertEq(d4.asMillis(), 1500)

    // Hashing
    var h1 = FnvHasher.new()
    var h2 = FnvHasher.new()
    let val: uint64 = 123456789
    val.hash(&mut h1)
    val.hash(&mut h2)
    assertEq(h1.finish(), h2.finish())
    assert(h1.finish() != 0)

    // RangeInclusive check
    let r = RangeInclusive.new(1, 5)
    assert(r.contains(&3))
    assert(!r.contains(&6))
}
