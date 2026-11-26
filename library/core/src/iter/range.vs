//! Range iterator implementations (`core/iter`).

package iter

import { Option } from "core/option"
import { Range, RangeInclusive } from "core/ops"

implement<T> Iterator for Range<T> {
    type Item = T

    @intrinsic("range_next")
    func next(&mut self) -> Option<T> { loop {} }
}

implement<T> Iterator for RangeInclusive<T> {
    type Item = T

    @intrinsic("range_inclusive_next")
    func next(&mut self) -> Option<T> { loop {} }
}
