//! Printing diagnostics in rustc's human-readable format.

use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Mutex};

use vsc_span::{SourceMap, Span};

use crate::diagnostic::{DiagInner, Level, MultiSpan, Suggestion};

pub trait Emitter {
    fn emit_diagnostic(&mut self, diag: &DiagInner);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorConfig {
    Auto,
    Always,
    Never,
}

/// A writer whose output can be read back, for tests.
#[derive(Clone, Default)]
pub struct SharedBuffer(pub Arc<Mutex<Vec<u8>>>);

impl SharedBuffer {
    pub fn new() -> SharedBuffer {
        SharedBuffer::default()
    }

    pub fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Style {
    Bold,
    LineNumber,
    Level(Level),
    Primary(Level),
    Secondary,
}

/// Writes diagnostics as rustc does:
///
/// ```text
/// error: expected expression, found `}`
///  --> main.vs:3:1
///   |
/// 3 | }
///   | ^ expected expression
/// ```
pub struct HumanEmitter {
    dst: Box<dyn Write + Send>,
    source_map: Option<Arc<SourceMap>>,
    color: bool,
    short: bool,
}

impl HumanEmitter {
    pub fn new(dst: Box<dyn Write + Send>, color: bool) -> HumanEmitter {
        HumanEmitter { dst, source_map: None, color, short: false }
    }

    pub fn stderr(color: ColorConfig) -> HumanEmitter {
        let color = match color {
            ColorConfig::Always => true,
            ColorConfig::Never => false,
            ColorConfig::Auto => io::stderr().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
        };
        HumanEmitter::new(Box::new(io::stderr()), color)
    }

    pub fn with_source_map(mut self, source_map: Arc<SourceMap>) -> HumanEmitter {
        self.source_map = Some(source_map);
        self
    }

    /// One line per diagnostic: `file:line:col: error: message`.
    pub fn short(mut self, short: bool) -> HumanEmitter {
        self.short = short;
        self
    }

    fn paint(&self, out: &mut String, style: Style, text: &str) {
        if !self.color || text.is_empty() {
            out.push_str(text);
            return;
        }
        let code = match style {
            Style::Bold => "1",
            Style::LineNumber | Style::Secondary => "1;34",
            Style::Level(level) | Style::Primary(level) => match level {
                Level::Bug | Level::Fatal | Level::Error => "1;31",
                Level::Warning => "1;33",
                Level::Note => "1;32",
                Level::Help => "1;36",
                Level::FailureNote => "1",
            },
        };
        out.push_str("\x1b[");
        out.push_str(code);
        out.push('m');
        out.push_str(text);
        out.push_str("\x1b[0m");
    }

    fn render(&self, diag: &DiagInner) -> String {
        let mut out = String::new();
        if self.short {
            if let (Some(sm), Some(span)) = (&self.source_map, diag.span.primary_span()) {
                if !span.is_dummy() {
                    out.push_str(&sm.span_to_location(span));
                    out.push_str(": ");
                }
            }
            self.header(&mut out, diag.level, diag.code.map(|c| c.to_string()), &diag.message);
            out.push('\n');
            return out;
        }

        let width = self.line_number_width(diag);
        self.header(&mut out, diag.level, diag.code.map(|c| c.to_string()), &diag.message);
        out.push('\n');
        let mut printed_snippet = self.snippet(&mut out, &diag.span, diag.level, width);

        for child in &diag.children {
            if child.span.is_empty() {
                if printed_snippet {
                    self.gutter(&mut out, width, "");
                    printed_snippet = false;
                }
                out.push_str(&" ".repeat(width + 1));
                self.paint(&mut out, Style::LineNumber, "=");
                out.push(' ');
                self.paint(&mut out, Style::Bold, &format!("{}:", child.level.as_str()));
                // Indent continuation lines under the message.
                let indent = " ".repeat(width + 4 + child.level.as_str().len());
                for (i, line) in child.message.lines().enumerate() {
                    if i > 0 {
                        out.push('\n');
                        out.push_str(&indent);
                    } else {
                        out.push(' ');
                    }
                    out.push_str(line);
                }
                out.push('\n');
            } else {
                self.header(&mut out, child.level, None, &child.message);
                out.push('\n');
                printed_snippet = self.snippet(&mut out, &child.span, child.level, width);
            }
        }

        for sugg in &diag.suggestions {
            self.header(&mut out, Level::Help, None, &sugg.message);
            out.push('\n');
            self.suggestion(&mut out, sugg, width);
        }
        out.push('\n');
        out
    }

    fn header(&self, out: &mut String, level: Level, code: Option<String>, message: &str) {
        let mut head = level.as_str().to_string();
        if let Some(code) = code {
            head.push('[');
            head.push_str(&code);
            head.push(']');
        }
        self.paint(out, Style::Level(level), &head);
        let msg = format!(": {message}");
        if matches!(level, Level::Error | Level::Fatal | Level::Warning | Level::Bug) {
            self.paint(out, Style::Bold, &msg);
        } else {
            out.push_str(&msg);
        }
    }

