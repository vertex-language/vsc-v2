//! Asynchronous iteration (`core/async_iter`).

package async_iter

import { Pin } from "core/pin"
import { Context, Poll } from "core/task"
import { Option } from "core/option"

/// An interface for dealing with asynchronous iterators (streams).
public trait AsyncIterator {
    type Item

    func pollNext(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self.Item>>
}
