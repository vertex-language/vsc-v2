package main

func sign(n: int32) -> int32 {
    if n > 0 {
        1
    } else if n < 0 {
        -1
    } else {
        0
    }
}

func main() {
    let x = 5
    let x = x * 2
    {
        let x = "inner"
        println("inner x {x}")
    }
    println("outer x {x}")

    let y = {
        let a = 3
        let b = 4
        a * a + b * b
    }
    println("y {y}")

    let parity = if y % 2 == 0 { "even" } else { "odd" }
    println("{parity}")

    for n in &[-8, 0, 3] {
        print("{} ", sign(*n))
    }
    println()

    var acc = 0
    let doubled = {
        acc += 10
        acc * 2
    }
    println("{acc} {doubled}")

    let grade = func(score: uint32) -> char {
        match score {
            90..=100 => 'A'
            80..=89 => 'B'
            70..=79 => 'C'
            _ => 'F'
        }
    }
    println("{} {} {}", grade(95), grade(81), grade(12))

    let nested = if x > 5 {
        if x > 8 { "big" } else { "medium" }
    } else {
        "small"
    }
    println("{nested}")
}
