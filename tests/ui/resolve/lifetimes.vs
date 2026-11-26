// Exception 13: quote-free lifetimes resolve to lifetime parameters.

struct Lines<doc> {
    text: &doc string
}

func first<a>(x: &a string, y: &string) -> &a string { x }
func both<a>(x: &a string, y: &a string) -> &a string { x }
func nowhere(a: &string) -> &c string { a } //~ ERROR use of undeclared lifetime name `c`
func param(a: &string) -> &a string { a } //~ ERROR use of undeclared lifetime name `a`
func view<doc>(l: Lines<doc>) -> &doc string { l.text }
