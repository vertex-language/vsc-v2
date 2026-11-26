use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use crate::{BytePos, Span};

/// The name a source file is shown under.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum FileName {
    Real(PathBuf),
    /// Source that didn't come from disk, such as a test string.
    Custom(String),
}

impl fmt::Display for FileName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileName::Real(path) => write!(f, "{}", path.display()),
            FileName::Custom(name) => write!(f, "<{name}>"),
        }
    }
}

impl From<PathBuf> for FileName {
    fn from(p: PathBuf) -> Self {
        FileName::Real(p)
    }
}

/// One loaded file.
pub struct SourceFile {
    pub name: FileName,
    /// The text after normalization: no byte-order mark, `\r\n` turned into `\n`.
    pub src: Arc<str>,
    /// Where this file starts in the source map's address space.
    pub start_pos: BytePos,
    pub source_len: u32,
    /// Byte offsets (relative to `start_pos`) of the first byte of every line.
    lines: Vec<u32>,
}

impl SourceFile {
    fn new(name: FileName, mut src: String, start_pos: BytePos) -> SourceFile {
        normalize_src(&mut src);
        let source_len = u32::try_from(src.len()).expect("source file larger than 4 GiB");
        let mut lines = vec![0];
        lines.extend(src.bytes().enumerate().filter(|&(_, b)| b == b'\n').map(|(i, _)| i as u32 + 1));
        SourceFile { name, src: src.into(), start_pos, source_len, lines }
    }

    #[inline]
    pub fn end_pos(&self) -> BytePos {
        BytePos(self.start_pos.0 + self.source_len)
    }

    #[inline]
    pub fn contains(&self, pos: BytePos) -> bool {
        self.start_pos <= pos && pos <= self.end_pos()
    }

    #[inline]
    pub fn relative(&self, pos: BytePos) -> usize {
        (pos.0 - self.start_pos.0) as usize
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// The 0-based line `pos` is on.
    pub fn lookup_line(&self, pos: BytePos) -> usize {
        let rel = pos.0 - self.start_pos.0;
        match self.lines.binary_search(&rel) {
            Ok(i) => i,
            Err(i) => i - 1,
        }
    }

    /// The text of a 0-based line, without its line feed.
    pub fn line_text(&self, line: usize) -> Option<&str> {
        let start = *self.lines.get(line)? as usize;
        let end = self.lines.get(line + 1).map(|&e| e as usize - 1).unwrap_or(self.src.len());
        Some(&self.src[start..end.max(start)])
    }

    /// The position of the first byte of a 0-based line.
    pub fn line_start(&self, line: usize) -> BytePos {
        BytePos(self.start_pos.0 + self.lines[line])
    }

    /// 1-based line and 0-based column (in chars) of `pos`.
    pub fn lookup_file_pos(&self, pos: BytePos) -> (usize, usize) {
        let line = self.lookup_line(pos);
        let line_start = self.lines[line] as usize;
        let rel = self.relative(pos);
        let col = self.src[line_start..rel.min(self.src.len())].chars().count();
        (line + 1, col)
    }
}

impl fmt::Debug for SourceFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SourceFile({})", self.name)
    }
}

fn normalize_src(src: &mut String) {
    if src.starts_with('\u{feff}') {
        src.drain(..3);
    }
    if src.contains("\r\n") {
        *src = src.replace("\r\n", "\n");
    }
}

/// A position looked up in the source map.
#[derive(Debug, Clone)]
pub struct Loc {
    pub file: Arc<SourceFile>,
    /// 1-based line.
    pub line: usize,
    /// 0-based column, in chars.
    pub col: usize,
    /// 0-based column as displayed, with tabs counted as four columns.
    pub col_display: usize,
}

/// Every source file the compiler has loaded, laid out one after another in a
/// single address space.
pub struct SourceMap {
    files: RwLock<Vec<Arc<SourceFile>>>,
}

impl Default for SourceMap {
    fn default() -> Self {
        SourceMap::new()
    }
}

impl SourceMap {
    pub fn new() -> SourceMap {
        SourceMap { files: RwLock::new(Vec::new()) }
    }

