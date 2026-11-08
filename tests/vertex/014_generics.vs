package main

trait Describe {
    func describe(&self) -> int64
}

implement Describe for int32 {
    func describe(&self) -> int64 {
        *self as int64 * 2
    }
}

implement Describe for bool {
    func describe(&self) -> int64 {
        if *self { 1 } else { 0 }
    }
}

struct Pair<T> {
    a: T
    b: T
}

implement<T: Describe> Pair<T> {
    func total(&self) -> int64 {
        self.a.describe() + self.b.describe()
    }
}

implement<T> Pair<T> {
    func swap(self) -> Pair<T> {
        Pair { a: self.b, b: self.a }
    }
    func first(&self) -> &T {
        &self.a
    }
}

func sumAll<T: Describe>(xs: &[T]) -> int64 {
    var t = 0
    for x in xs {
        t += x.describe()
    }
    t
}

func identity<T>(x: T) -> T {
    x
}

trait Zero {
    func zero() -> Self
}

implement Zero for int32 {
    func zero() -> Self {
        0
    }
}

implement Zero for float64 {
    func zero() -> Self {
        0.5
    }
}

func make<T: Zero>() -> T {
    T.zero()
}

trait Container {
    type Item
    func get(&self, i: uint) -> Self.Item
    func size(&self) -> uint
}

struct Evens

implement Container for Evens {
    type Item = int32
    func get(&self, i: uint) -> int32 {
        i as int32 * 2
    }
    func size(&self) -> uint {
        4
    }
}

func totalOf<C: Container>(c: &C) -> int64
where
    C.Item: Describe,
{
    var t = 0
    for i in 0..c.size() {
        t += c.get(i).describe()
    }
    t
}

func main() {
    let p = Pair { a: 3, b: 4 }
    println("total {}", p.total())
    let s = p.swap()
    println("first after swap {}", s.first())
    let flags = Pair { a: true, b: false }
    println("flags {}", flags.total())
    println("sum {}", sumAll(&[1, 2, 3]))
    println("sum bools {}", sumAll(&[true, true, false]))
    println("{} {} {}", identity(5), identity("str"), identity('c'))
    let z: int32 = make()
    println("{} {}", z, make<float64>())
    println("evens {}", totalOf(&Evens))
}
