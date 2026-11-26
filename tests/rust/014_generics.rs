trait Describe {
    fn describe(&self) -> i64;
}

impl Describe for i32 {
    fn describe(&self) -> i64 {
        *self as i64 * 2
    }
}

impl Describe for bool {
    fn describe(&self) -> i64 {
        if *self { 1 } else { 0 }
    }
}

struct Pair<T> {
    a: T,
    b: T,
}

impl<T: Describe> Pair<T> {
    fn total(&self) -> i64 {
        self.a.describe() + self.b.describe()
    }
}

impl<T> Pair<T> {
    fn swap(self) -> Pair<T> {
        Pair { a: self.b, b: self.a }
    }
    fn first(&self) -> &T {
        &self.a
    }
}

fn sum_all<T: Describe>(xs: &[T]) -> i64 {
    let mut t = 0;
    for x in xs {
        t += x.describe();
    }
    t
}

fn identity<T>(x: T) -> T {
    x
}

trait Zero {
    fn zero() -> Self;
}

impl Zero for i32 {
    fn zero() -> Self {
        0
    }
}

impl Zero for f64 {
    fn zero() -> Self {
        0.5
    }
}

fn make<T: Zero>() -> T {
    T::zero()
}

trait Container {
    type Item;
    fn get(&self, i: usize) -> Self::Item;
    fn size(&self) -> usize;
}

struct Evens;

impl Container for Evens {
    type Item = i32;
    fn get(&self, i: usize) -> i32 {
        i as i32 * 2
    }
    fn size(&self) -> usize {
        4
    }
}

fn total_of<C: Container>(c: &C) -> i64
where
    C::Item: Describe,
{
    let mut t = 0;
    for i in 0..c.size() {
        t += c.get(i).describe();
    }
    t
}

fn main() {
    let p = Pair { a: 3, b: 4 };
    println!("total {}", p.total());
    let s = p.swap();
    println!("first after swap {}", s.first());
    let flags = Pair { a: true, b: false };
    println!("flags {}", flags.total());
    println!("sum {}", sum_all(&[1, 2, 3]));
    println!("sum bools {}", sum_all(&[true, true, false]));
    println!("{} {} {}", identity(5), identity("str"), identity('c'));
    let z: i32 = make();
    println!("{} {}", z, make::<f64>());
    println!("evens {}", total_of(&Evens));
}
