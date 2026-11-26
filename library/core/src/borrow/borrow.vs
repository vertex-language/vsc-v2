//! A module for working with borrowed data (`core/borrow`).

package borrow

/// A trait for borrowing data.
public trait Borrow<Borrowed> {
    func borrow(&self) -> &Borrowed
}

/// A trait for mutably borrowing data.
public trait BorrowMut<Borrowed>: Borrow<Borrowed> {
    func borrowMut(&mut self) -> &mut Borrowed
}

implement<T> Borrow<T> for T {
    func borrow(&self) -> &T { self }
}

implement<T> BorrowMut<T> for T {
    func borrowMut(&mut self) -> &mut T { self }
}

implement<a, T> Borrow<T> for &a T {
    func borrow(&self) -> &T { *self }
}

implement<a, T> Borrow<T> for &a mut T {
    func borrow(&self) -> &T { &**self }
}

implement<a, T> BorrowMut<T> for &a mut T {
    func borrowMut(&mut self) -> &mut T { &mut **self }
}
