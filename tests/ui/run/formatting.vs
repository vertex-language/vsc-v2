//@ run-pass
// Width, fill, alignment, sign, zero padding, precision, radixes and
// `Debug`, as in Rust.

package main

struct Point { x: int32; y: int32 }
struct Meters(float64)
enum Shape { Circle(float64); Rect { w: float64; h: float64 }; Empty }

func main() {
    println("[{:>6}] [{:<6}] [{:^6}] [{:*^7}]", 12, "ab", 'c', "mid")
    println("[{:+}] [{:08.3}] [{:.0}] [{:e}]", 42, 3.14159, 2.5, 1234.5)
    println("[{:x}] [{:#X}] [{:#o}] [{:#b}] [{:#010x}]", 255, 255, 8, 5, 255)
    println("{} {} {} {}", 2.0, 0.1 + 0.2, -0.0, 1e21)
    println("{:?} {:?} {:?}", 2.0, 'x', "quote\"d")
    let p = Point { x: 3, y: -4 }
    println("{:?} {:?} {:?} {:?}", p, Meters(2.5), Shape.Rect { w: 1.0, h: 2.0 }, Shape.Empty)
    println("{p:#?}")
    println("{:?} {:?}", (1, "two", true), [1, 2, 3])
    let width = 5
    println("[{:>width$}] [{:>1$}]", 7, 8)
}
