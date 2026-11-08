//! Source positions, spans, the source map and interned symbols (rustc's
//! `rustc_span`).
//!
//! Vertex has one edition and no user macros, so there is no `Edition` and no
//! hygiene: a `Span` is just a byte range into the [`SourceMap`].

pub mod def_id;
mod source_map;
mod symbol;

pub use source_map::{FileName, Loc, SourceFile, SourceMap};
pub use symbol::{Ident, Symbol, kw, sym};

use std::fmt;
use std::ops::{Add, Sub};

/// A byte offset into the [`SourceMap`]'s address space. Every loaded file
/// gets its own range of positions, so a `BytePos` names a file and an offset
/// in it at once.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct BytePos(pub u32);

impl BytePos {
    #[inline]
    pub fn from_usize(n: usize) -> BytePos {
        BytePos(u32::try_from(n).expect("source position out of range"))
    }

    #[inline]
    pub fn to_usize(self) -> usize {
        self.0 as usize
    }
}

impl Add for BytePos {
    type Output = BytePos;
    #[inline]
    fn add(self, rhs: BytePos) -> BytePos {
        BytePos(self.0 + rhs.0)
    }
}

impl Sub for BytePos {
    type Output = BytePos;
    #[inline]
    fn sub(self, rhs: BytePos) -> BytePos {
        BytePos(self.0 - rhs.0)
    }
}

impl fmt::Debug for BytePos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BytePos({})", self.0)
    }
}

/// A region of source code, `lo..hi`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    lo: BytePos,
    hi: BytePos,
}

/// A span that points nowhere. Position 0 is never inside a file, because
/// the source map starts its first file at 1.
pub const DUMMY_SP: Span = Span { lo: BytePos(0), hi: BytePos(0) };

impl Span {
    #[inline]
    pub fn new(mut lo: BytePos, mut hi: BytePos) -> Span {
        if lo > hi {
            std::mem::swap(&mut lo, &mut hi);
        }
        Span { lo, hi }
    }

    #[inline]
    pub fn lo(self) -> BytePos {
        self.lo
    }

    #[inline]
    pub fn hi(self) -> BytePos {
        self.hi
    }

    #[inline]
    pub fn with_lo(self, lo: BytePos) -> Span {
        Span::new(lo, self.hi)
    }

    #[inline]
    pub fn with_hi(self, hi: BytePos) -> Span {
        Span::new(self.lo, hi)
    }

    #[inline]
    pub fn is_dummy(self) -> bool {
        self.lo.0 == 0 && self.hi.0 == 0
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.lo == self.hi
    }

    /// The span from the start of `self` to the end of `end`.
    #[inline]
    pub fn to(self, end: Span) -> Span {
        if self.is_dummy() {
            return end;
        }
        if end.is_dummy() {
            return self;
        }
        Span::new(self.lo.min(end.lo), self.hi.max(end.hi))
    }

    /// The span between the end of `self` and the start of `end`.
    #[inline]
    pub fn between(self, end: Span) -> Span {
        Span::new(self.hi, end.lo)
    }

    /// The span from the start of `self` to the start of `end`.
    #[inline]
    pub fn until(self, end: Span) -> Span {
        Span::new(self.lo, end.lo)
    }

    #[inline]
    pub fn shrink_to_lo(self) -> Span {
        Span::new(self.lo, self.lo)
    }

    #[inline]
    pub fn shrink_to_hi(self) -> Span {
        Span::new(self.hi, self.hi)
    }

    #[inline]
    pub fn contains(self, other: Span) -> bool {
        self.lo <= other.lo && other.hi <= self.hi
    }

    #[inline]
    pub fn overlaps(self, other: Span) -> bool {
        self.lo < other.hi && other.lo < self.hi
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}..{}", self.lo.0, self.hi.0)
    }
}

/// A value together with where it came from.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

pub fn respan<T>(span: Span, node: T) -> Spanned<T> {
    Spanned { node, span }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_to_and_between() {
        let a = Span::new(BytePos(2), BytePos(5));
        let b = Span::new(BytePos(8), BytePos(10));
        assert_eq!(a.to(b), Span::new(BytePos(2), BytePos(10)));
        assert_eq!(a.between(b), Span::new(BytePos(5), BytePos(8)));
        assert_eq!(a.until(b), Span::new(BytePos(2), BytePos(8)));
        assert_eq!(DUMMY_SP.to(a), a);
    }
}
