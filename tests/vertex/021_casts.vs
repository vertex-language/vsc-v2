package main

func main() {
    let big: int64 = 300
    println("{} {} {}", big as uint8, big as int8, -1i64 as uint16)
    let neg: int32 = -5
    println("{} {}", neg as uint32, neg as int64)
    let f = 3.7_f64
    println("{} {} {}", f as int32, -f as int32, f as uint8)
    println("{} {}", 1e10 as int32, -1e10 as int32)
    println("{} {}", (0.0_f64 / 0.0) as int32, 300.0_f32 as uint8)
    let u: uint32 = 4_000_000_000
    println("{} {}", u as int32, u as float32)
    println("{} {}", 7 as float64 / 2.0, 7 / 2)
    println("{}", (0.1_f64 + 0.2) as float32)
    println("{} {}", true as int32, false as uint8)
    let small: uint8 = 255
    println("{}", small as int8 as int32)
    println("{}", !0usize as uint32)
}
