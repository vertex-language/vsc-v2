fn find(xs: &[i32], target: i32) -> Option<usize> {
    let mut i = 0;
    while i < xs.len() {
        if xs[i] == target {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn checked_div(a: i32, b: i32) -> Option<i32> {
    if b == 0 {
        return None;
    }
    Some(a / b)
}

fn compute(a: i32, b: i32, c: i32) -> Option<i32> {
    let x = checked_div(a, b)?;
    let y = checked_div(x, c)?;
    Some(x + y)
}

fn first_negative(xs: &[i32]) -> i32 {
    for x in xs {
        if *x < 0 {
            return *x;
        }
    }
    0
}

fn main() {
    let xs = [4, 8, 15, 16, 23, 42];
    println!("{:?} {:?}", find(&xs, 16), find(&xs, 5));
    println!("{:?} {:?} {:?}", compute(100, 5, 2), compute(1, 0, 2), compute(10, 2, 0));
    println!("{} {}", first_negative(&[3, -4, -5]), first_negative(&xs));
}
