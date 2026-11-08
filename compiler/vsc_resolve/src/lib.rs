//! Name resolution (rustc's `rustc_resolve`).
//!
//! 1. `def_collector`: a `LocalDefId` for every definition.
//! 2. `build_reduced_graph`: each package's, enum's and trait's names.
//! 3. `imports`: each file's imports (exception 1: imports belong to their
//!    file, as in Go).
//! 4. `late`: every name in signatures and bodies.
//!
//! Exception 7 is handled in `late`: a path `a.b.c` resolves as far as it
//! names packages, enums and traits; whatever is left (fields, methods,
//! associated items) is recorded as unresolved segments of a `PartialRes`.
//! Exception 13 is handled there too: `[a]` names an origin parameter or a
//! function parameter.

mod build_reduced_graph;
mod def_collector;
mod diagnostics;
pub mod dump;
mod ident;
mod imports;
mod late;

#[cfg(test)]
mod tests;

use vsc_ast::{Crate, NodeId};
use vsc_data_structures::{FxHashMap, FxHashSet, FxIndexMap};
use vsc_hir::def::{DefKind, Namespace, OriginRes, PartialRes, Res};
use vsc_index::IndexVec;
use vsc_session::ParseSess;
use vsc_span::def_id::{CRATE_DEF_ID, LocalDefId};
use vsc_span::{Span, Symbol};

/// Who can see a name (rustc's `ty::Visibility`). Exception 6: `public` is
/// `Public`, `package` is `Restricted(crate root)`, and nothing written is
/// `Restricted(the item's package)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visibility {
    Public,
    Restricted(LocalDefId),
}

/// One definition.
#[derive(Clone, Debug)]
pub struct DefData {
    pub kind: DefKind,
    pub parent: Option<LocalDefId>,
    pub name: Option<Symbol>,
    pub span: Span,
    pub node_id: NodeId,
}

/// A name in a scope, and what it means.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Binding {
    pub res: Res,
    pub vis: Visibility,
    pub span: Span,
}

/// A package (with its files' imports), an enum (with its variants) or a
/// trait (with its associated items).
pub(crate) struct ModuleData {
    pub kind: DefKind,
    pub name: Symbol,
    /// For a package, its parent package; for an enum or trait, its package.
    pub parent: Option<LocalDefId>,
    pub bindings: FxIndexMap<(Symbol, Namespace), Binding>,
    /// Child packages by folder name. They're reached through imports.
    pub children: FxHashMap<Symbol, LocalDefId>,
    /// Each file's imports (packages only).
    pub files: Vec<FileScope>,
}

#[derive(Default)]
pub(crate) struct FileScope {
    /// (bound name, namespace) → what it names (or `Res::Err` if the import failed).
    pub imports: FxIndexMap<(Symbol, Namespace), (Res, Span)>,
}

/// What resolution found, for lowering and later passes.
pub struct ResolverOutputs {
    pub defs: IndexVec<LocalDefId, DefData>,
    pub node_id_to_def_id: FxHashMap<NodeId, LocalDefId>,
    /// For every resolved path: keyed by the id of the expression, type or
    /// pattern holding it.
    pub partial_res_map: FxHashMap<NodeId, PartialRes>,
    /// For every origin name (`a` in `&[a] T`), keyed by its span. This also
    /// covers generic arguments that turned out to be origins (`Lines<[doc]>`,
    /// `Highlight<src>`).
    pub origin_res_map: FxHashMap<Span, OriginRes>,
    /// `break`/`continue` expression → the loop or block its label names.
    pub label_res_map: FxHashMap<NodeId, NodeId>,
    /// A flat receiver impl → the parameters its receiver type introduces
    /// (exception 8), with their kinds now known.
    pub flat_receiver_params: FxHashMap<NodeId, Vec<LocalDefId>>,
    pub visibilities: FxHashMap<LocalDefId, Visibility>,
    pub crate_name: Symbol,
}

