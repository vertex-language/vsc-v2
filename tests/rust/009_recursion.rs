fn fib(n: u64) -> u64 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

fn ackermann(m: u64, n: u64) -> u64 {
    if m == 0 {
        n + 1
    } else if n == 0 {
        ackermann(m - 1, 1)
    } else {
        ackermann(m - 1, ackermann(m, n - 1))
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn main() {
    println!("fib(20) = {}", fib(20));
    println!("ackermann(2, 3) = {}", ackermann(2, 3));
    println!("gcd(1071, 462) = {}", gcd(1071, 462));
    let mut factorial: u64 = 1;
    for i in 1..=15u64 {
        factorial *= i;
    }
    println!("15! = {factorial}");
}
