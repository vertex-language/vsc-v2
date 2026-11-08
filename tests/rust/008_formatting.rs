#[derive(Debug)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(Debug)]
struct Meters(f64);

fn main() {
    println!("[{:>6}] [{:<6}] [{:^6}] [{:*^7}]", 12, "ab", 'c', "mid");
    println!("[{:+}] [{:08.3}] [{:05}] [{:<5}]", 42, 3.14159, -42, true);
    println!("[{:x}] [{:#X}] [{:#o}] [{:#b}] [{:#010x}]", 255, 255, 8, 5, 255);
    println!("{:?} {:?} {:?}", 'x', "quote\"d\n", "tab\t");
    let p = Point { x: 3, y: -4 };
    println!("{:?} {:?}", p, Meters(2.5));
    println!("{p:#?}");
    println!("{:?} {:?} {:?}", (1, "two", true), [1, 2, 3], (5,));
    let width = 5;
    println!("[{:>width$}] [{:>1$}] [{:.*}]", 7, 8, 2, 1.23456);
    println!("{{literal braces}} {}", 1);
}
