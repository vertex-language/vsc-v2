// The lang items tests need, until vsc-v2 can build `library/` (rustc's
// tests/auxiliary/minicore.rs). A test with `//@ add-core-stubs` gets this
// file in its package. Functions marked `@intrinsic` are carried out by the
// MIR interpreter; their bodies are stubs. The rest is ordinary code.

package main

@lang("String")
public struct String { len: uint }

@lang("CString")
public struct CString { len: uint }

@lang("CStr")
public struct CStr { len: uint }

@lang("Class")
public struct Class<T> { inner: T }

@lang("Func")
public struct Func<F> { f: F }

@lang("c_void")
public enum CVoid { Variant1; Variant2 }

public enum Infallible {}

public enum Option<T> {
    @lang("None") None
    @lang("Some") Some(T)
}

@lang("Range")
public struct Range<T> { start: T; end: T }

@lang("RangeFrom")
public struct RangeFrom<T> { start: T }

@lang("RangeTo")
public struct RangeTo<T> { end: T }

@lang("RangeFull")
public struct RangeFull

@lang("RangeToInclusive")
public struct RangeToInclusive<T> { end: T }

public struct RangeInclusive<T> { start: T; end: T; exhausted: bool }

implement<T> RangeInclusive<T> {
    @lang("rangeInclusiveNew")
    public func new(start: T, end: T) -> RangeInclusive<T> { RangeInclusive { start, end, exhausted: false } }
}

// Iteration.

public trait Iterator {
    type Item
    @lang("next")
    func next(&mut self) -> Option<Self.Item>
}

public trait IntoIterator {
    type Item
    type IntoIter
    @lang("intoIter")
    func intoIter(self) -> Self.IntoIter
}

implement<I: Iterator> IntoIterator for I {
    type Item = I.Item
    type IntoIter = I
    func intoIter(self) -> I { self }
}

implement<T> Iterator for Range<T> {
    type Item = T
    @intrinsic("range_next")
    func next(&mut self) -> Option<T> { loop {} }
}

implement<T> Iterator for RangeInclusive<T> {
    type Item = T
    @intrinsic("range_inclusive_next")
    func next(&mut self) -> Option<T> { loop {} }
}

@intrinsic("slice_len")
public func sliceLen<T>(s: &[T]) -> uint { loop {} }

public struct SliceIter<a, T> { s: &a [T]; i: uint }

implement<a, T> Iterator for SliceIter<a, T> {
    type Item = &a T
    func next(&mut self) -> Option<&a T> {
        if self.i < sliceLen(self.s) {
            let x = &self.s[self.i]
            self.i += 1
            Option.Some(x)
        } else {
            Option.None
        }
    }
}

implement<a, T, const N: uint> IntoIterator for &a [T; N] {
    type Item = &a T
    type IntoIter = SliceIter<a, T>
    func intoIter(self) -> SliceIter<a, T> { SliceIter { s: self, i: 0 } }
}

implement<a, T> IntoIterator for &a [T] {
    type Item = &a T
    type IntoIter = SliceIter<a, T>
    func intoIter(self) -> SliceIter<a, T> { SliceIter { s: self, i: 0 } }
}

// `?`

public enum ControlFlow<B, C> {
    @lang("Continue") Continue(C)
    @lang("Break") Break(B)
}

public trait FromResidual<R> {
    @lang("fromResidual")
    func fromResidual(residual: R) -> Self
}

public trait Try {
    type Output
    type Residual
    @lang("branch")
    func branch(self) -> ControlFlow<Self.Residual, Self.Output>
}

implement<T> Try for Option<T> {
    type Output = T
    type Residual = Option<Infallible>
    func branch(self) -> ControlFlow<Option<Infallible>, T> {
        match self {
            Option.Some(v) => ControlFlow.Continue(v)
            Option.None => ControlFlow.Break(Option.None)
        }
    }
}