pub(crate) struct Resolver<'a> {
    psess: &'a ParseSess,
    crate_name: Symbol,
    defs: IndexVec<LocalDefId, DefData>,
    node_id_to_def_id: FxHashMap<NodeId, LocalDefId>,
    modules: FxHashMap<LocalDefId, ModuleData>,
    /// Item node → (its package, its file index in the package).
    item_file: FxHashMap<NodeId, (LocalDefId, usize)>,
    /// Generic parameters of types and traits, for flat receivers: name, kind.
    generics_of: FxHashMap<LocalDefId, Vec<(Symbol, DefKind)>>,
    visibilities: FxHashMap<LocalDefId, Visibility>,
    partial_res_map: FxHashMap<NodeId, PartialRes>,
    origin_res_map: FxHashMap<Span, OriginRes>,
    label_res_map: FxHashMap<NodeId, NodeId>,
    flat_receiver_params: FxHashMap<NodeId, Vec<LocalDefId>>,
    /// Crates already reported as missing, to report each once.
    missing_crates: FxHashSet<Symbol>,
}

impl<'a> Resolver<'a> {
    fn new(psess: &'a ParseSess, crate_name: Symbol) -> Resolver<'a> {
        Resolver {
            psess,
            crate_name,
            defs: IndexVec::new(),
            node_id_to_def_id: FxHashMap::default(),
            modules: FxHashMap::default(),
            item_file: FxHashMap::default(),
            generics_of: FxHashMap::default(),
            visibilities: FxHashMap::default(),
            partial_res_map: FxHashMap::default(),
            origin_res_map: FxHashMap::default(),
            label_res_map: FxHashMap::default(),
            flat_receiver_params: FxHashMap::default(),
            missing_crates: FxHashSet::default(),
        }
    }

    pub(crate) fn dcx(&self) -> &'a vsc_errors::DiagCtxt {
        &self.psess.dcx
    }

    pub(crate) fn create_def(
        &mut self,
        parent: Option<LocalDefId>,
        node_id: NodeId,
        name: Option<Symbol>,
        kind: DefKind,
        span: Span,
    ) -> LocalDefId {
        let id = self.defs.push(DefData { kind, parent, name, span, node_id });
        if node_id != vsc_ast::DUMMY_NODE_ID {
            self.node_id_to_def_id.insert(node_id, id);
        }
        id
    }

    pub(crate) fn local_def_id(&self, node: NodeId) -> LocalDefId {
        self.node_id_to_def_id[&node]
    }

    /// Is `module` (a package) `ancestor` or inside it?
    pub(crate) fn is_descendant_of(&self, mut module: LocalDefId, ancestor: LocalDefId) -> bool {
        loop {
            if module == ancestor {
                return true;
            }
            match self.modules.get(&module).and_then(|m| m.parent) {
                Some(p) => module = p,
                None => return false,
            }
        }
    }

    pub(crate) fn is_accessible_from(&self, vis: Visibility, from_package: LocalDefId) -> bool {
        match vis {
            Visibility::Public => true,
            Visibility::Restricted(m) => self.is_descendant_of(from_package, m),
        }
    }
}

/// Resolves every name in the crate. Errors are reported to the session.
pub fn resolve_crate(psess: &ParseSess, krate: &Crate) -> ResolverOutputs {
    let mut r = Resolver::new(psess, krate.root.name.name);
    def_collector::collect_definitions(&mut r, krate);
    build_reduced_graph::build(&mut r, krate);
    imports::resolve_imports(&mut r, krate);
    late::resolve_bodies(&mut r, krate);
    debug_assert_eq!(r.defs[CRATE_DEF_ID].kind, DefKind::Mod);
    ResolverOutputs {
        defs: r.defs,
        node_id_to_def_id: r.node_id_to_def_id,
        partial_res_map: r.partial_res_map,
        origin_res_map: r.origin_res_map,
        label_res_map: r.label_res_map,
        flat_receiver_params: r.flat_receiver_params,
        visibilities: r.visibilities,
        crate_name: r.crate_name,
    }
}