    fn gutter(&self, out: &mut String, width: usize, line_no: &str) {
        let label = format!("{line_no:>width$} |");
        self.paint(out, Style::LineNumber, &label);
        out.push('\n');
    }

    fn line_number_width(&self, diag: &DiagInner) -> usize {
        let Some(sm) = &self.source_map else { return 1 };
        let mut max = 0;
        let mut consider = |span: Span| {
            if !span.is_dummy() {
                if let Some(loc) = sm.lookup_char_pos(span.hi()) {
                    max = max.max(loc.line);
                }
            }
        };
        for l in diag.span.span_labels() {
            consider(l.span);
        }
        for c in &diag.children {
            for l in c.span.span_labels() {
                consider(l.span);
            }
        }
        for s in &diag.suggestions {
            consider(s.span);
        }
        max.max(1).to_string().len()
    }

    /// Prints the source lines `ms` points at. Returns whether anything was printed.
    fn snippet(&self, out: &mut String, ms: &MultiSpan, level: Level, width: usize) -> bool {
        let Some(sm) = &self.source_map else { return false };
        let labels: Vec<_> = ms.span_labels().into_iter().filter(|l| !l.span.is_dummy()).collect();
        if labels.is_empty() {
            return false;
        }

        // Group annotations by file, with the primary span's file first.
        let mut files: Vec<(Arc<vsc_span::SourceFile>, Vec<Annotation>)> = Vec::new();
        let mut ordered = labels.clone();
        ordered.sort_by_key(|l| !l.is_primary);
        for l in &ordered {
            let Some(lo) = sm.lookup_char_pos(l.span.lo()) else { continue };
            let Some(hi) = sm.lookup_char_pos(l.span.hi()) else { continue };
            let idx = match files.iter().position(|(f, _)| Arc::ptr_eq(f, &lo.file)) {
                Some(i) => i,
                None => {
                    files.push((lo.file.clone(), Vec::new()));
                    files.len() - 1
                }
            };
            let anns = &mut files[idx].1;
            if lo.line == hi.line {
                anns.push(Annotation {
                    line: lo.line,
                    start: lo.col_display,
                    end: hi.col_display.max(lo.col_display + 1),
                    primary: l.is_primary,
                    label: l.label.clone(),
                });
            } else {
                // A span over several lines: mark the rest of its first line,
                // and the start of its last line, which carries the label.
                let first_len = display_len(lo.file.line_text(lo.line - 1).unwrap_or(""));
                anns.push(Annotation {
                    line: lo.line,
                    start: lo.col_display,
                    end: first_len.max(lo.col_display + 1),
                    primary: l.is_primary,
                    label: None,
                });
                anns.push(Annotation {
                    line: hi.line,
                    start: 0,
                    end: hi.col_display.max(1),
                    primary: l.is_primary,
                    label: l.label.clone(),
                });
            }
        }

        for (fi, (file, anns)) in files.iter().enumerate() {
            let loc_span = ordered
                .iter()
                .find(|l| sm.lookup_source_file(l.span.lo()).is_some_and(|f| Arc::ptr_eq(&f, file)))
                .map(|l| l.span)
                .unwrap();
            let arrow = if fi == 0 { "-->" } else { ":::" };
            out.push_str(&" ".repeat(width));
            self.paint(out, Style::LineNumber, arrow);
            out.push(' ');
            out.push_str(&sm.span_to_location(loc_span));
            out.push('\n');
            self.gutter(out, width, "");

            let mut lines: Vec<usize> = anns.iter().map(|a| a.line).collect();
            lines.sort_unstable();
            lines.dedup();
            let mut prev: Option<usize> = None;
            for &line in &lines {
                if let Some(p) = prev {
                    if line == p + 2 {
                        self.source_line(out, file, p + 1, width);
                    } else if line > p + 2 {
                        self.paint(out, Style::LineNumber, "...");
                        out.push('\n');
                    }
                }
                self.source_line(out, file, line, width);
                let mut on_line: Vec<&Annotation> = anns.iter().filter(|a| a.line == line).collect();
                on_line.sort_by_key(|a| (a.start, !a.primary));
                self.annotations(out, &on_line, level, width);
                prev = Some(line);
            }
        }
        true
    }

    fn source_line(&self, out: &mut String, file: &vsc_span::SourceFile, line: usize, width: usize) {
        let text = file.line_text(line - 1).unwrap_or("").replace('\t', "    ");
        let label = format!("{line:>width$} |");
        self.paint(out, Style::LineNumber, &label);
        if !text.is_empty() {
            out.push(' ');
            out.push_str(text.trim_end());
        }
        out.push('\n');
    }

