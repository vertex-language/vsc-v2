//! Custom code within the destructor (`core/ops`).

package ops

/// Custom code within the destructor.
@lang("drop")
public trait Drop {
    func drop(&mut self)
}
