fn main() {
    let mut v = vec![1, 2, 3];
    v.push(4);
    v.push(5);
    println!("{:?} len {}", v, v.len());
    let last = v.pop();
    println!("popped {:?}, now {:?}", last, v);
    let mut evens = Vec::new();
    for i in 0..10 {
        if i % 2 == 0 {
            evens.push(i);
        }
    }
    println!("{evens:?}");
    let zeros = vec![0u8; 4];
    println!("{zeros:?}");
    let mut empty: Vec<i32> = Vec::new();
    println!("{:?} {:?}", empty.pop(), empty.len());
}
