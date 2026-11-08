fn main() {
    let mut i = 0;
    let found = loop {
        i += 1;
        if i * i > 50 {
            break i;
        }
    };
    println!("first square over 50: {found}");

    let mut count = 0;
    'outer: for x in 0..10 {
        for y in 0..10 {
            if x * y > 20 {
                continue 'outer;
            }
            if x + y == 15 {
                break 'outer;
            }
            count += 1;
        }
    }
    println!("count {count}");

    let mut n = 27u64;
    let mut steps = 0;
    while n != 1 {
        n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 };
        steps += 1;
    }
    println!("collatz steps {steps}");

    let label = 'b: {
        for x in 0..5 {
            if x == 3 {
                break 'b "three";
            }
        }
        "none"
    };
    println!("{label}");
}
