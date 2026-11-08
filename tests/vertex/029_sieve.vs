package main

func sieve(n: uint) -> Vec<uint> {
    var isPrime = vec[true; n + 1]
    isPrime[0] = false
    if n >= 1 {
        isPrime[1] = false
    }
    var i = 2
    while i * i <= n {
        if isPrime[i] {
            var j = i * i
            while j <= n {
                isPrime[j] = false
                j += i
            }
        }
        i += 1
    }
    var out = vec[]
    for k in 0..=n {
        if isPrime[k] {
            out.push(k)
        }
    }
    out
}

func main() {
    let primes = sieve(60)
    println("{:?}", primes)
    println("count {}", primes.len())
    var gaps = vec[]
    for i in 1..primes.len() {
        gaps.push(primes[i] - primes[i - 1])
    }
    println("{:?}", gaps)
}
