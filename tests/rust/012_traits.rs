trait Shape {
    fn area(&self) -> f64;
    fn name(&self) -> &'static str;
    fn describe(&self) -> f64 {
        self.area() * 2.0
    }
}

struct Square {
    side: f64,
}

struct Circle {
    r: f64,
}

impl Shape for Square {
    fn area(&self) -> f64 {
        self.side * self.side
    }
    fn name(&self) -> &'static str {
        "square"
    }
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        3.0 * self.r * self.r
    }
    fn name(&self) -> &'static str {
        "circle"
    }
    fn describe(&self) -> f64 {
        -1.0
    }
}

fn report<T: Shape>(s: &T) {
    println!("{} area {} describe {}", s.name(), s.area(), s.describe());
}

struct Counter {
    n: u32,
}

impl Counter {
    fn new() -> Counter {
        Counter { n: 0 }
    }
    fn tick(&mut self) -> u32 {
        self.n += 1;
        self.n
    }
}

fn main() {
    report(&Square { side: 3.0 });
    report(&Circle { r: 2.0 });
    let mut c = Counter::new();
    c.tick();
    c.tick();
    println!("ticks {}", c.tick());
}
