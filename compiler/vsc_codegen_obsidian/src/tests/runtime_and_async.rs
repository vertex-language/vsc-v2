//! Atomics, intrinsics, optimization levels, TLS, core library, and async/await codegen tests.

use vsc_session::config::OptLevel;

use super::{compile_and_run_binary, compile_and_run_binary_with_opt};

#[test]
fn compiles_atomics_fences_bulk_memory_and_bit_intrinsics() {
    let src = r#"
package main

@intrinsic("atomic_store_release")
func atomicStore64(p: *mut int64, v: int64) { loop {} }

@intrinsic("atomic_load_acquire")
func atomicLoad64(p: *const int64) -> int64 { loop {} }

@intrinsic("atomic_xadd_acqrel")
func atomicAdd64(p: *mut int64, v: int64) -> int64 { loop {} }

@intrinsic("atomic_cxchg_seqcst_acquire")
func atomicCas64(p: *mut int64, expected: int64, desired: int64) -> (int64, bool) { loop {} }

@intrinsic("atomic_fence_seqcst")
func fenceSeqCst() { loop {} }

@intrinsic("spin_hint")
func spinHint() { loop {} }

@intrinsic("write_bytes")
func writeBytes<T>(dst: *mut T, val: uint8, count: uint) { loop {} }

@intrinsic("copy")
func copyOverlap<T>(src: *const T, dst: *mut T, count: uint) { loop {} }

@intrinsic("ctpop")
func popcnt64(x: uint64) -> uint32 { loop {} }

@intrinsic("ctlz")
func clz32(x: uint32) -> uint32 { loop {} }

@intrinsic("cttz")
func ctz32(x: uint32) -> uint32 { loop {} }

@intrinsic("bswap")
func bswap32(x: uint32) -> uint32 { loop {} }

@intrinsic("bitreverse")
func bitrev8(x: uint8) -> uint8 { loop {} }

@intrinsic("rotate_left")
func rotl32(x: uint32, n: uint32) -> uint32 { loop {} }

@intrinsic("rotate_right")
func rotr32(x: uint32, n: uint32) -> uint32 { loop {} }

func main() -> int32 {
    var cell: int64 = 0
    let p = &mut cell as *mut int64
    atomicStore64(p, 10)
    fenceSeqCst()
    spinHint()
    let old = atomicAdd64(p, 5) // old=10, cell=15
    let fail = atomicCas64(p, 99, 500) // (15, false)
    let ok = atomicCas64(p, 15, 42) // (15, true), cell=42
    let final_v = atomicLoad64(&cell as *const int64)

    var buf: [uint8; 8] = [0; 8]
    writeBytes(&mut buf[0] as *mut uint8, 9, 8)
    buf[0] = 1
    buf[1] = 2
    buf[2] = 3
    buf[3] = 4
    // Overlapping copy: buf[0..4] -> buf[2..6] => [1, 2, 1, 2, 3, 4, 9, 9]
    let base_p = &mut buf[0] as *mut uint8
    let dst_p = ((base_p as uint) + 2) as *mut uint8
    copyOverlap(base_p as *const uint8, dst_p, 4)
    let mem_ok = buf[2] == 1 && buf[3] == 2 && buf[4] == 3 && buf[5] == 4 && buf[6] == 9

    let bits_ok = popcnt64(0xF0F00F0F) == 16
        && clz32(0x00F00000) == 8
        && ctz32(0x00000010) == 4
        && bswap32(0x12345678) == 0x78563412
        && bitrev8(0b00001101) == 0b10110000
        && rotr32(rotl32(0xCAFEBABE, 7), 7) == 0xCAFEBABE

    if old == 10 && fail.0 == 15 && !fail.1 && ok.0 == 15 && ok.1 && final_v == 42 && mem_ok && bits_ok {
        42
    } else {
        1
    }
}
"#;
    let (code, _stdout, stderr, _ir) = compile_and_run_binary(src, "atomics_mem_bits");
    assert_eq!(code, 42, "stderr: {stderr}");
}

