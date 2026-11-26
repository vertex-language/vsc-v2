// Bindings in patterns.

struct Meters(float64)
enum Pair { Two(int64, int64) }

func f(p: Pair) {
    let (x, x) = (1, 2) //~ ERROR identifier `x` is bound more than once in the same pattern
    let Meters = 1 //~ ERROR bindings cannot shadow tuple structs
    match p {
        Pair.Two(a, 0) | Pair.Two(0, b) => {}
        //~^ ERROR variable `a` is not bound in all patterns
        //~| ERROR variable `b` is not bound in all patterns
    }
}
