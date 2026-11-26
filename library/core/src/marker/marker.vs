//! Primitive traits and types representing basic properties of types (`core/marker`).

package marker

/// Types whose values can be duplicated simply by copying bits.
@lang("copy")
public trait Copy {}

/// Types with a constant size known at compile time.
@lang("sized")
public trait Sized {}

/// Types that can be transferred across thread boundaries.
@lang("send")
public unsafe trait Send {}

/// Types for which it is safe to share references between threads.
@lang("sync")
public unsafe trait Sync {}

/// Types that do not care about being moved after pinning.
@lang("unpin")
public trait Unpin {}

/// Zero-sized type used to mark things that "act like" they own a `T`.
@lang("PhantomData")
public struct PhantomData<T>

implement<T> Copy for PhantomData<T> {}

/// A marker type which does not implement `Unpin`.
public struct PhantomPinned

implement Copy for PhantomPinned {}
implement !Unpin for PhantomPinned {}
