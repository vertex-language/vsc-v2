package main

func ways(n: uint, memo: &mut Vec<Option<uint64>>) -> uint64 {
    if n <= 1 {
        return 1
    }
    if let Option.Some(v) = memo[n] {
        return v
    }
    let v = ways(n - 1, memo) + ways(n - 2, memo)
    memo[n] = Option.Some(v)
    v
}

func lcs(a: &[char], b: &[char]) -> uint {
    var dp = vec[vec[0; sliceLen(b) + 1]; sliceLen(a) + 1]
    for i in 1..=sliceLen(a) {
        for j in 1..=sliceLen(b) {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1] + 1
            } else if dp[i - 1][j] >= dp[i][j - 1] {
                dp[i - 1][j]
            } else {
                dp[i][j - 1]
            }
        }
    }
    dp[sliceLen(a)][sliceLen(b)]
}

func main() {
    var memo = vec[Option.None; 60]
    println("{} {} {}", ways(5, &mut memo), ways(30, &mut memo), ways(59, &mut memo))
    let a = ['k', 'i', 't', 't', 'e', 'n']
    let b = ['s', 'i', 't', 't', 'i', 'n', 'g']
    println("lcs {}", lcs(&a, &b))
}