#[test]
fn optimizes_loops_and_helpers_across_opt_levels() {
    let src = r#"
func scaleAndBias(x: int64) -> int64 {
    let a = x * 8
    let b = (a + 0) * 1
    b + 1
}

func fib(n: int64) -> int64 {
    var a: int64 = 0
    var b: int64 = 1
    var i: int64 = 0
    while i < n {
        let next = a + b
        a = b
        b = next
        i = i + 1
    }
    a
}

func main() -> int32 {
    let f10 = fib(10) // 55
    let s = scaleAndBias(5) // 41
    // Redundant arithmetic + constant branch
    let dup = (f10 + s) + (s + f10) // 96 + 96 = 192
    let branch_val = if 3 < 5 { 10 } else { 999 }
    if f10 == 55 && s == 41 && dup == 192 && branch_val == 10 {
        42
    } else {
        1
    }
}
"#;
    let (code_o0, _, stderr_o0, ir_o0) =
        compile_and_run_binary_with_opt(src, "opt_levels_o0", OptLevel::No);
    let (code_o1, _, stderr_o1, _ir_o1) =
        compile_and_run_binary_with_opt(src, "opt_levels_o1", OptLevel::Less);
    let (code_o2, _, stderr_o2, _ir_o2) =
        compile_and_run_binary_with_opt(src, "opt_levels_o2", OptLevel::Default);
    let (code_o3, _, stderr_o3, ir_o3) =
        compile_and_run_binary_with_opt(src, "opt_levels_o3", OptLevel::Aggressive);

    assert_eq!(code_o0, 42, "O0 stderr: {stderr_o0}");
    assert_eq!(code_o1, 42, "O1 stderr: {stderr_o1}");
    assert_eq!(code_o2, 42, "O2 stderr: {stderr_o2}");
    assert_eq!(code_o3, 42, "O3 stderr: {stderr_o3}");

    let slots_o0 = ir_o0.matches("stack.slot").count();
    let slots_o3 = ir_o3.matches("stack.slot").count();
    assert!(
        slots_o3 < slots_o0,
        "expected Mem2Reg at -O3 to eliminate stack.slot ops (O0={slots_o0}, O3={slots_o3})\n--- O3 IR ---\n{ir_o3}"
    );
    assert!(
        ir_o3.lines().count() < ir_o0.lines().count(),
        "expected -O3 IR ({} lines) to be smaller than -O0 IR ({} lines)",
        ir_o3.lines().count(),
        ir_o0.lines().count()
    );
}

#[test]
fn compiles_multithreaded_thread_local_statics() {
    let src = r#"
@threadLocal
static var TL_INIT: int64 = 100

@threadLocal
static var TL_ZERO: int64 = 0

unsafe extern "C" {
    func pthread_create(
        tid: *mut *mut uint8,
        attr: *const uint8,
        start: extern "C" func(*mut uint8) -> *mut uint8,
        arg: *mut uint8
    ) -> int32;
    func pthread_join(tid: *mut uint8, retval: *mut *mut uint8) -> int32;
}

extern "C" func worker(_arg: *mut uint8) -> *mut uint8 {
    unsafe {
        let a = TL_INIT
        let b = TL_ZERO
        TL_INIT = 999
        TL_ZERO = 888
        (a + b) as uint as *mut uint8
    }
}

func main() -> int32 {
    unsafe {
        TL_INIT = 150
        TL_ZERO = 7
        var tid: *mut uint8 = 0 as *mut uint8
        var ret: *mut uint8 = 0 as *mut uint8
        let null_c = 0 as *const uint8
        let null_m = 0 as *mut uint8
        pthread_create(&mut tid as *mut *mut uint8, null_c, worker, null_m)
        pthread_join(tid, &mut ret as *mut *mut uint8)
        let worker_sum = ret as uint as int64
        if worker_sum == 100 && TL_INIT == 150 && TL_ZERO == 7 {
            42
        } else {
            1
        }
    }
}
"#;
    let (code, _stdout, stderr, ir) = compile_and_run_binary(src, "thread_local_pthread");
    assert!(ir.contains("tls @"), "expected `tls @` in emitted .obs IR:\n{ir}");
    assert_eq!(code, 42, "stderr: {stderr}");
}