implement<T> FromResidual<Option<Infallible>> for Option<T> {
    func fromResidual(residual: Option<Infallible>) -> Option<T> { Option.None }
}

// `vec[…]`

public struct Vec<T> { ptr: *mut T; len: uint }

implement<T> Vec<T> {
    @intrinsic("vec_len")
    public func len(&self) -> uint { loop {} }
    @intrinsic("vec_push")
    public func push(&mut self, value: T) {}
    @intrinsic("vec_pop")
    public func pop(&mut self) -> Option<T> { loop {} }
}

@lang("vecNew")
public func vecNew<T>() -> Vec<T> { loop {} }

@lang("vecFromArray")
public func vecFromArray<T, const N: uint>(items: [T; N]) -> Vec<T> { loop {} }

@lang("vecFromElem")
public func vecFromElem<T>(elem: T, n: uint) -> Vec<T> { loop {} }

// Format strings and the built-ins.

@lang("formatArguments")
public struct FormatArguments { pieces: uint }

implement FormatArguments {
    @intrinsic("format_new_const")
    public func newConst(pieces: &[&string]) -> FormatArguments { loop {} }
    @intrinsic("format_new_v1")
    public func newV1(pieces: &[&string], args: &[FormatArgument]) -> FormatArguments { loop {} }
    @intrinsic("format_new_v1_formatted")
    public func newV1Formatted(pieces: &[&string], args: &[FormatArgument], placeholders: &[FormatPlaceholder]) -> FormatArguments { loop {} }
}

@lang("formatArgument")
public struct FormatArgument { value: *const uint8 }

implement FormatArgument {
    @intrinsic("format_arg_display")
    public func newDisplay<T>(x: &T) -> FormatArgument { loop {} }
    @intrinsic("format_arg_debug")
    public func newDebug<T>(x: &T) -> FormatArgument { loop {} }
    @intrinsic("format_arg_lower_exp")
    public func newLowerExp<T>(x: &T) -> FormatArgument { loop {} }
    @intrinsic("format_arg_upper_exp")
    public func newUpperExp<T>(x: &T) -> FormatArgument { loop {} }
    @intrinsic("format_arg_octal")
    public func newOctal<T>(x: &T) -> FormatArgument { loop {} }
    @intrinsic("format_arg_pointer")
    public func newPointer<T>(x: &T) -> FormatArgument { loop {} }
    @intrinsic("format_arg_binary")
    public func newBinary<T>(x: &T) -> FormatArgument { loop {} }
    @intrinsic("format_arg_lower_hex")
    public func newLowerHex<T>(x: &T) -> FormatArgument { loop {} }
    @intrinsic("format_arg_upper_hex")
    public func newUpperHex<T>(x: &T) -> FormatArgument { loop {} }
    @intrinsic("format_arg_usize")
    public func fromUsize(x: &uint) -> FormatArgument { loop {} }
}

@lang("formatPlaceholder")
public struct FormatPlaceholder { position: uint }

implement FormatPlaceholder {
    @intrinsic("format_placeholder_new")
    public func new(position: uint, fill: char, align: FormatAlignment, flags: uint32, precision: FormatCount, width: FormatCount) -> FormatPlaceholder { loop {} }
}

@lang("formatCount")
public enum FormatCount { Is(uint); Param(uint); Implied }

@lang("formatAlignment")
public enum FormatAlignment { Left; Right; Center; Unknown }

@lang("print")
public func printArgs(args: FormatArguments) {}

@lang("eprint")
public func eprintArgs(args: FormatArguments) {}

@lang("format")
public func formatArgs(args: FormatArguments) -> string { loop {} }

@lang("panicFmt")
public func panicFmt(args: FormatArguments) -> never { loop {} }

@lang("assertKind")
public enum AssertKind { Eq; Ne }

@lang("assertFailed")
public func assertFailed<T, U>(kind: AssertKind, left: &T, right: &U, msg: Option<FormatArguments>) -> never { loop {} }

// The function traits.

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
