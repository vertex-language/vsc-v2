use std::ops::Mul;

#[derive(Debug, Clone, Copy)]
struct M2 {
    a: u64,
    b: u64,
    c: u64,
    d: u64,
}

impl Mul for M2 {
    type Output = M2;
    fn mul(self, o: M2) -> M2 {
        M2 {
            a: self.a * o.a + self.b * o.c,
            b: self.a * o.b + self.b * o.d,
            c: self.c * o.a + self.d * o.c,
            d: self.c * o.b + self.d * o.d,
        }
    }
}

fn power(m: M2, mut n: u32) -> M2 {
    let mut result = M2 { a: 1, b: 0, c: 0, d: 1 };
    let mut base = m;
    while n > 0 {
        if n % 2 == 1 {
            result = result * base;
        }
        n /= 2;
        if n > 0 {
            base = base * base;
        }
    }
    result
}

fn main() {
    let fib = M2 { a: 1, b: 1, c: 1, d: 0 };
    for n in &[1, 2, 10, 50, 90] {
        println!("fib({n}) = {}", power(fib, *n).b);
    }
    println!("{:?}", fib * fib);
}
