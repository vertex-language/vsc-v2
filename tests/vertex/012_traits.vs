package main

trait Shape {
    func area(&self) -> float64
    func name(&self) -> &static string
    func describe(&self) -> float64 {
        self.area() * 2.0
    }
}

struct Square {
    side: float64
}

struct Circle {
    r: float64
}

implement Shape for Square {
    func area(&self) -> float64 {
        self.side * self.side
    }
    func name(&self) -> &static string {
        "square"
    }
}

implement Shape for Circle {
    func area(&self) -> float64 {
        3.0 * self.r * self.r
    }
    func name(&self) -> &static string {
        "circle"
    }
    func describe(&self) -> float64 {
        -1.0
    }
}

func report<T: Shape>(s: &T) {
    println("{} area {} describe {}", s.name(), s.area(), s.describe())
}

struct Counter {
    n: uint32
}

implement Counter {
    func new() -> Counter {
        Counter { n: 0 }
    }
    func tick(&mut self) -> uint32 {
        self.n += 1
        self.n
    }
}

func main() {
    report(&Square { side: 3.0 })
    report(&Circle { r: 2.0 })
    var c = Counter.new()
    c.tick()
    c.tick()
    println("ticks {}", c.tick())
}
