//! Typed indices and the vectors they index (rustc's `rustc_index`).
//!
//! `newtype_index!` makes a `u32`-sized index type that stores `value + 1` in a
//! `NonZeroU32`, so `Option<MyIdx>` is still four bytes. rustc gets the same
//! niche from the nightly `rustc_layout_scalar_valid_range_end` attribute; this
//! is the stable way.

mod bit_set;
mod vec;

pub use bit_set::DenseBitSet;
pub use vec::{IndexSlice, IndexVec};

use std::fmt::Debug;
use std::hash::Hash;

/// A type that can be used as an index into an [`IndexVec`].
pub trait Idx: Copy + Eq + Hash + Ord + Debug + 'static {
    fn new(idx: usize) -> Self;
    fn index(self) -> usize;

    #[inline]
    fn plus(self, amount: usize) -> Self {
        Self::new(self.index() + amount)
    }
}

impl Idx for usize {
    #[inline]
    fn new(idx: usize) -> Self {
        idx
    }
    #[inline]
    fn index(self) -> usize {
        self
    }
}

impl Idx for u32 {
    #[inline]
    fn new(idx: usize) -> Self {
        u32::try_from(idx).expect("index out of range for u32")
    }
    #[inline]
    fn index(self) -> usize {
        self as usize
    }
}

/// Declares a `u32`-sized index type.
///
/// ```
/// vsc_index::newtype_index! {
///     /// A basic block.
///     pub struct BasicBlock {}
/// }
/// ```
#[macro_export]
macro_rules! newtype_index {
    ($(#[$attr:meta])* $vis:vis struct $name:ident {}) => {
        $crate::newtype_index! { $(#[$attr])* $vis struct $name { DEBUG_FORMAT = "{}({})", stringify!($name) } }
    };
    // `DEBUG_FORMAT = "_{}"` prints the index alone, as rustc's MIR locals.
    ($(#[$attr:meta])* $vis:vis struct $name:ident { DEBUG_FORMAT = $fmt:literal }) => {
        $crate::newtype_index! { @debug $(#[$attr])* $vis struct $name { DEBUG_FORMAT = $fmt, } }
    };
    ($(#[$attr:meta])* $vis:vis struct $name:ident { DEBUG_FORMAT = $fmt:literal, $($prefix:expr)? }) => {
        $crate::newtype_index! { @debug $(#[$attr])* $vis struct $name { DEBUG_FORMAT = $fmt, $($prefix)? } }
    };
    (@debug $(#[$attr:meta])* $vis:vis struct $name:ident { DEBUG_FORMAT = $fmt:literal, $($prefix:expr)? }) => {
        $(#[$attr])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        $vis struct $name(::std::num::NonZeroU32);

        #[allow(dead_code)]
        impl $name {
            /// The largest index this type can hold.
            pub const MAX_AS_U32: u32 = u32::MAX - 1;
            pub const MAX: Self = Self::from_u32(Self::MAX_AS_U32);
            pub const ZERO: Self = Self::from_u32(0);

            #[inline]
            pub const fn from_u32(value: u32) -> Self {
                assert!(value <= Self::MAX_AS_U32, "index out of range");
                // SAFETY: value + 1 is at least 1 and at most u32::MAX.
                match ::std::num::NonZeroU32::new(value + 1) {
                    Some(n) => Self(n),
                    None => unreachable!(),
                }
            }

            #[inline]
            pub const fn from_usize(value: usize) -> Self {
                assert!(value <= Self::MAX_AS_U32 as usize, "index out of range");
                Self::from_u32(value as u32)
            }

            #[inline]
            pub const fn as_u32(self) -> u32 {
                self.0.get() - 1
            }

            #[inline]
            pub const fn as_usize(self) -> usize {
                self.as_u32() as usize
            }
        }

        impl $crate::Idx for $name {
            #[inline]
            fn new(idx: usize) -> Self {
                Self::from_usize(idx)
            }
            #[inline]
            fn index(self) -> usize {
                self.as_usize()
            }
        }

        impl ::std::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, $fmt, $($prefix,)? self.as_u32())
            }
        }

        impl From<usize> for $name {
            #[inline]
            fn from(value: usize) -> Self {
                Self::from_usize(value)
            }
        }

        impl From<u32> for $name {
            #[inline]
            fn from(value: u32) -> Self {
                Self::from_u32(value)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    newtype_index! {
        struct TestIdx {}
    }

    #[test]
    fn niche_keeps_option_small() {
        assert_eq!(std::mem::size_of::<TestIdx>(), 4);
        assert_eq!(std::mem::size_of::<Option<TestIdx>>(), 4);
    }

    #[test]
    fn round_trips() {
        assert_eq!(TestIdx::from_u32(7).as_u32(), 7);
        assert_eq!(TestIdx::new(3).index(), 3);
        assert_eq!(TestIdx::MAX.as_u32(), u32::MAX - 1);
    }

    #[test]
    fn index_vec_push_and_iterate() {
        let mut v: IndexVec<TestIdx, &str> = IndexVec::new();
        let a = v.push("a");
        let b = v.push("b");
        assert_eq!(v[a], "a");
        assert_eq!(v[b], "b");
        assert_eq!(v.next_index(), TestIdx::new(2));
        let pairs: Vec<_> = v.iter_enumerated().map(|(i, s)| (i.index(), *s)).collect();
        assert_eq!(pairs, vec![(0, "a"), (1, "b")]);
    }

    #[test]
    fn bit_set() {
        let mut s: DenseBitSet<TestIdx> = DenseBitSet::new_empty(130);
        assert!(s.insert(TestIdx::new(0)));
        assert!(s.insert(TestIdx::new(129)));
        assert!(!s.insert(TestIdx::new(129)));
        assert!(s.contains(TestIdx::new(129)));
        assert!(!s.contains(TestIdx::new(64)));
        assert_eq!(s.iter().map(Idx::index).collect::<Vec<_>>(), vec![0, 129]);
        assert!(s.remove(TestIdx::new(0)));
        assert_eq!(s.count(), 1);
    }
}
