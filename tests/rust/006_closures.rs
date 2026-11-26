fn apply(f: impl Fn(i64) -> i64, x: i64) -> i64 {
    f(x)
}

fn main() {
    let offset = 10;
    let add = |x: i64| x + offset;
    println!("{}", apply(add, 5));
    println!("{}", apply(|x| x * x, 7));

    let mut total = 0;
    let mut accumulate = |n: i32| total += n;
    accumulate(3);
    accumulate(4);
    println!("total {total}");

    let compose = |a: i32| move |b: i32| a * 10 + b;
    println!("{}", compose(4)(2));
}
