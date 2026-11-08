package main

func main() {
    println("{} {} {} {}", 2.0, 0.1 + 0.2, -0.0, 1e21)
    println("{:?} {:?} {:?}", 2.0, 0.5f32, 1.0e-7)
    println("{:.2} {:.0} {:.0} {:8.3}", 3.14159, 2.5, 3.5, -1.5)
    println("{:e} {:E}", 1234.5, 0.00012)
    let x = 7.0 / 2.0
    let y = 10.0 % 3.0
    println("{x} {y} {}", x * y - 1.0)
    println("{}", 1.0 / 3.0)
    println("{}", 3.0f32)
}
