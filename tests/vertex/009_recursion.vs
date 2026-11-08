package main

func fib(n: uint64) -> uint64 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

func ackermann(m: uint64, n: uint64) -> uint64 {
    if m == 0 {
        n + 1
    } else if n == 0 {
        ackermann(m - 1, 1)
    } else {
        ackermann(m - 1, ackermann(m, n - 1))
    }
}

func gcd(a: uint64, b: uint64) -> uint64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

func main() {
    println("fib(20) = {}", fib(20))
    println("ackermann(2, 3) = {}", ackermann(2, 3))
    println("gcd(1071, 462) = {}", gcd(1071, 462))
    var factorial: uint64 = 1
    for i in 1..16u64 {
        factorial *= i
    }
    println("15! = {factorial}")
}
