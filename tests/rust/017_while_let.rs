fn main() {
    let mut stack = Vec::new();
    for i in 1..=5 {
        stack.push(i * i);
    }
    while let Some(top) = stack.pop() {
        print!("{top} ");
    }
    println!();

    let mut fib: Vec<u64> = vec![0, 1];
    let mut prev = 0;
    let mut cur = 1;
    while fib.len() < 15 {
        let next = prev + cur;
        prev = cur;
        cur = next;
        fib.push(next);
    }
    println!("{:?}", fib);

    let mut digits = Vec::new();
    let mut n = 90817u32;
    while n > 0 {
        digits.push(n % 10);
        n /= 10;
    }
    println!("{:?}", digits);
    let mut sum = 0;
    while let Some(d) = digits.pop() {
        if d == 0 {
            continue;
        }
        sum += d;
    }
    println!("digit sum {sum}");

    let mut opt = Some(3);
    while let Some(i) = opt {
        println!("countdown {i}");
        opt = if i > 1 { Some(i - 1) } else { None };
    }
}