#[test]
fn compiles_and_runs_program_importing_library_core() {
    let src = r#"
package main

import { Option } from "core/option"
import { Result } from "core/result"
import { Ordering, min, max, clamp, Reverse, Ord } from "core/cmp"
import { sizeOf, alignOf, swap, replace } from "core/mem"
import { NonNull, copyNonoverlapping, writeBytes } from "core/ptr"
import { Cell } from "core/cell"
import { NonZeroUint32, countOnesU32, leadingZerosU32, trailingZerosU32, swapBytesU32, rotateLeftU32Val, sqrtF64, absF64 } from "core/num"
import { Ordering as MemOrder, AtomicInt64, fence } from "core/sync/atomic"
import { isAsciiDigit, toAsciiUppercase } from "core/ascii"
import { Duration } from "core/time"
import { Hasher, Hash, FnvHasher } from "core/hash"
import { len as sliceLen, first, last } from "core/slice"
import { thenSome } from "core/bool"
import { lenUtf8, toDigit } from "core/char"
import { blackBox, spinLoop } from "core/hint"

func divide(a: int64, b: int64) -> Option<int64> {
    if b == 0 { Option.None } else { Option.Some(a / b) }
}

func compute(a: int64, b: int64) -> Option<int64> {
    let q = divide(a, b)?
    Option.Some(q + 5)
}

func checkPositive(x: int64) -> Result<int64, int32> {
    if x > 0 { Result.Ok(x * 2) } else { Result.Err(-1) }
}

func main() -> int32 {
    var score: int32 = 0

    // Option & ?
    if compute(20, 4).unwrap() == 10 && compute(20, 0).isNone() { score += 1 }
    // Result
    if checkPositive(6).unwrap() == 12 && checkPositive(-3).unwrapErr() == -1 { score += 1 }
    // cmp
    let r1 = Reverse { value: 2 }
    let r2 = Reverse { value: 9 }
    if min(8, 3) == 3 && max(8, 3) == 8 && clamp(12, 0, 5) == 5 && r1.cmp(&r2).isGt() { score += 1 }
    // mem
    var a: int64 = 10
    var b: int64 = 25
    swap(&mut a, &mut b)
    let prev = replace(&mut a, 99)
    if a == 99 && b == 10 && prev == 25 && sizeOf<int64>() == 8 && alignOf<int64>() == 8 { score += 1 }
    // ptr
    var buf1: [uint8; 4] = [1, 2, 3, 4]
    var buf2: [uint8; 4] = [0, 0, 0, 0]
    unsafe {
        copyNonoverlapping(&buf1[0] as *const uint8, &mut buf2[0] as *mut uint8, 4)
        writeBytes(&mut buf1[0] as *mut uint8, 7, 2)
    }
    let nn = NonNull.new(&mut buf2[0] as *mut uint8)
    if buf2[3] == 4 && buf1[0] == 7 && buf1[1] == 7 && buf1[2] == 3 && nn.isSome() { score += 1 }
    // cell
    let cell = Cell.new(100)
    cell.set(200)
    let oldCell = cell.replace(300)
    if oldCell == 200 && cell.get() == 300 { score += 1 }
    // atomics
    let atom = AtomicInt64.new(10)
    let oldAtom = atom.fetchAdd(5, MemOrder.AcqRel)
    let casOk = atom.compareExchange(15, 42, MemOrder.SeqCst, MemOrder.Acquire).isOk()
    fence(MemOrder.SeqCst)
    if oldAtom == 10 && casOk && atom.load(MemOrder.Acquire) == 42 { score += 1 }
    // num bit & math intrinsics
    if NonZeroUint32.new(0).isNone()
        && NonZeroUint32.new(7).unwrap().get() == 7
        && countOnesU32(0b1101) == 3
        && leadingZerosU32(1) == 31
        && trailingZerosU32(8) == 3
        && swapBytesU32(0x11223344) == 0x44332211
        && rotateLeftU32Val(0x80000001, 1) == 0x00000003
        && sqrtF64(16.0) == 4.0
        && absF64(-2.5) == 2.5
    {
        score += 1
    }
    // ascii, time, hash, slice, bool, char, hint
    let d = Duration.fromMillis(1250).checkedAdd(Duration.fromMillis(750)).unwrap()
    var hasher = FnvHasher.new()
    let hv: uint64 = 42
    hv.hash(&mut hasher)
    let nums: [int64; 3] = [11, 22, 33]
    spinLoop()
    if isAsciiDigit(57)
        && toAsciiUppercase(97) == 65
        && d.asSecs() == 2
        && hasher.finish() != 0
        && sliceLen(&nums) == 3
        && *first(&nums).unwrap() == 11
        && *last(&nums).unwrap() == 33
        && thenSome(true, 5).unwrap() == 5
        && lenUtf8('Z') == 1
        && toDigit('a', 16).unwrap() == 10
        && blackBox(9) == 9
    {
        score += 1
    }

    if score == 9 { 42 } else { score }
}
"#;
    let (code, _stdout, stderr, _ir) = compile_and_run_binary(src, "library_core_e2e");
    assert_eq!(code, 42, "stderr: {stderr}");
}

