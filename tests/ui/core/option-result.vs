//@ run-pass
//@ no-core-stubs

import { Option } from "core/option"
import { Result } from "core/result"
import { ControlFlow } from "core/ops"
import { Ordering, min, max, clamp, Reverse, Ord, PartialEq } from "core/cmp"
import { From, Into, identity } from "core/convert"

func tryDivide(a: int64, b: int64) -> Option<int64> {
    if b == 0 {
        Option.None
    } else {
        Option.Some(a / b)
    }
}

func chainOption(a: int64, b: int64, c: int64) -> Option<int64> {
    let x = tryDivide(a, b)?
    let y = tryDivide(x, c)?
    Option.Some(y + 1)
}

func tryChecked(x: int64) -> Result<int64, int32> {
    if x < 0 {
        Result.Err(-1)
    } else {
        Result.Ok(x * 2)
    }
}

func chainResult(x: int64) -> Result<int64, int32> {
    let a = tryChecked(x)?
    let b = tryChecked(a + 3)?
    Result.Ok(b)
}

func main() {
    assertEq(chainOption(40, 2, 4).unwrap(), 6)
    assert(chainOption(40, 0, 4).isNone())

    let mapped = Option.Some(10)
        .map(func(x: int64) -> int64 { x * 3 })
        .filter(func(x: &int64) -> bool { *x > 20 })
        .unwrapOr(0)
    assertEq(mapped, 30)

    let zipped = Option.Some(3).zip(Option.Some(4))
    assertEq(zipped.map(func(p: (int64, int64)) -> int64 { p.0 + p.1 }).unwrap(), 7)

    let flat = Option.Some(Option.Some(99)).flatten()
    assertEq(flat.unwrap(), 99)

    assertEq(chainResult(5).unwrap(), 26)
    assert(chainResult(-2).isErr())
    assertEq(chainResult(-2).unwrapErr(), -1)

    let cf: ControlFlow<int32, int32> = ControlFlow.Continue(42)
    assert(cf.isContinue())
    assertEq(cf.continueValue().unwrap(), 42)

    assertEq(min(10, 4), 4)
    assertEq(max(10, 4), 10)
    assertEq(clamp(15, 0, 10), 10)
    assertEq(clamp(-5, 0, 10), 0)

    let r1 = Reverse { value: 1 }
    let r2 = Reverse { value: 5 }
    assert(r1.cmp(&r2).isGt())
    let ord = Ordering.Less
    assert(ord.reverse().isGt())
    assertEq(identity(123), 123)
    let converted: int64 = 77.into()
    assertEq(converted, 77)
}
