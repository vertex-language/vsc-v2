fn min_max(xs: &[i32]) -> (i32, i32) {
    let mut lo = xs[0];
    let mut hi = xs[0];
    for &x in xs {
        if x < lo {
            lo = x;
        }
        if x > hi {
            hi = x;
        }
    }
    (lo, hi)
}

fn divmod(a: i32, b: i32) -> (i32, i32) {
    (a / b, a % b)
}

fn main() {
    let (lo, hi) = min_max(&[4, -2, 9, 0]);
    println!("{lo} {hi}");
    let mut a = 1;
    let mut b = 2;
    (a, b) = (b, a);
    println!("{a} {b}");
    let nested = ((1, 2), (3, (4, 5)));
    let ((p, _), (q, (r, s))) = nested;
    println!("{}", p + q + r + s);
    let t = (1u8, 'z', 2.5f32, "s");
    println!("{:?} {} {}", t, t.1, (t.0 as f32) * t.2);
    let mut pt = (0, 0);
    pt.0 += 5;
    pt.1 -= 3;
    println!("{:?}", pt);
    let unit = ();
    println!("{:?}", unit);
    let (d, m) = divmod(17, 5);
    println!("{d} rem {m}");
    let pairs = [(1, 'a'), (2, 'b'), (3, 'c')];
    for (n, c) in &pairs {
        print!("{n}{c} ");
    }
    println!();
}
