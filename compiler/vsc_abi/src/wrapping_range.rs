//! Wrapping integer value ranges for scalar validity and niche optimization.

use std::fmt;

use crate::Size;

/// Inclusive range of bit-pattern values `[start, end]` modulo `2^(bits)`.
///
/// If `start > end`, the range wraps around `0`, e.g. `[0xff, 0x01]` for a byte
/// allows `0xff`, `0x00`, and `0x01`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WrappingRange {
    pub start: u128,
    pub end: u128,
}

impl fmt::Debug for WrappingRange {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "[{:#x}..={:#x}]", self.start, self.end)
    }
}

impl WrappingRange {
    /// Returns the full range covering all representable values of `size`.
    #[inline]
    pub fn full(size: Size) -> Self {
        Self { start: 0, end: size.unsigned_int_max() }
    }

    /// Returns `true` if `self` covers all representable values of `size`.
    #[inline]
    pub fn is_full_for(self, size: Size) -> bool {
        let mask = size.unsigned_int_max();
        let start = self.start & mask;
        let end = self.end & mask;
        self.wrapping_size(size) == mask && start <= mask && end <= mask
    }

    /// Number of valid values minus 1 within `size` (returns `mask` if full).
    #[inline]
    pub fn wrapping_size(self, size: Size) -> u128 {
        let mask = size.unsigned_int_max();
        (self.end & mask).wrapping_sub(self.start & mask) & mask
    }

    /// Returns `true` if `v` (masked to `size`) falls within `self`.
    #[inline]
    pub fn contains(self, v: u128, size: Size) -> bool {
        let mask = size.unsigned_int_max();
        let v = v & mask;
        let start = self.start & mask;
        let end = self.end & mask;
        if start <= end { v >= start && v <= end } else { v >= start || v <= end }
    }

    /// Returns a new range shifted upward by `offset` modulo `size`.
    #[inline]
    pub fn offset(self, offset: u128, size: Size) -> Self {
        let mask = size.unsigned_int_max();
        Self {
            start: self.start.wrapping_add(offset) & mask,
            end: self.end.wrapping_add(offset) & mask,
        }
    }
}
