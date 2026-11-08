//! A hash map that remembers insertion order (the job `indexmap` does in rustc).
//!
//! Entries live in a `Vec`; a hash map from key to position finds them. Removal
//! is not supported, which is all the compiler needs: it builds tables and then
//! reads them in a deterministic order.

use std::borrow::Borrow;
use std::fmt;
use std::hash::Hash;

use crate::fx::FxHashMap;

#[derive(Clone)]
pub struct FxIndexMap<K, V> {
    entries: Vec<(K, V)>,
    positions: FxHashMap<K, usize>,
}

impl<K: Hash + Eq + Clone, V> FxIndexMap<K, V> {
    pub fn new() -> Self {
        FxIndexMap { entries: Vec::new(), positions: FxHashMap::default() }
    }

    /// Inserts or replaces. Returns the entry's position and the old value.
    pub fn insert_full(&mut self, key: K, value: V) -> (usize, Option<V>) {
        if let Some(&i) = self.positions.get(&key) {
            let old = std::mem::replace(&mut self.entries[i].1, value);
            (i, Some(old))
        } else {
            let i = self.entries.len();
            self.positions.insert(key.clone(), i);
            self.entries.push((key, value));
            (i, None)
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.insert_full(key, value).1
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.positions.get(key).map(|&i| &self.entries[i].1)
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.positions.get(key).map(|&i| &mut self.entries[i].1)
    }

    pub fn get_index_of<Q>(&self, key: &Q) -> Option<usize>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.positions.get(key).copied()
    }

    pub fn get_index(&self, index: usize) -> Option<(&K, &V)> {
        self.entries.get(index).map(|(k, v)| (k, v))
    }

    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.positions.contains_key(key)
    }

    pub fn entry_or_insert_with(&mut self, key: K, f: impl FnOnce() -> V) -> &mut V {
        let i = match self.positions.get(&key) {
            Some(&i) => i,
            None => {
                let i = self.entries.len();
                self.positions.insert(key.clone(), i);
                self.entries.push((key, f()));
                i
            }
        };
        &mut self.entries[i].1
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&K, &V)> + ExactSizeIterator {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &K> + ExactSizeIterator {
        self.entries.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl DoubleEndedIterator<Item = &V> + ExactSizeIterator {
        self.entries.iter().map(|(_, v)| v)
    }
}

impl<K: Hash + Eq + Clone, V> Default for FxIndexMap<K, V> {
    fn default() -> Self {
        FxIndexMap::new()
    }
}

impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for FxIndexMap<K, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.entries.iter().map(|(k, v)| (k, v))).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_insertion_order() {
        let mut m = FxIndexMap::new();
        m.insert("c", 3);
        m.insert("a", 1);
        m.insert("b", 2);
        assert_eq!(m.insert("a", 10), Some(1));
        assert_eq!(m.keys().copied().collect::<Vec<_>>(), vec!["c", "a", "b"]);
        assert_eq!(m.get("a"), Some(&10));
        assert_eq!(m.get_index_of("b"), Some(2));
    }
}
