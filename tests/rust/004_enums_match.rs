#[derive(Debug)]
enum Shape {
    Circle(f64),
    Rect { w: f64, h: f64 },
    Empty,
}

fn area(s: &Shape) -> f64 {
    match s {
        Shape::Circle(r) => 3.0 * r * r,
        Shape::Rect { w, h } => w * h,
        Shape::Empty => 0.0,
    }
}

fn classify(n: i32) -> &'static str {
    match n {
        i32::MIN..=-1 => "negative",
        0 => "zero",
        1..=9 => "small",
        _ => "large",
    }
}

fn main() {
    let shapes = [Shape::Circle(1.5), Shape::Rect { w: 2.0, h: 4.0 }, Shape::Empty];
    for s in &shapes {
        println!("{:?} has area {:.2}", s, area(s));
    }
    for n in [-5, 0, 7, 42] {
        println!("{n} is {}", classify(n));
    }
    let pair = (3, -3);
    match pair {
        (x, y) if x + y == 0 => println!("opposites"),
        (x, _) if x % 2 == 0 => println!("even first"),
        _ => println!("other"),
    }
}
