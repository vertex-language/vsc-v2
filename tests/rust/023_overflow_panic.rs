fn factorial(n: u8) -> u8 {
    if n == 0 { 1 } else { n * factorial(n - 1) }
}

fn main() {
    for i in 0..10 {
        println!("{i}! = {}", factorial(i));
    }
}
