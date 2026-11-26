//! Post-analysis codegen and native linking orchestration (`rustc_interface::queries`).

use std::any::Any;
use std::sync::Arc;

use vsc_codegen_ssa::traits::CodegenBackend;
use vsc_codegen_ssa::{CompiledModules, CrateInfo};
use vsc_middle::ty::TyCtxt;
use vsc_session::Session;
use vsc_session::config::OutputFilenames;

use crate::passes;

/// Holds ongoing codegen state and output metadata across the `TyCtxt` boundary,
/// allowing front-end/middle-end arenas to drop before native linking runs.
pub struct Linker {
    output_filenames: Arc<OutputFilenames>,
    crate_info: CrateInfo,
    ongoing_codegen: Box<dyn Any>,
}

impl Linker {
    /// Drives `codegen_backend.codegen_crate` and snapshots linking metadata.
    pub fn codegen_and_build_linker(
        tcx: TyCtxt<'_>,
        sess: &Session,
        codegen_backend: &dyn CodegenBackend,
    ) -> Linker {
        let (ongoing_codegen, crate_info, output_filenames) =
            passes::start_codegen(codegen_backend, tcx, sess);
        Linker { output_filenames, crate_info, ongoing_codegen }
    }

    /// Finalizes compiled modules (`join_codegen`) and links native artifacts when requested.
    pub fn link(self, sess: &Session, codegen_backend: &dyn CodegenBackend) {
        let compiled_modules = match self.ongoing_codegen.downcast::<CompiledModules>() {
            Ok(compiled_modules) => *compiled_modules,
            Err(ongoing_codegen) => codegen_backend.join_codegen(
                ongoing_codegen,
                sess,
                &self.output_filenames,
                &self.crate_info,
            ),
        };

        if sess.dcx().has_errors().is_some() {
            return;
        }

        codegen_backend.link(sess, compiled_modules, self.crate_info, &self.output_filenames);
    }
}
