use std::cmp::Ordering;
use std::ops::{Add, AddAssign, Mul, Neg, Sub};

#[derive(Debug, Clone, Copy)]
struct V2 {
    x: i32,
    y: i32,
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        V2 { x: self.x + o.x, y: self.y + o.y }
    }
}

impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        V2 { x: self.x - o.x, y: self.y - o.y }
    }
}

impl Mul<i32> for V2 {
    type Output = V2;
    fn mul(self, k: i32) -> V2 {
        V2 { x: self.x * k, y: self.y * k }
    }
}

impl Neg for V2 {
    type Output = V2;
    fn neg(self) -> V2 {
        V2 { x: -self.x, y: -self.y }
    }
}

impl AddAssign for V2 {
    fn add_assign(&mut self, o: V2) {
        self.x += o.x;
        self.y += o.y;
    }
}

impl PartialEq for V2 {
    fn eq(&self, o: &V2) -> bool {
        self.x == o.x && self.y == o.y
    }
}

impl PartialOrd for V2 {
    fn partial_cmp(&self, o: &V2) -> Option<Ordering> {
        let a = self.x * self.x + self.y * self.y;
        let b = o.x * o.x + o.y * o.y;
        a.partial_cmp(&b)
    }
}

fn main() {
    let a = V2 { x: 1, y: 2 };
    let b = V2 { x: 3, y: -4 };
    println!("{:?}", a + b);
    println!("{:?}", a - b);
    println!("{:?}", b * 3);
    println!("{:?}", -a);
    let mut c = a;
    c += b;
    c += V2 { x: 10, y: 10 };
    println!("{:?}", c);
    println!("{} {} {}", a == a, a == b, a != b);
    println!("{} {} {} {}", a < b, a <= b, a > b, a >= b);
    println!("{:?}", a.partial_cmp(&b));
    let mut total = V2 { x: 0, y: 0 };
    for v in &[a, b, c] {
        total = total + *v;
    }
    println!("{:?}", total);
}
