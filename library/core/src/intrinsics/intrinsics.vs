//! Compiler intrinsics (`core/intrinsics`).
//!
//! Functions marked `@intrinsic("...")` are lowered directly by the MIR
//! interpreter and the Obsidian codegen backend.

package intrinsics

// Layout and type queries.

@intrinsic("size_of")
public func sizeOf<T>() -> uint { loop {} }

@intrinsic("align_of")
public func alignOf<T>() -> uint { loop {} }

@intrinsic("needs_drop")
public func needsDrop<T>() -> bool { loop {} }

@intrinsic("transmute")
public unsafe func transmute<Src, Dst>(src: Src) -> Dst { loop {} }

@intrinsic("black_box")
public func blackBox<T>(dummy: T) -> T { loop {} }

@intrinsic("spin_loop")
public func spinLoop() {}

@intrinsic("abort")
public func abort() -> never { loop {} }

@intrinsic("unreachable")
public unsafe func unreachableHint() -> never { loop {} }

// Slice length.

@intrinsic("slice_len")
public func sliceLen<T>(s: &[T]) -> uint { loop {} }

// Bulk memory and volatile operations.

@intrinsic("copy")
public unsafe func copy<T>(src: *const T, dst: *mut T, count: uint) {}

@intrinsic("copy_nonoverlapping")
public unsafe func copyNonoverlapping<T>(src: *const T, dst: *mut T, count: uint) {}

@intrinsic("write_bytes")
public unsafe func writeBytes<T>(dst: *mut T, val: uint8, count: uint) {}

@intrinsic("volatile_load")
public unsafe func volatileLoad<T>(src: *const T) -> T { loop {} }

@intrinsic("volatile_store")
public unsafe func volatileStore<T>(dst: *mut T, val: T) {}

// Bit manipulation intrinsics.

@intrinsic("ctpop")
public func ctpopU8(x: uint8) -> uint32 { loop {} }

@intrinsic("ctpop")
public func ctpopU16(x: uint16) -> uint32 { loop {} }

@intrinsic("ctpop")
public func ctpopU32(x: uint32) -> uint32 { loop {} }

@intrinsic("ctpop")
public func ctpopU64(x: uint64) -> uint32 { loop {} }

@intrinsic("ctpop")
public func ctpopUsize(x: uint) -> uint32 { loop {} }

@intrinsic("ctlz")
public func ctlzU8(x: uint8) -> uint32 { loop {} }

@intrinsic("ctlz")
public func ctlzU16(x: uint16) -> uint32 { loop {} }

@intrinsic("ctlz")
public func ctlzU32(x: uint32) -> uint32 { loop {} }

@intrinsic("ctlz")
public func ctlzU64(x: uint64) -> uint32 { loop {} }

@intrinsic("ctlz")
public func ctlzUsize(x: uint) -> uint32 { loop {} }

@intrinsic("cttz")
public func cttzU8(x: uint8) -> uint32 { loop {} }

@intrinsic("cttz")
public func cttzU16(x: uint16) -> uint32 { loop {} }

@intrinsic("cttz")
public func cttzU32(x: uint32) -> uint32 { loop {} }

@intrinsic("cttz")
public func cttzU64(x: uint64) -> uint32 { loop {} }

@intrinsic("cttz")
public func cttzUsize(x: uint) -> uint32 { loop {} }

@intrinsic("bswap")
public func bswapU16(x: uint16) -> uint16 { loop {} }

@intrinsic("bswap")
public func bswapU32(x: uint32) -> uint32 { loop {} }

@intrinsic("bswap")
public func bswapU64(x: uint64) -> uint64 { loop {} }

@intrinsic("bitreverse")
public func bitreverseU8(x: uint8) -> uint8 { loop {} }

@intrinsic("bitreverse")
public func bitreverseU16(x: uint16) -> uint16 { loop {} }

@intrinsic("bitreverse")
public func bitreverseU32(x: uint32) -> uint32 { loop {} }

@intrinsic("bitreverse")
public func bitreverseU64(x: uint64) -> uint64 { loop {} }

@intrinsic("rotate_left")
public func rotateLeftU32(x: uint32, shift: uint32) -> uint32 { loop {} }

@intrinsic("rotate_left")
public func rotateLeftU64(x: uint64, shift: uint32) -> uint64 { loop {} }

@intrinsic("rotate_right")
public func rotateRightU32(x: uint32, shift: uint32) -> uint32 { loop {} }

@intrinsic("rotate_right")
public func rotateRightU64(x: uint64, shift: uint32) -> uint64 { loop {} }

// Floating-point math intrinsics.

@intrinsic("sqrtf32")
public func sqrtf32(x: float32) -> float32 { loop {} }

@intrinsic("sqrtf64")
public func sqrtf64(x: float64) -> float64 { loop {} }

@intrinsic("fabsf32")
public func fabsf32(x: float32) -> float32 { loop {} }

@intrinsic("fabsf64")
public func fabsf64(x: float64) -> float64 { loop {} }

@intrinsic("floorf32")
public func floorf32(x: float32) -> float32 { loop {} }

