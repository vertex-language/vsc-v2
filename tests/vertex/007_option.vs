package main

func half(n: int32) -> Option<int32> {
    if n % 2 == 0 { Option.Some(n / 2) } else { Option.None }
}

func quarter(n: int32) -> Option<int32> {
    let h = half(n)?
    half(h)
}

func main() {
    println("{:?} {:?} {:?}", quarter(8), quarter(6), quarter(3))
    let values: [Option<int32>; 3] = [Option.Some(1), Option.None, Option.Some(3)]
    var sum = 0
    for v in &values {
        if let Option.Some(x) = v {
            sum += *x
        }
    }
    println("sum {sum}")
    let nested: Option<Option<int32>> = Option.Some(Option.None)
    println("{nested:?}")
    match quarter(16) {
        Option.Some(q) if q > 3 => println("big quarter {q}")
        Option.Some(q) => println("quarter {q}")
        Option.None => println("none")
    }
}
