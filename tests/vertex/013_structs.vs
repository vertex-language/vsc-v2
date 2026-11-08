package main

struct Point {
    x: int32
    y: int32
}

struct Rect {
    origin: Point
    size: (int32, int32)
}

func abs(n: int32) -> int32 {
    if n < 0 { -n } else { n }
}

implement Point {
    func new(x: int32, y: int32) -> Point {
        Point { x, y }
    }
    func manhattan(&self, other: &Point) -> int32 {
        abs(self.x - other.x) + abs(self.y - other.y)
    }
    func shift(&mut self, dx: int32, dy: int32) {
        self.x += dx
        self.y += dy
    }
}

implement Rect {
    func area(&self) -> int32 {
        self.size.0 * self.size.1
    }
    func contains(&self, p: &Point) -> bool {
        p.x >= self.origin.x
            && p.x < self.origin.x + self.size.0
            && p.y >= self.origin.y
            && p.y < self.origin.y + self.size.1
    }
}

struct Wrapper(int32, bool)

struct Marker

implement Marker {
    func hello(&self) -> &static string {
        "marker"
    }
}

func main() {
    var p = Point.new(1, 2)
    let q = Point { x: 4, ..Point.new(0, 6) }
    println("{:?} {:?} dist {}", p, q, p.manhattan(&q))
    p.shift(2, -1)
    println("shifted {:?}", p)
    let r = Rect { origin: Point.new(0, 0), size: (5, 3) }
    println("area {} contains p {} contains q {}", r.area(), r.contains(&p), r.contains(&q))
    let w = Wrapper(7, true)
    let Wrapper(n, flag) = w
    println("{} {}", n, flag)
    let Point { x, y: py } = q
    println("x {x} py {py}")
    println("{r:?}")
    let marker = Marker
    println("{}", marker.hello())
}
