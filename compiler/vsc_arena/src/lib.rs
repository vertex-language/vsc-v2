//! Typed arenas (rustc's `rustc_arena`).
//!
//! An arena hands out references that live as long as the arena itself, so
//! interned types can be plain `&'tcx` references compared by address. Values
//! are stored in chunks that never move; a full chunk is kept and a bigger one
//! started. Everything is dropped together when the arena is.

use std::cell::RefCell;

const FIRST_CHUNK: usize = 64;
const MAX_CHUNK_BYTES: usize = 2 * 1024 * 1024;

/// An arena of `T`s.
///
/// Each chunk is a `Vec` that is never pushed past its capacity, so it never
/// reallocates and references into it stay valid. The arena has no `Drop`
/// impl of its own: the chunks' `Vec`s drop the values, and since std's
/// `Vec` allows its elements to dangle while dropping, a context can hold
/// `&'tcx TypedArena<T<'tcx>>` (rustc needs the unstable `#[may_dangle]`
/// for this).
pub struct TypedArena<T> {
    chunks: RefCell<Vec<Vec<T>>>,
}

impl<T> Default for TypedArena<T> {
    fn default() -> Self {
        TypedArena { chunks: RefCell::new(Vec::new()) }
    }
}

impl<T> TypedArena<T> {
    /// A chunk with room for `n` more values.
    fn chunk_with_room(chunks: &mut Vec<Vec<T>>, n: usize) -> &mut Vec<T> {
        let needs_new = match chunks.last() {
            Some(c) => c.capacity() - c.len() < n,
            None => true,
        };
        if needs_new {
            let elem = size_of::<T>().max(1);
            let prev = chunks.last().map_or(FIRST_CHUNK / 2, |c| c.capacity());
            let cap = (prev * 2).min((MAX_CHUNK_BYTES / elem).max(1)).max(n);
            chunks.push(Vec::with_capacity(cap));
        }
        chunks.last_mut().unwrap()
    }

    #[allow(clippy::mut_from_ref)]
    pub fn alloc(&self, value: T) -> &mut T {
        let mut chunks = self.chunks.borrow_mut();
        let chunk = Self::chunk_with_room(&mut chunks, 1);
        let i = chunk.len();
        chunk.push(value);
        // SAFETY: the push didn't reallocate (there was room), and the chunk
        // is never pushed past its capacity or shrunk, so the element stays
        // where it is for as long as the arena lives. Each slot is handed
        // out once.
        unsafe { &mut *chunk.as_mut_ptr().add(i) }
    }

    /// The values of `iter`, stored contiguously.
    #[allow(clippy::mut_from_ref)]
    pub fn alloc_from_iter(&self, iter: impl IntoIterator<Item = T>) -> &mut [T] {
        // Collect first: the iterator may itself allocate in this arena.
        let values: Vec<T> = iter.into_iter().collect();
        if values.is_empty() {
            return &mut [];
        }
        let n = values.len();
        let mut chunks = self.chunks.borrow_mut();
        let chunk = Self::chunk_with_room(&mut chunks, n);
        let start = chunk.len();
        chunk.extend(values);
        // SAFETY: as in `alloc`; the `n` values are contiguous because the
        // chunk had room for all of them.
        unsafe { std::slice::from_raw_parts_mut(chunk.as_mut_ptr().add(start), n) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_stay_valid_across_chunks() {
        let arena = TypedArena::default();
        let refs: Vec<&mut String> = (0..1000).map(|i| arena.alloc(i.to_string())).collect();
        for (i, r) in refs.iter().enumerate() {
            assert_eq!(**r, i.to_string());
        }
        let ints = TypedArena::default();
        let s = ints.alloc_from_iter((0..300).map(|i| i * 2));
        assert_eq!(s.len(), 300);
        assert_eq!(s[299], 598);
    }

    #[test]
    fn drops_everything() {
        use std::rc::Rc;
        let counter = Rc::new(());
        {
            let arena = TypedArena::default();
            for _ in 0..200 {
                arena.alloc(counter.clone());
            }
            assert_eq!(Rc::strong_count(&counter), 201);
        }
        assert_eq!(Rc::strong_count(&counter), 1);
    }
}
