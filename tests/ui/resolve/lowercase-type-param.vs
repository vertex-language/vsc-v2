// A lowercase generic parameter is a lifetime (exception 11), so using it
// as a type is explained.

package main

func id<t>(x: t) -> t { x }
//~^ ERROR cannot find type `t` in this scope
//~| ERROR cannot find type `t` in this scope
