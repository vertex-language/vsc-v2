package main

func main() {
    var v = vec[1, 2, 3]
    v.push(4)
    v.push(5)
    println("{:?} len {}", v, v.len())
    let last = v.pop()
    println("popped {:?}, now {:?}", last, v)
    var evens = vec[]
    for i in 0..10 {
        if i % 2 == 0 {
            evens.push(i)
        }
    }
    println("{evens:?}")
    let zeros = vec[0u8; 4]
    println("{zeros:?}")
    var empty: Vec<int32> = vec[]
    println("{:?} {:?}", empty.pop(), empty.len())
}
