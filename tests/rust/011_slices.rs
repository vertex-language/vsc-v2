fn sum(xs: &[i64]) -> i64 {
    let mut total = 0;
    for x in xs {
        total += x;
    }
    total
}

fn describe(xs: &[i64]) -> String {
    match xs {
        [] => format!("empty"),
        [one] => format!("one: {one}"),
        [first, .., last] => format!("{first}..{last} ({} items)", xs.len()),
    }
}

fn main() {
    let a = [3, 1, 4, 1, 5, 9, 2, 6];
    println!("sum {}", sum(&a));
    println!("{}", describe(&a));
    println!("{}", describe(&[7]));
    println!("{}", describe(&[]));
    let mut grid = [[0; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            grid[r][c] = r * 3 + c;
        }
    }
    println!("{grid:?}");
    let [x, y, z] = [10, 20, 30];
    println!("{}", x + y + z);
}
