//! What type checking found in a body (rustc's `ty/typeck_results.rs`).

use vsc_data_structures::FxHashMap;
use vsc_hir::HirId;
use vsc_hir::def::DefKind;
use vsc_span::def_id::{DefId, LocalDefId};

use super::*;

#[derive(Debug)]
pub struct TypeckResults<'tcx> {
    pub owner: LocalDefId,
    /// The type of every expression and pattern.
    pub node_types: FxHashMap<HirId, Ty<'tcx>>,
    /// The generic arguments of each path and method call.
    pub node_args: FxHashMap<HirId, GenericArgsRef<'tcx>>,
    /// What a method call or type-relative path (`Task.New`) resolved to.
    pub type_dependent_defs: FxHashMap<HirId, (DefKind, DefId)>,
    /// Field expressions and patterns: the field's index in its variant.
    pub field_indices: FxHashMap<HirId, usize>,
    /// The implicit conversions applied to an expression's value.
    pub adjustments: FxHashMap<HirId, Vec<Adjustment<'tcx>>>,
    /// Each function literal's signature.
    pub closure_sigs: FxHashMap<LocalDefId, PolyFnSig<'tcx>>,
    /// The references a pattern looked through under default binding
    /// modes, outermost first (rustc's `pat_adjustments`).
    pub pat_adjustments: FxHashMap<HirId, Vec<Ty<'tcx>>>,
    /// How each binding binds: by value, or by reference (written `ref`, or
    /// from default binding modes).
    pub pat_binding_modes: FxHashMap<HirId, vsc_hir::BindingMode>,
    /// An error was reported while checking the body.
    pub tainted_by_errors: bool,
}

impl<'tcx> TypeckResults<'tcx> {
    pub fn new(owner: LocalDefId) -> Self {
        TypeckResults {
            owner,
            node_types: FxHashMap::default(),
            node_args: FxHashMap::default(),
            type_dependent_defs: FxHashMap::default(),
            field_indices: FxHashMap::default(),
            adjustments: FxHashMap::default(),
            closure_sigs: FxHashMap::default(),
            pat_adjustments: FxHashMap::default(),
            pat_binding_modes: FxHashMap::default(),
            tainted_by_errors: false,
        }
    }

    pub fn node_type(&self, id: HirId) -> Option<Ty<'tcx>> {
        self.node_types.get(&id).copied()
    }
}

/// An implicit conversion (rustc's `ty::adjustment`).
#[derive(Clone, Copy, Debug)]
pub struct Adjustment<'tcx> {
    pub kind: Adjust,
    /// The type after this step.
    pub target: Ty<'tcx>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Adjust {
    /// `never` to any type.
    NeverToAny,
    /// `*x`, for autoderef.
    Deref,
    /// `&x` or `&mut x`, for autoref and reborrows.
    Borrow(Mutability),
    /// `&[T; N]` to `&[T]`.
    Unsize,
    /// A function item to a function pointer.
    ReifyFnPointer,
    /// A function literal or function to a `func` value (exception 14).
    ToFunc,
}