    /// Adds a file. Files are separated by one unused position, and the first
    /// starts at 1, so position 0 and the end of one file never belong to the
    /// next.
    pub fn new_source_file(&self, name: FileName, src: String) -> Arc<SourceFile> {
        let mut files = self.files.write().unwrap();
        let start = files.last().map(|f| f.end_pos().0 + 1).unwrap_or(1);
        let file = Arc::new(SourceFile::new(name, src, BytePos(start)));
        files.push(file.clone());
        file
    }

    pub fn load_file(&self, path: &Path) -> io::Result<Arc<SourceFile>> {
        let src = std::fs::read_to_string(path)?;
        Ok(self.new_source_file(FileName::Real(path.to_path_buf()), src))
    }

    pub fn files(&self) -> Vec<Arc<SourceFile>> {
        self.files.read().unwrap().clone()
    }

    pub fn lookup_source_file(&self, pos: BytePos) -> Option<Arc<SourceFile>> {
        let files = self.files.read().unwrap();
        let i = match files.binary_search_by_key(&pos, |f| f.start_pos) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let file = &files[i];
        file.contains(pos).then(|| file.clone())
    }

    pub fn lookup_char_pos(&self, pos: BytePos) -> Option<Loc> {
        let file = self.lookup_source_file(pos)?;
        let (line, col) = file.lookup_file_pos(pos);
        let text = file.line_text(line - 1).unwrap_or("");
        let col_display = text.chars().take(col).map(|c| if c == '\t' { 4 } else { 1 }).sum();
        Some(Loc { file, line, col, col_display })
    }

    /// `file:line:col`, 1-based, as diagnostics print it.
    pub fn span_to_location(&self, span: Span) -> String {
        match self.lookup_char_pos(span.lo()) {
            Some(loc) => format!("{}:{}:{}", loc.file.name, loc.line, loc.col + 1),
            None => "<unknown>".to_string(),
        }
    }

    pub fn span_to_snippet(&self, span: Span) -> Option<String> {
        let file = self.lookup_source_file(span.lo())?;
        if !file.contains(span.hi()) {
            return None;
        }
        file.src.get(file.relative(span.lo())..file.relative(span.hi())).map(str::to_string)
    }

    pub fn is_multiline(&self, span: Span) -> bool {
        match self.lookup_source_file(span.lo()) {
            Some(f) if f.contains(span.hi()) => f.lookup_line(span.lo()) != f.lookup_line(span.hi()),
            _ => false,
        }
    }

    /// Whether `span` is on the same line as the end of `prev`.
    pub fn is_same_line(&self, a: Span, b: Span) -> bool {
        match self.lookup_source_file(a.lo()) {
            Some(f) if f.contains(b.lo()) => f.lookup_line(a.hi()) == f.lookup_line(b.lo()),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_positions() {
        let sm = SourceMap::new();
        let a = sm.new_source_file(FileName::Custom("a".into()), "ab\ncd\n".into());
        let b = sm.new_source_file(FileName::Custom("b".into()), "\u{feff}x\r\ny".into());
        assert_eq!(a.start_pos, BytePos(1));
        assert_eq!(b.start_pos, BytePos(a.end_pos().0 + 1));
        assert_eq!(&*b.src, "x\ny");

        let loc = sm.lookup_char_pos(BytePos(a.start_pos.0 + 4)).unwrap();
        assert_eq!((loc.line, loc.col), (2, 1));
        assert_eq!(a.line_text(1), Some("cd"));
        assert_eq!(a.line_text(2), Some(""));

        let loc = sm.lookup_char_pos(BytePos(b.start_pos.0 + 2)).unwrap();
        assert_eq!(loc.file.name, FileName::Custom("b".into()));
        assert_eq!((loc.line, loc.col), (2, 0));

        let span = Span::new(BytePos(a.start_pos.0 + 3), BytePos(a.start_pos.0 + 5));
        assert_eq!(sm.span_to_snippet(span).as_deref(), Some("cd"));
        assert_eq!(sm.span_to_location(span), "<a>:2:1");
        assert!(sm.lookup_source_file(BytePos(0)).is_none());
    }
}
