package main

enum Token {
    Num(int64)
    Op(char)
    LParen
    RParen
    Ident { name: &static string; len: uint }
}

func classify(t: &Token) -> string {
    match t {
        Token.Num(n @ 0..=9) => format("digit {n}")
        Token.Num(n) if *n < 0 => format("negative {n}")
        Token.Num(n) => format("number {n}")
        Token.Op('+' | '-') => format("additive")
        Token.Op(c) => format("op {c}")
        Token.LParen | Token.RParen => format("paren")
        Token.Ident { name, len: 1 } => format("short {name}")
        Token.Ident { name, .. } => format("ident {name}")
    }
}

func bucket(x: uint8) -> &static string {
    match x {
        0 => "zero"
        1..=9 => "one digit"
        10..=99 => "two digits"
        _ => "three digits"
    }
}

func firstWord(x: Option<int32>) -> int32 {
    let Option.Some(k) = x else {
        return -1
    }
    k * 10
}

func main() {
    let toks = [
        Token.Num(7),
        Token.Num(-3),
        Token.Num(42),
        Token.Op('+'),
        Token.Op('*'),
        Token.LParen,
        Token.RParen,
        Token.Ident { name: "x", len: 1 },
        Token.Ident { name: "foo", len: 3 },
    ]
    for t in &toks {
        println("{:?} -> {}", t, classify(t))
    }
    for x in &[0u8, 5, 50, 255] {
        println("{x}: {}", bucket(*x))
    }
    for pair in &[(0, 3), (4, 0), (2, -2), (1, 1)] {
        let msg = match *pair {
            (0, y) => format("y axis {y}")
            (x, 0) => format("x axis {x}")
            (x, y) if x == -y => format("anti {x}")
            _ => format("other")
        }
        println("{msg}")
    }
    println("{} {}", firstWord(Option.Some(4)), firstWord(Option.None))
    let arr = [1, 2, 3, 4, 5]
    let [first, rest @ ..] = &arr
    println("{first} then {} more", sliceLen(rest))
    let [.., penult, _] = arr
    println("penultimate {penult}")
    let r = &(5, 6)
    let &(m, n) = r
    println("{}", m * n)
}
