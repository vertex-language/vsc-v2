//! Abstract codegen backend interface (`rustc_codegen_ssa::traits::CodegenBackend`).

use std::any::Any;

use vsc_middle::query::Providers;
use vsc_middle::ty::TyCtxt;
use vsc_session::Session;
use vsc_session::config::{CrateType, OutputFilenames};

use crate::{CompiledModules, CrateInfo, TargetConfig};

pub trait CodegenBackend: Send + Sync {
    fn name(&self) -> &'static str;

    fn target_config(&self, _sess: &Session) -> TargetConfig {
        TargetConfig::default()
    }

    fn supported_crate_types(&self, _sess: &Session) -> Vec<CrateType> {
        vec![
            CrateType::Executable,
            CrateType::Dylib,
            CrateType::Rlib,
            CrateType::Staticlib,
            CrateType::Cdylib,
        ]
    }

    fn provide(&self, _providers: &mut Providers) {}

    fn codegen_crate<'tcx>(&self, tcx: TyCtxt<'tcx>) -> Box<dyn Any>;

    fn join_codegen(
        &self,
        ongoing_codegen: Box<dyn Any>,
        sess: &Session,
        outputs: &OutputFilenames,
        crate_info: &CrateInfo,
    ) -> CompiledModules;

    fn link(
        &self,
        sess: &Session,
        compiled_modules: CompiledModules,
        crate_info: CrateInfo,
        outputs: &OutputFilenames,
    ) {
        crate::back::link::link_binary(sess, compiled_modules, crate_info, outputs);
    }
}
