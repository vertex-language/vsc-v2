package main

func isVowel(c: char) -> bool {
    match c {
        'a' | 'e' | 'i' | 'o' | 'u' => true
        _ => false
    }
}

func shift(c: char, by: uint8) -> char {
    if c >= 'a' && c <= 'z' {
        (((c as uint8 - b'a' + by) % 26) + b'a') as char
    } else {
        c
    }
}

func main() {
    let letters = ['h', 'e', 'l', 'l', 'o', '!']
    var vowels = 0
    for c in &letters {
        if isVowel(*c) {
            vowels += 1
        }
    }
    println("vowels {vowels}")
    for c in &letters {
        print("{}", shift(*c, 3))
    }
    println()
    println("{} {} {}", 'A' as uint32, 'é' as uint32, '\u{1F600}' as uint32)
    println("{:?} {:?} {:?}", '\n', '\'', 'ß')
    let digit = '7'
    println("{}", digit as uint8 - b'0')
    let upper = (b'a' - 32) as char
    println("{upper}")
    println("{}", 'z' > 'a')
    let c = 65u8 as char
    println("[{c:>3}] [{c:<3}] [{c:^5}]")
}