@intrinsic("floorf64")
public func floorf64(x: float64) -> float64 { loop {} }

@intrinsic("ceilf32")
public func ceilf32(x: float32) -> float32 { loop {} }

@intrinsic("ceilf64")
public func ceilf64(x: float64) -> float64 { loop {} }

@intrinsic("truncf32")
public func truncf32(x: float32) -> float32 { loop {} }

@intrinsic("truncf64")
public func truncf64(x: float64) -> float64 { loop {} }

@intrinsic("roundevenf32")
public func roundevenf32(x: float32) -> float32 { loop {} }

@intrinsic("roundevenf64")
public func roundevenf64(x: float64) -> float64 { loop {} }

// Atomic fences.

@intrinsic("atomic_fence_acquire")
public func atomicFenceAcquire() {}

@intrinsic("atomic_fence_release")
public func atomicFenceRelease() {}

@intrinsic("atomic_fence_acqrel")
public func atomicFenceAcqRel() {}

@intrinsic("atomic_fence_seqcst")
public func atomicFenceSeqCst() {}

@intrinsic("atomic_singlethreadfence_acquire")
public func atomicSingleThreadFenceAcquire() {}

@intrinsic("atomic_singlethreadfence_release")
public func atomicSingleThreadFenceRelease() {}

@intrinsic("atomic_singlethreadfence_acqrel")
public func atomicSingleThreadFenceAcqRel() {}

@intrinsic("atomic_singlethreadfence_seqcst")
public func atomicSingleThreadFenceSeqCst() {}

// Atomic operations (generic over primitive integer/bool widths).

@intrinsic("atomic_load_relaxed")
public unsafe func atomicLoadRelaxed<T>(src: *const T) -> T { loop {} }

@intrinsic("atomic_load_acquire")
public unsafe func atomicLoadAcquire<T>(src: *const T) -> T { loop {} }

@intrinsic("atomic_load_seqcst")
public unsafe func atomicLoadSeqCst<T>(src: *const T) -> T { loop {} }

@intrinsic("atomic_store_relaxed")
public unsafe func atomicStoreRelaxed<T>(dst: *mut T, val: T) {}

@intrinsic("atomic_store_release")
public unsafe func atomicStoreRelease<T>(dst: *mut T, val: T) {}

@intrinsic("atomic_store_seqcst")
public unsafe func atomicStoreSeqCst<T>(dst: *mut T, val: T) {}

@intrinsic("atomic_xchg_relaxed")
public unsafe func atomicXchgRelaxed<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xchg_acquire")
public unsafe func atomicXchgAcquire<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xchg_release")
public unsafe func atomicXchgRelease<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xchg_acqrel")
public unsafe func atomicXchgAcqRel<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xchg_seqcst")
public unsafe func atomicXchgSeqCst<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xadd_relaxed")
public unsafe func atomicXaddRelaxed<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xadd_acquire")
public unsafe func atomicXaddAcquire<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xadd_release")
public unsafe func atomicXaddRelease<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xadd_acqrel")
public unsafe func atomicXaddAcqRel<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xadd_seqcst")
public unsafe func atomicXaddSeqCst<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xsub_relaxed")
public unsafe func atomicXsubRelaxed<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xsub_acquire")
public unsafe func atomicXsubAcquire<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xsub_release")
public unsafe func atomicXsubRelease<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xsub_acqrel")
public unsafe func atomicXsubAcqRel<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xsub_seqcst")
public unsafe func atomicXsubSeqCst<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_and_relaxed")
public unsafe func atomicAndRelaxed<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_and_acqrel")
public unsafe func atomicAndAcqRel<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_and_seqcst")
public unsafe func atomicAndSeqCst<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_or_relaxed")
public unsafe func atomicOrRelaxed<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_or_acqrel")
public unsafe func atomicOrAcqRel<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_or_seqcst")
public unsafe func atomicOrSeqCst<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xor_relaxed")
public unsafe func atomicXorRelaxed<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xor_acqrel")
public unsafe func atomicXorAcqRel<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_xor_seqcst")
public unsafe func atomicXorSeqCst<T>(dst: *mut T, val: T) -> T { loop {} }

@intrinsic("atomic_cxchg_relaxed_relaxed")
public unsafe func atomicCxchgRelaxed<T>(dst: *mut T, old: T, src: T) -> (T, bool) { loop {} }

@intrinsic("atomic_cxchg_acquire_relaxed")
public unsafe func atomicCxchgAcquire<T>(dst: *mut T, old: T, src: T) -> (T, bool) { loop {} }

@intrinsic("atomic_cxchg_release_relaxed")
public unsafe func atomicCxchgRelease<T>(dst: *mut T, old: T, src: T) -> (T, bool) { loop {} }

@intrinsic("atomic_cxchg_acqrel_acquire")
public unsafe func atomicCxchgAcqRel<T>(dst: *mut T, old: T, src: T) -> (T, bool) { loop {} }

@intrinsic("atomic_cxchg_seqcst_seqcst")
public unsafe func atomicCxchgSeqCst<T>(dst: *mut T, old: T, src: T) -> (T, bool) { loop {} }
