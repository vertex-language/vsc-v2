package main

func main() {
    let a: int32 = -17
    let b: int32 = 5
    println("{} {} {} {} {}", a + b, a - b, a * b, a / b, a % b)
    let x: uint8 = 200
    println("{} {} {}", x as int8, -1 as uint32, 300 as uint8)
    let max: int64 = 9223372036854775807
    println("{} {}", max as float64, 3.99 as int32)
    println("{} {} {}", 1u32 << 5, 0xF0u8 >> 4, !0u16)
    println("{} {}", -7i32 >> 1, 7 & 3 | 8 ^ 1)
    println("{} {}", 'A' as uint32, 97u8 as char)
    let big: uint64 = 1 << 40
    println("{big}")
}
