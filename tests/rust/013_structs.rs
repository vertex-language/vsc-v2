#[derive(Debug)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Debug)]
struct Rect {
    origin: Point,
    size: (i32, i32),
}

fn abs(n: i32) -> i32 {
    if n < 0 { -n } else { n }
}

impl Point {
    fn new(x: i32, y: i32) -> Point {
        Point { x, y }
    }
    fn manhattan(&self, other: &Point) -> i32 {
        abs(self.x - other.x) + abs(self.y - other.y)
    }
    fn shift(&mut self, dx: i32, dy: i32) {
        self.x += dx;
        self.y += dy;
    }
}

impl Rect {
    fn area(&self) -> i32 {
        self.size.0 * self.size.1
    }
    fn contains(&self, p: &Point) -> bool {
        p.x >= self.origin.x
            && p.x < self.origin.x + self.size.0
            && p.y >= self.origin.y
            && p.y < self.origin.y + self.size.1
    }
}

struct Wrapper(i32, bool);

struct Marker;

impl Marker {
    fn hello(&self) -> &'static str {
        "marker"
    }
}

fn main() {
    let mut p = Point::new(1, 2);
    let q = Point { x: 4, ..Point::new(0, 6) };
    println!("{:?} {:?} dist {}", p, q, p.manhattan(&q));
    p.shift(2, -1);
    println!("shifted {:?}", p);
    let r = Rect { origin: Point::new(0, 0), size: (5, 3) };
    println!("area {} contains p {} contains q {}", r.area(), r.contains(&p), r.contains(&q));
    let w = Wrapper(7, true);
    let Wrapper(n, flag) = w;
    println!("{} {}", n, flag);
    let Point { x, y: py } = q;
    println!("x {x} py {py}");
    println!("{r:?}");
    println!("{}", Marker.hello());
}
