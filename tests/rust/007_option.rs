fn half(n: i32) -> Option<i32> {
    if n % 2 == 0 { Some(n / 2) } else { None }
}

fn quarter(n: i32) -> Option<i32> {
    let h = half(n)?;
    half(h)
}

fn main() {
    println!("{:?} {:?} {:?}", quarter(8), quarter(6), quarter(3));
    let values: [Option<i32>; 3] = [Some(1), None, Some(3)];
    let mut sum = 0;
    for v in &values {
        if let Some(x) = v {
            sum += x;
        }
    }
    println!("sum {sum}");
    let nested: Option<Option<i32>> = Some(None);
    println!("{nested:?}");
    match quarter(16) {
        Some(q) if q > 3 => println!("big quarter {q}"),
        Some(q) => println!("quarter {q}"),
        None => println!("none"),
    }
}
