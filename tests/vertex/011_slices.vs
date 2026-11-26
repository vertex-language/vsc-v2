package main

func sum(xs: &[int64]) -> int64 {
    var total = 0
    for x in xs {
        total += *x
    }
    total
}

func describe(xs: &[int64]) -> string {
    match xs {
        [] => format("empty")
        [one] => format("one: {one}")
        [first, .., last] => format("{first}..{last} ({} items)", sliceLen(xs))
    }
}

func main() {
    let a = [3, 1, 4, 1, 5, 9, 2, 6]
    println("sum {}", sum(&a))
    println("{}", describe(&a))
    println("{}", describe(&[7]))
    println("{}", describe(&[]))
    var grid = [[0; 3]; 3]
    for r in 0..3 {
        for c in 0..3 {
            grid[r][c] = r * 3 + c
        }
    }
    println("{grid:?}")
    let [x, y, z] = [10, 20, 30]
    println("{}", x + y + z)
}
