//! The core prelude (`core/prelude`).
//!
//! Convenience type aliases for the most commonly used `core` types.

package prelude

import option from "core/option"
import result from "core/result"
import cmp from "core/cmp"
import ops from "core/ops"
import marker from "core/marker"
import mem from "core/mem"
import ptr from "core/ptr"
import cell from "core/cell"
import pin from "core/pin"
import task from "core/task"
import future from "core/future"
import time from "core/time"
import num from "core/num"

public type Option<T> = option.Option<T>
public type Result<T, E> = result.Result<T, E>
public type Ordering = cmp.Ordering
public type Range<T> = ops.Range<T>
public type RangeInclusive<T> = ops.RangeInclusive<T>
public type ControlFlow<B, C> = ops.ControlFlow<B, C>
public type PhantomData<T> = marker.PhantomData<T>
public type PhantomPinned = marker.PhantomPinned
public type ManuallyDrop<T> = mem.ManuallyDrop<T>
public type NonNull<T> = ptr.NonNull<T>
public type UnsafeCell<T> = cell.UnsafeCell<T>
public type Cell<T> = cell.Cell<T>
public type Pin<P> = pin.Pin<P>
public type Poll<T> = task.Poll<T>
public type Waker = task.Waker
public type LocalWaker = task.LocalWaker
public type RawWaker = task.RawWaker
public type RawWakerVTable = task.RawWakerVTable
public type Context<a> = task.Context<a>
public type Ready<T> = future.Ready<T>
public type Pending<T> = future.Pending<T>
public type Duration = time.Duration
public type Wrapping<T> = num.Wrapping<T>
public type Saturating<T> = num.Saturating<T>
