//! Dereferencing traits (`core/ops`).

package ops

@lang("deref")
public trait Deref {
    type Target
    func deref(&self) -> &Self.Target
}

@lang("derefMut")
public trait DerefMut: Deref {
    func derefMut(&mut self) -> &mut <Self as Deref>.Target
}

implement<a, T> Deref for &a T {
    type Target = T
    func deref(&self) -> &T { *self }
}

implement<a, T> Deref for &a mut T {
    type Target = T
    func deref(&self) -> &T { &**self }
}

implement<a, T> DerefMut for &a mut T {
    func derefMut(&mut self) -> &mut T { &mut **self }
}
