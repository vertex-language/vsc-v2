fn main() {
    let big: i64 = 300;
    println!("{} {} {}", big as u8, big as i8, -1i64 as u16);
    let neg: i32 = -5;
    println!("{} {}", neg as u32, neg as i64);
    let f = 3.7_f64;
    println!("{} {} {}", f as i32, -f as i32, f as u8);
    println!("{} {}", 1e10 as i32, -1e10 as i32);
    println!("{} {}", (0.0_f64 / 0.0) as i32, 300.0_f32 as u8);
    let u: u32 = 4_000_000_000;
    println!("{} {}", u as i32, u as f32);
    println!("{} {}", 7 as f64 / 2.0, 7 / 2);
    println!("{}", (0.1_f64 + 0.2) as f32);
    println!("{} {}", true as i32, false as u8);
    let small: u8 = 255;
    println!("{}", small as i8 as i32);
    println!("{}", !0usize as u32);
}
