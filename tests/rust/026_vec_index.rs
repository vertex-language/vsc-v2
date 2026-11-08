fn insertion_sort(v: &mut Vec<i32>) {
    for i in 1..v.len() {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
    }
}

fn prefix_sums(v: &Vec<i32>) -> Vec<i32> {
    let mut out = vec![0; v.len() + 1];
    for i in 0..v.len() {
        out[i + 1] = out[i] + v[i];
    }
    out
}

fn main() {
    let mut v = vec![9, 4, 7, 1, 8, 2];
    insertion_sort(&mut v);
    println!("{:?}", v);
    let p = prefix_sums(&v);
    println!("{:?}", p);
    println!("sum of v[1..4] = {}", p[4] - p[1]);

    let mut grid = vec![vec![0; 4]; 3];
    for r in 0..3 {
        for c in 0..4 {
            grid[r][c] = r * 4 + c;
        }
    }
    grid[2][3] *= 10;
    println!("{:?}", grid);
    let mut col_sums = vec![0; 4];
    for row in &grid {
        for c in 0..4 {
            col_sums[c] += row[c];
        }
    }
    println!("{:?}", col_sums);

    let mut counts = vec![0u32; 5];
    for x in &[1, 3, 3, 4, 1, 1] {
        counts[*x] += 1;
    }
    println!("{:?}", counts);
    let last = v[v.len() - 1];
    println!("last {last}");
}
