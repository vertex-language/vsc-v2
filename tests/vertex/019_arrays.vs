package main

func transpose(m: [[int32; 3]; 2]) -> [[int32; 2]; 3] {
    var t = [[0; 2]; 3]
    for r in 0..2 {
        for c in 0..3 {
            t[c][r] = m[r][c]
        }
    }
    t
}

func bubbleSort(xs: &mut [int32]) {
    let n = sliceLen(xs)
    for i in 0..n {
        for j in 0..n - 1 - i {
            if xs[j] > xs[j + 1] {
                let tmp = xs[j]
                xs[j] = xs[j + 1]
                xs[j + 1] = tmp
            }
        }
    }
}

func main() {
    let m = [[1, 2, 3], [4, 5, 6]]
    println("{:?}", transpose(m))
    var data = [5, -1, 8, 3, 0, 2]
    bubbleSort(&mut data)
    println("{:?}", data)
    var squares = [0u64; 8]
    for i in 0..8 {
        squares[i] = (i * i) as uint64
    }
    println("{:?}", squares)
    var total = 0
    for s in &squares {
        total += *s
    }
    println("total {total}")
    let words = ["alpha", "beta", "gamma"]
    for i in 0..sliceLen(&words) {
        print("{i}:{} ", words[i])
    }
    println()
    var counts = [0; 5]
    for v in &[1, 3, 3, 4, 1, 1, 0] {
        counts[*v] += 1
    }
    println("{:?}", counts)
    let first = &data[0]
    println("first {first} len {}", sliceLen(&data))
}
