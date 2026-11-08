fn ways(n: usize, memo: &mut Vec<Option<u64>>) -> u64 {
    if n <= 1 {
        return 1;
    }
    if let Some(v) = memo[n] {
        return v;
    }
    let v = ways(n - 1, memo) + ways(n - 2, memo);
    memo[n] = Some(v);
    v
}

fn lcs(a: &[char], b: &[char]) -> usize {
    let mut dp = vec![vec![0; b.len() + 1]; a.len() + 1];
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            dp[i][j] = if a[i - 1] == b[j - 1] {
                dp[i - 1][j - 1] + 1
            } else if dp[i - 1][j] >= dp[i][j - 1] {
                dp[i - 1][j]
            } else {
                dp[i][j - 1]
            };
        }
    }
    dp[a.len()][b.len()]
}

fn main() {
    let mut memo = vec![None; 60];
    println!("{} {} {}", ways(5, &mut memo), ways(30, &mut memo), ways(59, &mut memo));
    let a = ['k', 'i', 't', 't', 'e', 'n'];
    let b = ['s', 'i', 't', 't', 'i', 'n', 'g'];
    println!("lcs {}", lcs(&a, &b));
}
