use std::fmt;
use std::marker::PhantomData;

use crate::Idx;

const WORD_BITS: usize = u64::BITS as usize;

/// A fixed-size set of indices, one bit per index (rustc's `DenseBitSet`).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct DenseBitSet<T> {
    domain_size: usize,
    words: Vec<u64>,
    _marker: PhantomData<T>,
}

impl<T: Idx> DenseBitSet<T> {
    pub fn new_empty(domain_size: usize) -> Self {
        DenseBitSet { domain_size, words: vec![0; domain_size.div_ceil(WORD_BITS)], _marker: PhantomData }
    }

    pub fn new_filled(domain_size: usize) -> Self {
        let mut set = DenseBitSet {
            domain_size,
            words: vec![!0; domain_size.div_ceil(WORD_BITS)],
            _marker: PhantomData,
        };
        set.clear_excess_bits();
        set
    }

    #[inline]
    pub fn domain_size(&self) -> usize {
        self.domain_size
    }

    #[inline]
    fn word_and_mask(&self, elem: T) -> (usize, u64) {
        let i = elem.index();
        assert!(i < self.domain_size, "index {i} out of bit set domain {}", self.domain_size);
        (i / WORD_BITS, 1 << (i % WORD_BITS))
    }

    #[inline]
    pub fn contains(&self, elem: T) -> bool {
        let (w, m) = self.word_and_mask(elem);
        self.words[w] & m != 0
    }

    /// Inserts `elem`; returns whether the set changed.
    #[inline]
    pub fn insert(&mut self, elem: T) -> bool {
        let (w, m) = self.word_and_mask(elem);
        let old = self.words[w];
        self.words[w] = old | m;
        old & m == 0
    }

    /// Removes `elem`; returns whether the set changed.
    #[inline]
    pub fn remove(&mut self, elem: T) -> bool {
        let (w, m) = self.word_and_mask(elem);
        let old = self.words[w];
        self.words[w] = old & !m;
        old & m != 0
    }

    pub fn clear(&mut self) {
        self.words.fill(0);
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// `self |= other`; returns whether `self` changed.
    pub fn union(&mut self, other: &DenseBitSet<T>) -> bool {
        assert_eq!(self.domain_size, other.domain_size);
        let mut changed = false;
        for (a, &b) in self.words.iter_mut().zip(&other.words) {
            let new = *a | b;
            changed |= new != *a;
            *a = new;
        }
        changed
    }

    /// `self &= !other`; returns whether `self` changed.
    pub fn subtract(&mut self, other: &DenseBitSet<T>) -> bool {
        assert_eq!(self.domain_size, other.domain_size);
        let mut changed = false;
        for (a, &b) in self.words.iter_mut().zip(&other.words) {
            let new = *a & !b;
            changed |= new != *a;
            *a = new;
        }
        changed
    }

    /// `self &= other`; returns whether `self` changed.
    pub fn intersect(&mut self, other: &DenseBitSet<T>) -> bool {
        assert_eq!(self.domain_size, other.domain_size);
        let mut changed = false;
        for (a, &b) in self.words.iter_mut().zip(&other.words) {
            let new = *a & b;
            changed |= new != *a;
            *a = new;
        }
        changed
    }

    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.words.iter().enumerate().flat_map(|(wi, &word)| {
            let mut w = word;
            std::iter::from_fn(move || {
                if w == 0 {
                    return None;
                }
                let bit = w.trailing_zeros() as usize;
                w &= w - 1;
                Some(T::new(wi * WORD_BITS + bit))
            })
        })
    }

    fn clear_excess_bits(&mut self) {
        let rem = self.domain_size % WORD_BITS;
        if rem != 0 {
            if let Some(last) = self.words.last_mut() {
                *last &= (1u64 << rem) - 1;
            }
        }
    }
}

impl<T: Idx> fmt::Debug for DenseBitSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}
