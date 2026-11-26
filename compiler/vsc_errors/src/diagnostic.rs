use std::fmt;

use vsc_span::Span;

use crate::{DiagCtxt, ErrorGuaranteed};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Level {
    /// An internal compiler error.
    Bug,
    Fatal,
    Error,
    Warning,
    Note,
    Help,
    /// The `For more information …` lines at the end.
    FailureNote,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Bug => "error: internal compiler error",
            Level::Fatal | Level::Error => "error",
            Level::Warning => "warning",
            Level::Note => "note",
            Level::Help => "help",
            Level::FailureNote => "failure-note",
        }
    }
}

/// An error code, such as `E0382`. Vertex keeps Rust's numbers.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ErrCode(pub u32);

impl fmt::Display for ErrCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:04}", self.0)
    }
}

#[derive(Clone, Debug)]
pub struct SpanLabel {
    pub span: Span,
    pub label: Option<String>,
    pub is_primary: bool,
}

/// The spans a diagnostic points at: primary spans (`^^^`) and labeled
/// secondary spans (`---`).
#[derive(Clone, Debug, Default)]
pub struct MultiSpan {
    pub primary_spans: Vec<Span>,
    pub labels: Vec<(Span, String)>,
}

impl MultiSpan {
    pub fn new() -> MultiSpan {
        MultiSpan::default()
    }

    pub fn from_span(span: Span) -> MultiSpan {
        MultiSpan { primary_spans: vec![span], labels: vec![] }
    }

    pub fn push_span_label(&mut self, span: Span, label: impl Into<String>) {
        self.labels.push((span, label.into()));
    }

    pub fn primary_span(&self) -> Option<Span> {
        self.primary_spans.first().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.primary_spans.is_empty() && self.labels.is_empty()
    }

    /// Every span with its label. A primary span takes the label given for it.
    pub fn span_labels(&self) -> Vec<SpanLabel> {
        let mut out: Vec<SpanLabel> = self
            .primary_spans
            .iter()
            .map(|&span| SpanLabel {
                span,
                label: self.labels.iter().find(|(s, _)| *s == span).map(|(_, l)| l.clone()),
                is_primary: true,
            })
            .collect();
        for (span, label) in &self.labels {
            if !self.primary_spans.contains(span) {
                out.push(SpanLabel { span: *span, label: Some(label.clone()), is_primary: false });
            }
        }
        out
    }
}

impl From<Span> for MultiSpan {
    fn from(span: Span) -> MultiSpan {
        MultiSpan::from_span(span)
    }
}

impl From<Vec<Span>> for MultiSpan {
    fn from(spans: Vec<Span>) -> MultiSpan {
        MultiSpan { primary_spans: spans, labels: vec![] }
    }
}

/// A note or help attached to a diagnostic.
#[derive(Clone, Debug)]
pub struct Subdiag {
    pub level: Level,
    pub message: String,
    pub span: MultiSpan,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Applicability {
    /// The suggestion is definitely what the user meant.
    MachineApplicable,
    /// It may be what the user meant.
    MaybeIncorrect,
    /// It contains placeholders the user has to fill in.
    HasPlaceholders,
    Unspecified,
}

/// A replacement for the code in `span`.
#[derive(Clone, Debug)]
pub struct Suggestion {
    pub span: Span,
    pub replacement: String,
    pub message: String,
    pub applicability: Applicability,
}

/// The contents of a diagnostic, without the context that emits it.
#[derive(Clone, Debug)]
pub struct DiagInner {
    pub level: Level,
    pub message: String,
    pub code: Option<ErrCode>,
    pub span: MultiSpan,
    pub children: Vec<Subdiag>,
    pub suggestions: Vec<Suggestion>,
}

impl DiagInner {
    pub fn new(level: Level, message: impl Into<String>) -> DiagInner {
        DiagInner {
            level,
            message: message.into(),
            code: None,
            span: MultiSpan::new(),
            children: vec![],
            suggestions: vec![],
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self.level, Level::Bug | Level::Fatal | Level::Error)
    }
}

/// A diagnostic under construction. It must be emitted or cancelled: dropping
/// one unused is a bug in the compiler, and panics in debug builds.
#[must_use = "a diagnostic must be emitted or cancelled"]
pub struct Diag<'a> {
    dcx: &'a DiagCtxt,
    diag: Option<Box<DiagInner>>,
}

