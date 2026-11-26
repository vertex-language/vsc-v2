//! A fast, non-cryptographic hasher for compiler-internal keys.
//!
//! The algorithm is Firefox's "FxHash", which rustc also uses: for every word,
//! `hash = (hash.rotate_left(5) ^ word) * K`. It is fast on small integer-like
//! keys (indices, interned symbols), which is most of what a compiler hashes.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

pub type FxBuildHasher = BuildHasherDefault<FxHasher>;
pub type FxHashMap<K, V> = HashMap<K, V, FxBuildHasher>;
pub type FxHashSet<T> = HashSet<T, FxBuildHasher>;

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

impl FxHasher {
    #[inline]
    fn add_to_hash(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while bytes.len() >= 8 {
            self.add_to_hash(u64::from_le_bytes(bytes[..8].try_into().unwrap()));
            bytes = &bytes[8..];
        }
        if bytes.len() >= 4 {
            self.add_to_hash(u32::from_le_bytes(bytes[..4].try_into().unwrap()) as u64);
            bytes = &bytes[4..];
        }
        for &b in bytes {
            self.add_to_hash(b as u64);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add_to_hash(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add_to_hash(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_works() {
        let mut m: FxHashMap<u32, &str> = FxHashMap::default();
        m.insert(1, "one");
        m.insert(2, "two");
        assert_eq!(m.get(&1), Some(&"one"));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn distinct_inputs_hash_differently() {
        let h = |s: &str| {
            let mut x = FxHasher::default();
            x.write(s.as_bytes());
            x.finish()
        };
        assert_ne!(h("func"), h("fund"));
        assert_ne!(h("abcdefgh1"), h("abcdefgh2"));
    }
}
