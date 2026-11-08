package main

func insertionSort(v: &mut Vec<int32>) {
    for i in 1..v.len() {
        let key = v[i]
        var j = i
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1]
            j -= 1
        }
        v[j] = key
    }
}

func prefixSums(v: &Vec<int32>) -> Vec<int32> {
    var out = vec[0; v.len() + 1]
    for i in 0..v.len() {
        out[i + 1] = out[i] + v[i]
    }
    out
}

func main() {
    var v = vec[9, 4, 7, 1, 8, 2]
    insertionSort(&mut v)
    println("{:?}", v)
    let p = prefixSums(&v)
    println("{:?}", p)
    println("sum of v[1..4] = {}", p[4] - p[1])

    var grid = vec[vec[0; 4]; 3]
    for r in 0..3 {
        for c in 0..4 {
            grid[r][c] = r * 4 + c
        }
    }
    grid[2][3] *= 10
    println("{:?}", grid)
    var colSums = vec[0; 4]
    for row in &grid {
        for c in 0..4 {
            colSums[c] += row[c]
        }
    }
    println("{:?}", colSums)

    var counts = vec[0u32; 5]
    for x in &[1, 3, 3, 4, 1, 1] {
        counts[*x] += 1
    }
    println("{:?}", counts)
    let last = v[v.len() - 1]
    println("last {last}")
}
