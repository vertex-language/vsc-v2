fn sieve(n: usize) -> Vec<usize> {
    let mut is_prime = vec![true; n + 1];
    is_prime[0] = false;
    if n >= 1 {
        is_prime[1] = false;
    }
    let mut i = 2;
    while i * i <= n {
        if is_prime[i] {
            let mut j = i * i;
            while j <= n {
                is_prime[j] = false;
                j += i;
            }
        }
        i += 1;
    }
    let mut out = Vec::new();
    for k in 0..=n {
        if is_prime[k] {
            out.push(k);
        }
    }
    out
}

fn main() {
    let primes = sieve(60);
    println!("{:?}", primes);
    println!("count {}", primes.len());
    let mut gaps = Vec::new();
    for i in 1..primes.len() {
        gaps.push(primes[i] - primes[i - 1]);
    }
    println!("{:?}", gaps);
}
