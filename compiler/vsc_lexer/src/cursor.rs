use std::str::Chars;

/// What `first()` and friends return past the end of the input.
pub(crate) const EOF_CHAR: char = '\0';

/// Walks the input one char at a time, with a few chars of lookahead.
pub struct Cursor<'a> {
    len_remaining: usize,
    chars: Chars<'a>,
}

impl<'a> Cursor<'a> {
    pub fn new(input: &'a str) -> Cursor<'a> {
        Cursor { len_remaining: input.len(), chars: input.chars() }
    }

    pub fn as_str(&self) -> &'a str {
        self.chars.as_str()
    }

    /// The next char, without consuming it.
    #[inline]
    pub(crate) fn first(&self) -> char {
        self.chars.clone().next().unwrap_or(EOF_CHAR)
    }

    #[inline]
    pub(crate) fn second(&self) -> char {
        let mut iter = self.chars.clone();
        iter.next();
        iter.next().unwrap_or(EOF_CHAR)
    }

    #[inline]
    pub(crate) fn third(&self) -> char {
        let mut iter = self.chars.clone();
        iter.next();
        iter.next();
        iter.next().unwrap_or(EOF_CHAR)
    }

    #[inline]
    pub(crate) fn is_eof(&self) -> bool {
        self.chars.as_str().is_empty()
    }

    /// Bytes consumed since the start of the current token.
    #[inline]
    pub(crate) fn pos_within_token(&self) -> u32 {
        (self.len_remaining - self.chars.as_str().len()) as u32
    }

    #[inline]
    pub(crate) fn reset_pos_within_token(&mut self) {
        self.len_remaining = self.chars.as_str().len();
    }

    #[inline]
    pub(crate) fn bump(&mut self) -> Option<char> {
        self.chars.next()
    }

    #[inline]
    pub(crate) fn eat_while(&mut self, mut predicate: impl FnMut(char) -> bool) {
        while predicate(self.first()) && !self.is_eof() {
            self.bump();
        }
    }

    pub(crate) fn eat_until(&mut self, byte: u8) {
        let rest = self.chars.as_str();
        let skip = rest.bytes().position(|b| b == byte).unwrap_or(rest.len());
        self.chars = rest[skip..].chars();
    }
}
