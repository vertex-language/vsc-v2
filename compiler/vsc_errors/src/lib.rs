//! Diagnostics: building them, counting them and printing them (rustc's
//! `rustc_errors`).
//!
//! A diagnostic is built with [`DiagCtxt::struct_span_err`] and friends, given
//! labels, notes and suggestions, and then emitted. Messages are plain format
//! strings; there is no Fluent.

mod diagnostic;
mod emitter;

pub use diagnostic::{
    Applicability, Diag, DiagInner, ErrCode, Level, MultiSpan, SpanLabel, Subdiag, Suggestion,
};
pub use emitter::{ColorConfig, Emitter, HumanEmitter, SharedBuffer};

use std::sync::{Arc, Mutex};

use vsc_span::{SourceMap, Span};

/// Proof that an error was emitted. Code that returns this can't forget to
/// report the error it is talking about.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ErrorGuaranteed(());

impl ErrorGuaranteed {
    /// For code that has checked `DiagCtxt::has_errors` itself.
    pub fn unchecked_error_guaranteed() -> ErrorGuaranteed {
        ErrorGuaranteed(())
    }
}

/// Raised to stop compilation after a fatal error.
#[derive(Debug)]
pub struct FatalError;

impl FatalError {
    pub fn raise(self) -> ! {
        std::panic::resume_unwind(Box::new(FatalErrorMarker))
    }
}

/// The panic payload `FatalError::raise` unwinds with. The driver catches it.
pub struct FatalErrorMarker;

struct DiagCtxtInner {
    emitter: Box<dyn Emitter + Send>,
    err_count: usize,
    warn_count: usize,
}

/// Collects and prints diagnostics (rustc's `DiagCtxt`).
pub struct DiagCtxt {
    inner: Mutex<DiagCtxtInner>,
}

impl DiagCtxt {
    pub fn new(emitter: Box<dyn Emitter + Send>) -> DiagCtxt {
        DiagCtxt { inner: Mutex::new(DiagCtxtInner { emitter, err_count: 0, warn_count: 0 }) }
    }

    /// Prints to stderr in rustc's human format.
    pub fn with_stderr(source_map: Arc<SourceMap>, color: ColorConfig) -> DiagCtxt {
        DiagCtxt::new(Box::new(HumanEmitter::stderr(color).with_source_map(source_map)))
    }

    pub fn struct_err(&self, msg: impl Into<String>) -> Diag<'_> {
        Diag::new(self, Level::Error, msg)
    }

    pub fn struct_span_err(&self, span: impl Into<MultiSpan>, msg: impl Into<String>) -> Diag<'_> {
        Diag::new(self, Level::Error, msg).with_span(span)
    }

    pub fn struct_warn(&self, msg: impl Into<String>) -> Diag<'_> {
        Diag::new(self, Level::Warning, msg)
    }

    pub fn struct_span_warn(&self, span: impl Into<MultiSpan>, msg: impl Into<String>) -> Diag<'_> {
        Diag::new(self, Level::Warning, msg).with_span(span)
    }

    pub fn struct_fatal(&self, msg: impl Into<String>) -> Diag<'_> {
        Diag::new(self, Level::Fatal, msg)
    }

    pub fn struct_note(&self, msg: impl Into<String>) -> Diag<'_> {
        Diag::new(self, Level::Note, msg)
    }

    pub fn err(&self, msg: impl Into<String>) -> ErrorGuaranteed {
        self.struct_err(msg).emit()
    }

    pub fn span_err(&self, span: impl Into<MultiSpan>, msg: impl Into<String>) -> ErrorGuaranteed {
        self.struct_span_err(span, msg).emit()
    }

    pub fn warn(&self, msg: impl Into<String>) {
        self.struct_warn(msg).emit();
    }

    pub fn span_warn(&self, span: impl Into<MultiSpan>, msg: impl Into<String>) {
        self.struct_span_warn(span, msg).emit();
    }

    /// Emits a fatal error and stops compilation.
    pub fn fatal(&self, msg: impl Into<String>) -> ! {
        self.struct_fatal(msg).emit();
        FatalError.raise()
    }

    /// An internal compiler error: a bug in vsc, not in the program.
    pub fn bug(&self, msg: impl Into<String>) -> ! {
        Diag::new(self, Level::Bug, msg).emit();
        panic!("internal compiler error")
    }

    pub fn span_bug(&self, span: Span, msg: impl Into<String>) -> ! {
        Diag::new(self, Level::Bug, msg).with_span(span).emit();
        panic!("internal compiler error")
    }

    pub fn emit_diagnostic(&self, diag: DiagInner) -> Option<ErrorGuaranteed> {
        let mut inner = self.inner.lock().unwrap();
        let guar = match diag.level {
            Level::Bug | Level::Fatal | Level::Error => {
                inner.err_count += 1;
                Some(ErrorGuaranteed(()))
            }
            Level::Warning => {
                inner.warn_count += 1;
                None
            }
            Level::Note | Level::Help | Level::FailureNote => None,
        };
        inner.emitter.emit_diagnostic(&diag);
        guar
    }

    pub fn err_count(&self) -> usize {
        self.inner.lock().unwrap().err_count
    }

    pub fn warn_count(&self) -> usize {
        self.inner.lock().unwrap().warn_count
    }

    pub fn has_errors(&self) -> Option<ErrorGuaranteed> {
        (self.err_count() > 0).then_some(ErrorGuaranteed(()))
    }

    pub fn abort_if_errors(&self) {
        if self.has_errors().is_some() {
            FatalError.raise();
        }
    }

    /// rustc's closing line: `error: aborting due to 2 previous errors; 1 warning emitted`.
    pub fn print_error_count(&self) {
        let (errors, warnings) = (self.err_count(), self.warn_count());
        let plural = |n: usize, word: &str| if n == 1 { format!("{n} {word}") } else { format!("{n} {word}s") };
        let msg = match (errors, warnings) {
            (0, 0) => return,
            (0, w) => format!("{} emitted", plural(w, "warning")),
            (1, 0) => "aborting due to 1 previous error".to_string(),
            (e, 0) => format!("aborting due to {e} previous errors"),
            (e, w) => {
                let errs = if e == 1 { "1 previous error".to_string() } else { format!("{e} previous errors") };
                format!("aborting due to {errs}; {} emitted", plural(w, "warning"))
            }
        };
        let level = if errors > 0 { Level::Error } else { Level::Warning };
        let diag = DiagInner::new(level, msg);
        self.inner.lock().unwrap().emitter.emit_diagnostic(&diag);
    }
}

#[cfg(test)]
mod tests;