impl<'a> Diag<'a> {
    pub fn new(dcx: &'a DiagCtxt, level: Level, message: impl Into<String>) -> Diag<'a> {
        Diag { dcx, diag: Some(Box::new(DiagInner::new(level, message))) }
    }

    fn inner(&mut self) -> &mut DiagInner {
        self.diag.as_mut().expect("diagnostic already emitted")
    }

    pub fn dcx(&self) -> &'a DiagCtxt {
        self.dcx
    }

    pub fn level(&self) -> Level {
        self.diag.as_ref().unwrap().level
    }

    pub fn message(&self) -> &str {
        &self.diag.as_ref().unwrap().message
    }

    pub fn primary_span(&self) -> Option<Span> {
        self.diag.as_ref().unwrap().span.primary_span()
    }

    pub fn emit(mut self) -> ErrorGuaranteed {
        let diag = *self.diag.take().unwrap();
        let is_error = diag.is_error();
        let guar = self.dcx.emit_diagnostic(diag);
        if is_error { guar.unwrap() } else { ErrorGuaranteed::unchecked_error_guaranteed() }
    }

    pub fn cancel(mut self) {
        self.diag = None;
    }

    pub fn set_span(&mut self, span: impl Into<MultiSpan>) -> &mut Self {
        let new: MultiSpan = span.into();
        let inner = self.inner();
        inner.span.primary_spans = new.primary_spans;
        inner.span.labels.extend(new.labels);
        self
    }

    pub fn with_span(mut self, span: impl Into<MultiSpan>) -> Self {
        self.set_span(span);
        self
    }

    pub fn span_label(&mut self, span: Span, label: impl Into<String>) -> &mut Self {
        self.inner().span.push_span_label(span, label);
        self
    }

    pub fn with_span_label(mut self, span: Span, label: impl Into<String>) -> Self {
        self.span_label(span, label);
        self
    }

    pub fn code(&mut self, code: ErrCode) -> &mut Self {
        self.inner().code = Some(code);
        self
    }

    pub fn with_code(mut self, code: ErrCode) -> Self {
        self.code(code);
        self
    }

    fn sub(&mut self, level: Level, message: impl Into<String>, span: MultiSpan) -> &mut Self {
        self.inner().children.push(Subdiag { level, message: message.into(), span });
        self
    }

    pub fn note(&mut self, msg: impl Into<String>) -> &mut Self {
        self.sub(Level::Note, msg, MultiSpan::new())
    }

    pub fn with_note(mut self, msg: impl Into<String>) -> Self {
        self.note(msg);
        self
    }

    pub fn span_note(&mut self, span: impl Into<MultiSpan>, msg: impl Into<String>) -> &mut Self {
        self.sub(Level::Note, msg, span.into())
    }

    pub fn with_span_note(mut self, span: impl Into<MultiSpan>, msg: impl Into<String>) -> Self {
        self.span_note(span, msg);
        self
    }

    pub fn help(&mut self, msg: impl Into<String>) -> &mut Self {
        self.sub(Level::Help, msg, MultiSpan::new())
    }

    pub fn with_help(mut self, msg: impl Into<String>) -> Self {
        self.help(msg);
        self
    }

    pub fn span_help(&mut self, span: impl Into<MultiSpan>, msg: impl Into<String>) -> &mut Self {
        self.sub(Level::Help, msg, span.into())
    }

    pub fn with_span_help(mut self, span: impl Into<MultiSpan>, msg: impl Into<String>) -> Self {
        self.span_help(span, msg);
        self
    }

    pub fn span_suggestion(
        &mut self,
        span: Span,
        msg: impl Into<String>,
        replacement: impl Into<String>,
        applicability: Applicability,
    ) -> &mut Self {
        self.inner().suggestions.push(Suggestion {
            span,
            replacement: replacement.into(),
            message: msg.into(),
            applicability,
        });
        self
    }

    pub fn with_span_suggestion(
        mut self,
        span: Span,
        msg: impl Into<String>,
        replacement: impl Into<String>,
        applicability: Applicability,
    ) -> Self {
        self.span_suggestion(span, msg, replacement, applicability);
        self
    }
}

impl Drop for Diag<'_> {
    fn drop(&mut self) {
        if let Some(diag) = self.diag.take() {
            if cfg!(debug_assertions) && !std::thread::panicking() {
                panic!("diagnostic dropped without being emitted: {}", diag.message);
            }
        }
    }
}

impl fmt::Debug for Diag<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.diag.fmt(f)
    }
}
