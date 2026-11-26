//! Function and global declaration/definition helpers (`vsc_codegen_obsidian::declare`).

use obsidian::ir;
use vsc_middle::middle::codegen_fn_attrs::Linkage;
use vsc_middle::mono::Visibility;
use vsc_middle::ty::{Instance, Ty};
use vsc_target::callconv::FnAbi;

use crate::abi::{fn_abi_obs_type, to_obs_callconv};
use crate::context::CodegenCx;
use crate::value::{ObsType, ObsVal};

/// Translates MIR [`Linkage`] into [`obsidian::ir::Linkage`].
pub const fn linkage_to_obs(linkage: Linkage) -> ir::Linkage {
    match linkage {
        Linkage::Internal => ir::Linkage::Local,
        Linkage::LinkOnceAny | Linkage::LinkOnceODR | Linkage::WeakAny | Linkage::WeakODR => {
            ir::Linkage::Weak
        }
        Linkage::AvailableExternally | Linkage::ExternalWeak => ir::Linkage::Extern,
        Linkage::External | Linkage::Common => ir::Linkage::Export,
    }
}

impl<'tcx> CodegenCx<'tcx> {
    /// Declares (or retrieves) a function with the given symbol name and `FnAbi`.
    pub fn declare_fn(&self, name: &str, fn_abi: &FnAbi<'tcx, Ty<'tcx>>) -> ObsVal {
        let fn_ty = fn_abi_obs_type(self, fn_abi);
        let cc = to_obs_callconv(fn_abi.conv);
        self.declare_raw_fn(name, cc, fn_ty)
    }

    /// Declares (or retrieves) a raw C/intrinsic function by name, calling convention, and [`ObsType::Func`].
    pub fn declare_raw_fn(&self, name: &str, cc: ir::CallConv, fn_ty: ObsType) -> ObsVal {
        let ObsType::Func(sig_id) = fn_ty else {
            panic!("declare_raw_fn requires ObsType::Func, got {fn_ty:?}");
        };
        let sig = self.get_fn_sig(sig_id);
        let sym = self.module.borrow_mut().declare_function(
            name,
            ir::Linkage::Extern,
            cc,
            &sig.params,
            &sig.returns,
        );
        self.sym_fn_sigs.borrow_mut().insert(sym, sig_id);
        ObsVal::sym_addr(sym, false)
    }

    /// Declares and configures linkage/attributes for a resolved monomorphized function `Instance`.
    pub fn get_fn(&self, instance: Instance<'tcx>) -> ObsVal {
        if let Some(&llfn) = self.instances.borrow().get(&instance) {
            return llfn;
        }
        let sym_name = self.tcx.symbol_name(instance).name;
        let fn_abi = self.fn_abi_of_instance(instance);
        let llfn = self.declare_fn(sym_name, fn_abi);
        let attrs = self.tcx.codegen_instance_attrs(instance.def);
        if let Some((func_sym, _, _)) = llfn.as_sym() {
            self.with_func_mut(func_sym, |func| {
                if let Some(sec) = attrs.link_section {
                    func.attrs.section = Some(sec.as_str().to_string());
                }
            });
        }
        self.instances.borrow_mut().insert(instance, llfn);
        llfn
    }

    /// Sets linkage on a declared global or function [`ObsVal`].
    pub fn set_linkage_and_visibility(&self, val: ObsVal, linkage: Linkage, _vis: Visibility) {
        let Some((sym, _, _)) = val.as_sym() else {
            return;
        };
        let obs_linkage = linkage_to_obs(linkage);
        let mut module = self.module.borrow_mut();
        match module.symbols[sym.index()].1 {
            ir::SymKind::Function(idx) => {
                module.functions[idx].linkage = obs_linkage;
            }
            ir::SymKind::Global(idx) => {
                module.globals[idx].linkage = obs_linkage;
            }
            ir::SymKind::Tls(idx) => {
                module.tls_vars[idx].linkage = obs_linkage;
            }
            _ => {}
        }
    }
}
