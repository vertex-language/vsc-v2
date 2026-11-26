fn is_vowel(c: char) -> bool {
    match c {
        'a' | 'e' | 'i' | 'o' | 'u' => true,
        _ => false,
    }
}

fn shift(c: char, by: u8) -> char {
    if c >= 'a' && c <= 'z' {
        (((c as u8 - b'a' + by) % 26) + b'a') as char
    } else {
        c
    }
}

fn main() {
    let letters = ['h', 'e', 'l', 'l', 'o', '!'];
    let mut vowels = 0;
    for c in &letters {
        if is_vowel(*c) {
            vowels += 1;
        }
    }
    println!("vowels {vowels}");
    for c in &letters {
        print!("{}", shift(*c, 3));
    }
    println!();
    println!("{} {} {}", 'A' as u32, 'é' as u32, '\u{1F600}' as u32);
    println!("{:?} {:?} {:?}", '\n', '\'', 'ß');
    let digit = '7';
    println!("{}", digit as u8 - b'0');
    let upper = (b'a' - 32) as char;
    println!("{upper}");
    println!("{}", 'z' > 'a');
    let c = 65u8 as char;
    println!("[{c:>3}] [{c:<3}] [{c:^5}]");
}
