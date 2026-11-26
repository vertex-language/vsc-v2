fn apply_twice<F: Fn(i32) -> i32>(f: F, x: i32) -> i32 {
    f(f(x))
}

fn call_mut<F: FnMut()>(mut f: F, times: u32) {
    for _ in 0..times {
        f();
    }
}

fn main() {
    let add3 = |x| x + 3;
    println!("{}", apply_twice(add3, 10));
    let factor = 4;
    println!("{}", apply_twice(|x| x * factor, 2));
    let mut count = 0;
    call_mut(|| count += 2, 5);
    println!("count {count}");
    let mut log = Vec::new();
    let mut record = |n: i32| log.push(n * n);
    record(3);
    record(4);
    println!("{:?}", log);
    let pick = |flag: bool| if flag { "yes" } else { "no" };
    println!("{} {}", pick(true), pick(false));
}
