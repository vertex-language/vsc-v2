fn main() {
    let a: i32 = -17;
    let b: i32 = 5;
    println!("{} {} {} {} {}", a + b, a - b, a * b, a / b, a % b);
    let x: u8 = 200;
    println!("{} {} {}", x as i8, -1i32 as u32, 300i32 as u8);
    println!("{} {}", i64::MAX as f64, 3.99f64 as i32);
    println!("{} {} {}", 1u32 << 5, 0xF0u8 >> 4, !0u16);
    println!("{} {}", -7i32 >> 1, 7 & 3 | 8 ^ 1);
    println!("{} {}", 'A' as u32, 97u8 as char);
    let big: u64 = 1 << 40;
    println!("{big}");
}
