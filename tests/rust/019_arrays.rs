fn transpose(m: [[i32; 3]; 2]) -> [[i32; 2]; 3] {
    let mut t = [[0; 2]; 3];
    for r in 0..2 {
        for c in 0..3 {
            t[c][r] = m[r][c];
        }
    }
    t
}

fn bubble_sort(xs: &mut [i32]) {
    let n = xs.len();
    for i in 0..n {
        for j in 0..n - 1 - i {
            if xs[j] > xs[j + 1] {
                let tmp = xs[j];
                xs[j] = xs[j + 1];
                xs[j + 1] = tmp;
            }
        }
    }
}

fn main() {
    let m = [[1, 2, 3], [4, 5, 6]];
    println!("{:?}", transpose(m));
    let mut data = [5, -1, 8, 3, 0, 2];
    bubble_sort(&mut data);
    println!("{:?}", data);
    let mut squares = [0u64; 8];
    for i in 0..8 {
        squares[i] = (i * i) as u64;
    }
    println!("{:?}", squares);
    let mut total = 0;
    for s in &squares {
        total += *s;
    }
    println!("total {total}");
    let words = ["alpha", "beta", "gamma"];
    for i in 0..words.len() {
        print!("{i}:{} ", words[i]);
    }
    println!();
    let mut counts = [0; 5];
    for v in &[1, 3, 3, 4, 1, 1, 0] {
        counts[*v] += 1;
    }
    println!("{:?}", counts);
    let first = &data[0];
    println!("first {first} len {}", data.len());
}