#[test]
fn compiles_and_runs_async_await_combinators_afit_and_drops() {
    let src = r#"
package main

import "core/future"
import "core/pin"
import "core/task"
import "core/ops"
import "core/marker"

@lang("blockOn")
func testBlockOn<F: future.Future>(var f: F) -> F.Output {
    let waker = task.Waker.noop()
    var cx = task.Context.fromWaker(&waker)
    loop {
        let pinned = unsafe { pin.Pin.newUnchecked(&mut f) }
        match future.Future.poll(pinned, &mut cx) {
            task.Poll.Ready(v) => return v
            task.Poll.Pending => {}
        }
    }
}

static var CANCEL_DROPS: int64 = 0

struct CancelGuard {
    weight: int64
}

implement ops.Drop for CancelGuard {
    func drop(&mut self) {
        unsafe { CANCEL_DROPS = CANCEL_DROPS + self.weight }
    }
}

async func addYield(a: int64, b: int64) -> int64 {
    future.yieldNow().await
    let x = future.ready(a * 2).await
    future.yieldNow().await
    let y = future.ready(b + 3).await
    x + y
}

async func nestedJoin(x: int64) -> int64 {
    let (a, b) = future.join2(addYield(x, 1), addYield(x, 2)).await
    a + b
}

func makeImplFut(n: int64) -> impl future.Future<Output = int64> {
    async {
        let a = addYield(n, 1).await
        a + 100
    }
}

trait AsyncCompute {
    async func compute(&self, x: int64) -> int64
}

struct Multiplier {
    factor: int64
}

implement AsyncCompute for Multiplier {
    async func compute(&self, x: int64) -> int64 {
        future.yieldNow().await
        self.factor * x
    }
}

async func runTrait<C: AsyncCompute>(c: &C, x: int64) -> int64 {
    c.compute(x).await
}

async func cancelableTask() -> int64 {
    let _g = CancelGuard { weight: 13 }
    future.yieldNow().await
    999
}

func pollOnceAndDrop() {
    var f = cancelableTask()
    let waker = task.Waker.noop()
    var cx = task.Context.fromWaker(&waker)
    let p = unsafe { pin.Pin.newUnchecked(&mut f) }
    let _ = future.Future.poll(p, &mut cx)
}

func assertSend<T: marker.Send>(_v: T) {}

async func main() -> int32 {
    var score: int32 = 0

    // 1. Multi-await async func
    if addYield(10, 5).await == 28 { score += 1 }
    // 2. join2 + join3 + nested join
    let (j1, j2) = future.join2(addYield(10, 5), addYield(2, 3)).await
    let (k1, k2, k3) = future.join3(future.ready(1 as int64), addYield(1, 2), future.ready(9 as int64)).await
    if j1 == 28 && j2 == 10 && k1 == 1 && k2 == 7 && k3 == 9 && nestedJoin(10).await == 49 { score += 1 }
    // 3. race2 & select2
    let r = future.race2(future.ready(7 as int64), future.pending()).await
    let p: future.Pending<int64> = future.pending()
    let s = future.select2(p, future.ready(99 as int64)).await
    let sOk = match s {
        future.Either.Left(_) => false
        future.Either.Right(v) => v == 99
    }
    if r == 7 && sOk { score += 1 }
    // 4. impl Future + async block
    if makeImplFut(5).await == 114 { score += 1 }
    // 5. AFIT (async func in trait)
    let m = Multiplier { factor: 6 }
    if runTrait(&m, 7).await == 42 { score += 1 }
    // 6. Cancellation drops live coroutine locals
    pollOnceAndDrop()
    if unsafe { CANCEL_DROPS } == 13 { score += 1 }
    // 7. Auto-trait Send satisfied for plain future
    assertSend(addYield(1, 2))
    score += 1

    if score == 7 { 42 } else { score }
}
"#;
    let (code, _stdout, stderr, _ir) = compile_and_run_binary(src, "async_await_e2e");
    assert_eq!(code, 42, "stderr: {stderr}");
}
