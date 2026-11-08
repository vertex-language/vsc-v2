package main

func factorial(n: uint8) -> uint8 {
    if n == 0 { 1 } else { n * factorial(n - 1) }
}

func main() {
    for i in 0..10 {
        println("{i}! = {}", factorial(i))
    }
}
