//! Compiler session configuration and driver entry point (`rustc_interface::interface`).

use std::path::PathBuf;
use std::sync::Arc;

use vsc_codegen_obsidian::ObsidianCodegenBackend;
use vsc_codegen_ssa::traits::CodegenBackend;
use vsc_errors::{DiagCtxt, HumanEmitter};
use vsc_middle::query::Providers;
use vsc_session::config::{ErrorFormat, Options};
use vsc_session::{ParseSess, Session};
use vsc_span::SourceMap;

/// Primary crate source input passed into [`Config`].
#[derive(Clone, Debug)]
pub enum Input {
    File(PathBuf),
}

/// Session and backend configuration passed to [`run_compiler`] (`rustc_interface::Config`).
pub struct Config {
    pub opts: Options,
    pub input: Input,
    pub override_queries: Option<fn(&Session, &mut Providers)>,
    pub make_codegen_backend:
        Option<Box<dyn FnOnce(&Session) -> Box<dyn CodegenBackend> + Send>>,
}

/// Live compiler state for a single session (`rustc_interface::Compiler`).
pub struct Compiler {
    pub sess: Session,
    pub codegen_backend: Box<dyn CodegenBackend>,
    pub(crate) override_queries: Option<fn(&Session, &mut Providers)>,
}

impl Compiler {
    /// Returns the diagnostic context bound to this session.
    pub fn dcx(&self) -> &DiagCtxt {
        self.sess.dcx()
    }
}

/// Builds a [`Session`] and codegen backend from [`Config`], runs `f(&compiler)`,
/// and returns its value. Emits a fatal diagnostic if target setup fails.
pub fn run_compiler<R>(config: Config, f: impl FnOnce(&Compiler) -> R) -> R {
    let color = config.opts.color;
    let short = config.opts.error_format == ErrorFormat::Short;
    let sm = Arc::new(SourceMap::new());
    let emitter = HumanEmitter::stderr(color).with_source_map(sm.clone()).short(short);
    let psess = ParseSess::new(sm.clone(), DiagCtxt::new(Box::new(emitter)));

    let sess = match Session::new(config.opts, psess) {
        Ok(sess) => sess,
        Err(err) => {
            let emitter = HumanEmitter::stderr(color).with_source_map(sm).short(short);
            let dcx = DiagCtxt::new(Box::new(emitter));
            dcx.fatal(err);
        }
    };

    let codegen_backend = match config.make_codegen_backend {
        Some(make) => make(&sess),
        None => Box::new(ObsidianCodegenBackend::new()),
    };

    let compiler = Compiler {
        sess,
        codegen_backend,
        override_queries: config.override_queries,
    };
    f(&compiler)
}
