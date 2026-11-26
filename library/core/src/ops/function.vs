//! Callable traits (`core/ops`).

package ops

@lang("fnOnce")
public trait FnOnce<Args> {
    type Output
    func callOnce(self, args: Args) -> Self.Output
}

@lang("fnMut")
public trait FnMut<Args>: FnOnce<Args> {
    func callMut(&mut self, args: Args) -> <Self as FnOnce<Args>>.Output
}

@lang("fn")
public trait Fn<Args>: FnMut<Args> {
    func call(&self, args: Args) -> <Self as FnOnce<Args>>.Output
}

implement<a, A, F: Fn<A>> FnOnce<A> for &a F {
    type Output = <F as FnOnce<A>>.Output
    func callOnce(self, args: A) -> <F as FnOnce<A>>.Output { (*self).call(args) }
}

implement<a, A, F: Fn<A>> FnMut<A> for &a F {
    func callMut(&mut self, args: A) -> <F as FnOnce<A>>.Output { (**self).call(args) }
}

implement<a, A, F: Fn<A>> Fn<A> for &a F {
    func call(&self, args: A) -> <F as FnOnce<A>>.Output { (**self).call(args) }
}

implement<a, A, F: FnMut<A>> FnOnce<A> for &a mut F {
    type Output = <F as FnOnce<A>>.Output
    func callOnce(self, args: A) -> <F as FnOnce<A>>.Output { (*self).callMut(args) }
}

implement<a, A, F: FnMut<A>> FnMut<A> for &a mut F {
    func callMut(&mut self, args: A) -> <F as FnOnce<A>>.Output { (**self).callMut(args) }
}
