// Exception 13: quote-free lifetimes and origin parameters.

struct Lines<doc> {
    text: &doc string
}

func first(a: &string, b: &string) -> &a string { a }
func both(a: &string, b: &string) -> &[a, b] string { a }
func nowhere(a: &string) -> &c string { a } //~ ERROR use of undeclared origin `c`
func view<doc>(l: Lines<doc>) -> &doc string { l.text }
