//! What the parser needs from the session.

use std::sync::Arc;

use vsc_errors::{ColorConfig, DiagCtxt, Emitter, HumanEmitter};
use vsc_span::SourceMap;

/// The source map and the diagnostics context, shared by everything that
/// parses (rustc's `ParseSess`).
pub struct ParseSess {
    pub source_map: Arc<SourceMap>,
    pub dcx: DiagCtxt,
}

impl ParseSess {
    pub fn new(source_map: Arc<SourceMap>, dcx: DiagCtxt) -> ParseSess {
        ParseSess { source_map, dcx }
    }

    /// A session that prints diagnostics to stderr.
    pub fn with_stderr(color: ColorConfig) -> ParseSess {
        let sm = Arc::new(SourceMap::new());
        let dcx = DiagCtxt::with_stderr(sm.clone(), color);
        ParseSess::new(sm, dcx)
    }

    /// A session whose diagnostics go to `emitter`, which is given this
    /// session's source map.
    pub fn with_emitter(make: impl FnOnce(Arc<SourceMap>) -> Box<dyn Emitter + Send>) -> ParseSess {
        let sm = Arc::new(SourceMap::new());
        let dcx = DiagCtxt::new(make(sm.clone()));
        ParseSess::new(sm, dcx)
    }

    /// A session that writes human diagnostics, without color, into a buffer.
    pub fn with_buffer(buf: vsc_errors::SharedBuffer) -> ParseSess {
        ParseSess::with_emitter(|sm| Box::new(HumanEmitter::new(Box::new(buf), false).with_source_map(sm)))
    }
}
