package main

func main() {
    var stack = vec[]
    for i in 1..=5 {
        stack.push(i * i)
    }
    while let Option.Some(top) = stack.pop() {
        print("{top} ")
    }
    println()

    var fib: Vec<uint64> = vec[0, 1]
    var prev = 0
    var cur = 1
    while fib.len() < 15 {
        let next = prev + cur
        prev = cur
        cur = next
        fib.push(next)
    }
    println("{:?}", fib)

    var digits = vec[]
    var n = 90817u32
    while n > 0 {
        digits.push(n % 10)
        n /= 10
    }
    println("{:?}", digits)
    var sum = 0
    while let Option.Some(d) = digits.pop() {
        if d == 0 {
            continue
        }
        sum += d
    }
    println("digit sum {sum}")

    var opt = Option.Some(3)
    while let Option.Some(i) = opt {
        println("countdown {i}")
        opt = if i > 1 { Option.Some(i - 1) } else { Option.None }
    }
}
