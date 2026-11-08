// Lang items a signature needs must be defined, once, on the right kind
// of item.

package main

func f(s: string) {} //~ ERROR requires the `String` lang item

@lang("Strng") //~ ERROR definition of an unknown lang item: `Strng`
struct A {}

@lang("Class") //~ ERROR `Class` lang item must be applied to a struct
func g() {}

@lang("Func")
struct F1<F> { f: F }

@lang("Func") //~ ERROR found duplicate lang item `Func`
struct F2<F> { f: F }