    fn annotations(&self, out: &mut String, anns: &[&Annotation], level: Level, width: usize) {
        if anns.is_empty() {
            return;
        }
        let style = |a: &Annotation| if a.primary { Style::Primary(level) } else { Style::Secondary };

        // The underline row. The rightmost label goes at its end.
        let inline = anns.iter().rposition(|a| a.label.is_some()).filter(|&i| {
            let a = anns[i];
            anns[i + 1..].iter().all(|b| b.start >= a.end) && i == anns.len() - 1
        });
        let mut row = String::new();
        let mut col = 0;
        for a in anns {
            if a.start < col {
                continue;
            }
            row.push_str(&" ".repeat(a.start - col));
            let ch = if a.primary { "^" } else { "-" };
            self.paint(&mut row, style(a), &ch.repeat(a.end - a.start));
            col = a.end;
        }
        if let Some(i) = inline {
            row.push(' ');
            self.paint(&mut row, style(anns[i]), anns[i].label.as_deref().unwrap());
        }
        self.gutter_prefix(out, width);
        out.push(' ');
        out.push_str(&row);
        out.push('\n');

        // Remaining labels hang below on connector lines.
        let mut pending: Vec<&Annotation> =
            anns.iter().enumerate().filter(|&(i, a)| a.label.is_some() && Some(i) != inline).map(|(_, a)| *a).collect();
        if pending.is_empty() {
            return;
        }
        let connectors = |pending: &[&Annotation], upto: usize| {
            let mut row = String::new();
            let mut col = 0;
            for a in pending.iter().take(upto) {
                if a.start < col {
                    continue;
                }
                row.push_str(&" ".repeat(a.start - col));
                self.paint(&mut row, style(a), "|");
                col = a.start + 1;
            }
            row
        };
        self.gutter_prefix(out, width);
        out.push(' ');
        let row = connectors(&pending, pending.len());
        out.push_str(&row);
        out.push('\n');
        while let Some(a) = pending.pop() {
            self.gutter_prefix(out, width);
            out.push(' ');
            let mut row = connectors(&pending, pending.len());
            let used = pending.iter().map(|p| p.start + 1).max().unwrap_or(0);
            row.push_str(&" ".repeat(a.start.saturating_sub(used)));
            self.paint(&mut row, style(a), a.label.as_deref().unwrap());
            out.push_str(&row);
            out.push('\n');
        }
    }

    fn gutter_prefix(&self, out: &mut String, width: usize) {
        let label = format!("{} |", " ".repeat(width));
        self.paint(out, Style::LineNumber, &label);
    }

    fn suggestion(&self, out: &mut String, sugg: &Suggestion, width: usize) {
        let Some(sm) = &self.source_map else { return };
        let (Some(lo), Some(hi)) = (sm.lookup_char_pos(sugg.span.lo()), sm.lookup_char_pos(sugg.span.hi())) else {
            return;
        };
        if lo.line != hi.line {
            return;
        }
        let text = lo.file.line_text(lo.line - 1).unwrap_or("");
        let start_byte = byte_of_char(text, lo.col);
        let end_byte = byte_of_char(text, hi.col);
        // A removal shows the original line with the removed part marked.
        let new_line = if sugg.replacement.is_empty() {
            text.to_string()
        } else {
            format!("{}{}{}", &text[..start_byte], sugg.replacement, &text[end_byte..])
        };
        self.gutter(out, width, "");
        let label = format!("{:>width$} |", lo.line);
        self.paint(out, Style::LineNumber, &label);
        out.push(' ');
        out.push_str(new_line.replace('\t', "    ").trim_end());
        out.push('\n');
        let start = display_len(&text[..start_byte]);
        let (mark, len) = if sugg.replacement.is_empty() {
            ("-", display_len(&text[start_byte..end_byte]))
        } else if start_byte == end_byte {
            ("+", display_len(&sugg.replacement))
        } else {
            ("~", display_len(&sugg.replacement))
        };
        self.gutter_prefix(out, width);
        out.push(' ');
        out.push_str(&" ".repeat(start));
        self.paint(out, Style::Level(Level::Help), &mark.repeat(len.max(1)));
        out.push('\n');
    }
}

impl Emitter for HumanEmitter {
    fn emit_diagnostic(&mut self, diag: &DiagInner) {
        let text = self.render(diag);
        let _ = self.dst.write_all(text.as_bytes());
        let _ = self.dst.flush();
    }
}

#[derive(Clone)]
struct Annotation {
    line: usize,
    start: usize,
    end: usize,
    primary: bool,
    label: Option<String>,
}

fn display_len(s: &str) -> usize {
    s.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

fn byte_of_char(s: &str, chars: usize) -> usize {
    s.char_indices().nth(chars).map(|(i, _)| i).unwrap_or(s.len())
}
