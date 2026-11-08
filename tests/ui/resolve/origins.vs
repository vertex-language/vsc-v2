// Exception 13: an origin names an origin parameter or a function parameter.

struct Lines<doc: Origin> {
    text: &[doc] string
}

func first(a: &string, b: &string) -> &[a] string { a }
func both(a: &string, b: &string) -> &[a, b] string { a }
func nowhere(a: &string) -> &[c] string { a } //~ ERROR use of undeclared origin `c`
func view<doc: Origin>(l: Lines<doc>) -> &[doc] string { l.text }
