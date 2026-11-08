package main

struct V2 {
    x: int32
    y: int32
}

implement Add for V2 {
    type Output = V2
    func add(self, o: V2) -> V2 {
        V2 { x: self.x + o.x, y: self.y + o.y }
    }
}

implement Sub for V2 {
    type Output = V2
    func sub(self, o: V2) -> V2 {
        V2 { x: self.x - o.x, y: self.y - o.y }
    }
}

implement Mul<int32> for V2 {
    type Output = V2
    func mul(self, k: int32) -> V2 {
        V2 { x: self.x * k, y: self.y * k }
    }
}

implement Neg for V2 {
    type Output = V2
    func neg(self) -> V2 {
        V2 { x: -self.x, y: -self.y }
    }
}

implement AddAssign for V2 {
    func addAssign(&mut self, o: V2) {
        self.x += o.x
        self.y += o.y
    }
}

implement PartialEq for V2 {
    func eq(&self, o: &V2) -> bool {
        self.x == o.x && self.y == o.y
    }
}

implement PartialOrd for V2 {
    func partialCmp(&self, o: &V2) -> Option<Ordering> {
        let a = self.x * self.x + self.y * self.y
        let b = o.x * o.x + o.y * o.y
        a.partialCmp(&b)
    }
}

func main() {
    let a = V2 { x: 1, y: 2 }
    let b = V2 { x: 3, y: -4 }
    println("{:?}", a + b)
    println("{:?}", a - b)
    println("{:?}", b * 3)
    println("{:?}", -a)
    var c = a
    c += b
    c += V2 { x: 10, y: 10 }
    println("{:?}", c)
    println("{} {} {}", a == a, a == b, a != b)
    println("{} {} {} {}", a < b, a <= b, a > b, a >= b)
    println("{:?}", a.partialCmp(&b))
    var total = V2 { x: 0, y: 0 }
    for v in &[a, b, c] {
        total = total + *v
    }
    println("{:?}", total)
}
