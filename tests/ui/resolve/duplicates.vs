func f() {}
func f() {} //~ ERROR the name `f` is defined multiple times
struct S
enum S { A } //~ ERROR the name `S` is defined multiple times
func println() {} //~ ERROR `println` is a built-in name and can't be declared
