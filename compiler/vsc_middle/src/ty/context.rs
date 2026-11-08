//! The type context (rustc's `ty/context.rs`).
//!
//! `TyCtxt` is a copyable handle to the `GlobalCtxt`, which owns the
//! interners, the crate's HIR and definitions, and the query caches. Every
//! interned value lives in the `Arena`, which outlives the context:
//!
//! ```ignore
//! let arena = Arena::default();
//! let gcx_cell = OnceCell::new();
//! let tcx = TyCtxt::create(&arena, &gcx_cell, CtxtInputs { … });
//! ```

use std::cell::{OnceCell, RefCell};
use std::ops::Deref;

use vsc_data_structures::{FxHashMap, FxHashSet};
use vsc_errors::{DiagCtxt, ErrCode};
use vsc_hir::def::DefKind;
use vsc_hir::{self as hir, LangItem};
use vsc_index::IndexVec;
use vsc_session::ParseSess;
use vsc_session::config::TargetInfo;
use vsc_span::def_id::{CRATE_DEF_ID, DefId, LocalDefId};
use vsc_span::{Span, Symbol, kw};

use super::*;
use crate::arena::Arena;
use crate::hir::map::{self, Node};
use crate::middle::lang_items::LanguageItems;
use crate::query::{Providers, QueryCache};

/// What resolution knew about a definition.
#[derive(Clone, Debug)]
pub struct DefInfo {
    pub kind: DefKind,
    pub parent: Option<LocalDefId>,
    /// `kw::Empty` for definitions without a name.
    pub name: Symbol,
    pub span: Span,
}

#[derive(Clone, Copy)]
pub struct TyCtxt<'tcx> {
    gcx: &'tcx GlobalCtxt<'tcx>,
}

impl<'tcx> Deref for TyCtxt<'tcx> {
    type Target = &'tcx GlobalCtxt<'tcx>;
    fn deref(&self) -> &Self::Target {
        &self.gcx
    }
}

/// What the context is made from.
pub struct CtxtInputs<'tcx> {
    pub sess: &'tcx ParseSess,
    pub target: &'tcx TargetInfo,
    pub hir: &'tcx hir::Crate,
    pub defs: IndexVec<LocalDefId, DefInfo>,
    pub crate_name: Symbol,
    pub providers: Providers,
}

pub struct GlobalCtxt<'tcx> {
    pub arena: &'tcx Arena<'tcx>,
    pub sess: &'tcx ParseSess,
    pub target: &'tcx TargetInfo,
    pub hir_crate: &'tcx hir::Crate,
    pub defs: IndexVec<LocalDefId, DefInfo>,
    pub crate_name: Symbol,
    pub types: CommonTypes<'tcx>,
    pub regions: CommonRegions<'tcx>,
    providers: Providers,
    interners: Interners<'tcx>,
    hir_nodes: FxHashMap<LocalDefId, Node<'tcx>>,
    lang_items: OnceCell<&'tcx LanguageItems>,
    missing_lang_items: RefCell<FxHashSet<LangItem>>,
    caches: QueryCaches<'tcx>,
}

#[derive(Default)]
struct Interners<'tcx> {
    ty: RefCell<FxHashSet<&'tcx TyKind<'tcx>>>,
    region: RefCell<FxHashSet<&'tcx RegionKind>>,
    konst: RefCell<FxHashSet<&'tcx ConstKind<'tcx>>>,
    args: RefCell<FxHashSet<&'tcx [GenericArg<'tcx>]>>,
    type_lists: RefCell<FxHashSet<&'tcx [Ty<'tcx>]>>,
    existentials: RefCell<FxHashSet<&'tcx [ExistentialPredicate<'tcx>]>>,
    bound_vars: RefCell<FxHashSet<&'tcx [BoundVar<'tcx>]>>,
}

#[derive(Default)]
struct QueryCaches<'tcx> {
    generics_of: QueryCache<DefId, &'tcx Generics>,
    type_of: QueryCache<DefId, Ty<'tcx>>,
    fn_sig: QueryCache<DefId, PolyFnSig<'tcx>>,
    predicates_of: QueryCache<DefId, GenericPredicates<'tcx>>,
    adt_def: QueryCache<DefId, AdtDef<'tcx>>,
    impl_trait_ref: QueryCache<DefId, Option<TraitRef<'tcx>>>,
    associated_items: QueryCache<DefId, &'tcx [AssocItem]>,
    typeck: QueryCache<DefId, &'tcx TypeckResults<'tcx>>,
    thir_body: QueryCache<DefId, &'tcx crate::thir::Thir<'tcx>>,
    mir_built: QueryCache<DefId, &'tcx crate::mir::Body<'tcx>>,
}

pub struct CommonTypes<'tcx> {
    pub unit: Ty<'tcx>,
    pub bool: Ty<'tcx>,
    pub char: Ty<'tcx>,
    pub isize: Ty<'tcx>,
    pub usize: Ty<'tcx>,
    pub u32: Ty<'tcx>,
    pub str_: Ty<'tcx>,
    pub never: Ty<'tcx>,
    pub error: Ty<'tcx>,
    /// A trait's `Self`: parameter 0.
    pub self_param: Ty<'tcx>,
}

pub struct CommonRegions<'tcx> {
    pub re_static: Region<'tcx>,
    pub re_erased: Region<'tcx>,
    pub re_error: Region<'tcx>,
}

impl<'tcx> TyCtxt<'tcx> {
    pub fn create(arena: &'tcx Arena<'tcx>, gcx_cell: &'tcx OnceCell<GlobalCtxt<'tcx>>, inputs: CtxtInputs<'tcx>) -> TyCtxt<'tcx> {
        let interners = Interners::default();
        let mk = |k: TyKind<'tcx>| intern_ty(arena, &interners, k);
        let types = CommonTypes {
            unit: mk(TyKind::Tuple(&[])),
            bool: mk(TyKind::Bool),
            char: mk(TyKind::Char),
            isize: mk(TyKind::Int(IntTy::Isize)),
            usize: mk(TyKind::Uint(UintTy::Usize)),
            u32: mk(TyKind::Uint(UintTy::U32)),
            str_: mk(TyKind::Str),
            never: mk(TyKind::Never),
            error: mk(TyKind::Error),
            self_param: mk(TyKind::Param(ParamTy { index: 0, name: kw::SelfUpper })),
        };
        let mk_r = |k: RegionKind| intern_region(arena, &interners, k);
        let regions = CommonRegions {
            re_static: mk_r(RegionKind::ReStatic),
            re_erased: mk_r(RegionKind::ReErased),
            re_error: mk_r(RegionKind::ReError),
        };
        let gcx = gcx_cell.get_or_init(|| GlobalCtxt {
            arena,
            sess: inputs.sess,
            target: inputs.target,
            hir_crate: inputs.hir,
            defs: inputs.defs,
            crate_name: inputs.crate_name,
            types,
            regions,
            providers: inputs.providers,
            interners,
            hir_nodes: map::index_crate(inputs.hir),
            lang_items: OnceCell::new(),
            missing_lang_items: RefCell::new(FxHashSet::default()),
            caches: QueryCaches::default(),
        });
        TyCtxt { gcx }
    }

    pub fn dcx(self) -> &'tcx DiagCtxt {
        &self.gcx.sess.dcx
    }

    // -----------------------------------------------------------------------
    // Interning
    // -----------------------------------------------------------------------

    pub fn mk_ty(self, kind: TyKind<'tcx>) -> Ty<'tcx> {
        intern_ty(self.gcx.arena, &self.gcx.interners, kind)
    }

    pub fn mk_region(self, kind: RegionKind) -> Region<'tcx> {
        intern_region(self.gcx.arena, &self.gcx.interners, kind)
    }

    pub fn mk_const(self, kind: ConstKind<'tcx>) -> Const<'tcx> {
        let set = &self.gcx.interners.konst;
        if let Some(&k) = set.borrow().get(&kind) {
            return Const(k);
        }
        let k = &*self.gcx.arena.const_kinds.alloc(kind);
        set.borrow_mut().insert(k);
        Const(k)
    }

    pub fn mk_args(self, args: &[GenericArg<'tcx>]) -> GenericArgsRef<'tcx> {
        intern_slice(&self.gcx.interners.args, &self.gcx.arena.generic_args, args)
    }

    pub fn mk_type_list(self, tys: &[Ty<'tcx>]) -> &'tcx [Ty<'tcx>] {
        intern_slice(&self.gcx.interners.type_lists, &self.gcx.arena.tys, tys)
    }

    pub fn mk_existentials(self, preds: &[ExistentialPredicate<'tcx>]) -> &'tcx [ExistentialPredicate<'tcx>] {
        intern_slice(&self.gcx.interners.existentials, &self.gcx.arena.existentials, preds)
    }

    pub fn mk_bound_vars(self, vars: &[BoundVar<'tcx>]) -> &'tcx [BoundVar<'tcx>] {
        intern_slice(&self.gcx.interners.bound_vars, &self.gcx.arena.bound_vars, vars)
    }

    pub fn alloc_u32s(self, v: &[u32]) -> &'tcx [u32] {
        self.gcx.arena.u32s.alloc_from_iter(v.iter().copied())
    }

    pub fn alloc_clauses(self, v: Vec<(Clause<'tcx>, Span)>) -> &'tcx [(Clause<'tcx>, Span)] {
        self.gcx.arena.clauses.alloc_from_iter(v)
    }

    pub fn mk_adt(self, def: AdtDef<'tcx>, args: GenericArgsRef<'tcx>) -> Ty<'tcx> {
        self.mk_ty(TyKind::Adt(def, args))
    }

    pub fn mk_ref(self, r: Region<'tcx>, ty: Ty<'tcx>, m: Mutability) -> Ty<'tcx> {
        self.mk_ty(TyKind::Ref(r, ty, m))
    }

    pub fn mk_tup(self, tys: &[Ty<'tcx>]) -> Ty<'tcx> {
        let list = self.mk_type_list(tys);
        self.mk_ty(TyKind::Tuple(list))
    }

    pub fn mk_param(self, index: u32, name: Symbol) -> Ty<'tcx> {
        self.mk_ty(TyKind::Param(ParamTy { index, name }))
    }

    pub fn mk_fn_def(self, def: DefId, args: GenericArgsRef<'tcx>) -> Ty<'tcx> {
        self.mk_ty(TyKind::FnDef(def, args))
    }

    pub fn mk_early_region(self, index: u32, name: Symbol) -> Region<'tcx> {
        self.mk_region(RegionKind::ReEarlyParam(EarlyParamRegion { index, name }))
    }

    /// The item's own parameters as arguments: `S<T>` inside `S`'s
    /// definition.
    pub fn identity_args(self, def: DefId) -> GenericArgsRef<'tcx> {
        let g = self.generics_of(def);
        let args: Vec<_> = (0..g.count())
            .map(|i| {
                let p = g.param_at(i, self);
                match p.kind {
                    GenericParamDefKind::Origin => GenericArg::Region(self.mk_early_region(p.index, p.name)),
                    GenericParamDefKind::Type { .. } => GenericArg::Type(self.mk_param(p.index, p.name)),
                    GenericParamDefKind::Const { .. } => {
                        GenericArg::Const(self.mk_const(ConstKind::Param(ParamConst { index: p.index, name: p.name })))
                    }
                }
            })
            .collect();
        self.mk_args(&args)
    }

    // -----------------------------------------------------------------------
    // Definitions
    // -----------------------------------------------------------------------

    #[track_caller]
    fn info(self, def: DefId) -> &'tcx DefInfo {
        &self.gcx.defs[def.as_local().unwrap_or_else(|| panic!("{def:?} is in another crate, which vsc-v2 can't read yet"))]
    }

    pub fn def_kind(self, def: DefId) -> DefKind {
        self.info(def).kind
    }

    pub fn def_span(self, def: DefId) -> Span {
        self.info(def).span
    }

    pub fn opt_parent(self, def: DefId) -> Option<DefId> {
        self.info(def).parent.map(LocalDefId::to_def_id)
    }

    #[track_caller]
    pub fn parent(self, def: DefId) -> DefId {
        self.opt_parent(def).unwrap_or_else(|| panic!("{def:?} has no parent"))
    }

    pub fn item_name(self, def: DefId) -> Symbol {
        self.info(def).name
    }

    pub fn hir_node(self, def: LocalDefId) -> Option<Node<'tcx>> {
        self.gcx.hir_nodes.get(&def).copied()
    }

    /// The body a definition owns: a function's, constant's, static's or
    /// anonymous constant's (function literals' bodies belong to their
    /// enclosing body's owner).
    pub fn body_owned_by(self, def: LocalDefId) -> Option<hir::BodyId> {
        match self.hir_node(def)? {
            Node::Item(i) => match &i.kind {
                hir::ItemKind::Fn { body, .. } | hir::ItemKind::Const(_, _, body) | hir::ItemKind::Static(_, _, body) => Some(*body),
                _ => None,
            },
            Node::AssocItem(a) => match &a.kind {
                hir::AssocItemKind::Fn(_, body) | hir::AssocItemKind::Const(_, body) => *body,
                hir::AssocItemKind::Type(..) => None,
            },
            Node::AnonConst(c) => Some(c.body),
            _ => None,
        }
    }

    /// The attributes on an item-like definition.
    pub fn owner_attrs(self, def: LocalDefId) -> &'tcx [vsc_ast::Attribute] {
        match self.gcx.hir_crate.owners.get(def).and_then(|o| o.as_ref()) {
            Some(o) => o.attrs.get(&hir::ItemLocalId::ZERO).map_or(&[], |v| v.as_slice()),
            None => &[],
        }
    }

    /// The path of a definition as Vertex writes it: `shapes.Circle`,
    /// `Status.Done`, `Task.New`.
    pub fn def_path_str(self, def: DefId) -> String {
        let info = self.info(def);
        let Some(parent) = info.parent else { return self.crate_name.to_string() };
        let name = info.name;
        if name == kw::Empty {
            return match info.kind {
                DefKind::Closure => format!("{}.{{function literal}}", self.def_path_str(parent.to_def_id())),
                DefKind::AnonConst | DefKind::InlineConst => "{constant}".to_string(),
                // A constructor is named by its struct or variant.
                _ => self.def_path_str(parent.to_def_id()),
            };
        }
        let pinfo = &self.gcx.defs[parent];
        let prefix = match pinfo.kind {
            _ if parent == CRATE_DEF_ID => None,
            DefKind::Impl { .. } => Some(self.impl_self_name(parent)),
            DefKind::ForeignMod => match pinfo.parent {
                Some(p) if p != CRATE_DEF_ID => Some(self.def_path_str(p.to_def_id())),
                _ => None,
            },
            _ => Some(self.def_path_str(parent.to_def_id())),
        };
        match prefix {
            Some(p) => format!("{p}.{name}"),
            None => name.to_string(),
        }
    }

    /// An impl's self type's name, from the HIR (asking for its type here
    /// could be a cycle).
    fn impl_self_name(self, imp: LocalDefId) -> String {
        if let Some(Node::Item(hir::Item { kind: hir::ItemKind::Impl(i), .. })) = self.hir_node(imp) {
            if let hir::TyKind::Path(hir::QPath::Resolved(None, p)) = &i.self_ty.kind {
                return p.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>().join(".");
            }
        }
        "{implementation}".to_string()
    }

    // -----------------------------------------------------------------------
    // Lang items
    // -----------------------------------------------------------------------

    pub fn lang_items(self) -> &'tcx LanguageItems {
        self.gcx.lang_items.get_or_init(|| {
            let items = (self.gcx.providers.lang_items)(self);
            self.gcx.arena.lang_items.alloc(items)
        })
    }

    /// A lang item the code at `span` needs. A missing one is reported
    /// once.
    pub fn require_lang_item(self, item: LangItem, span: Span) -> Option<DefId> {
        if let Some(d) = self.lang_items().get(item) {
            return Some(d);
        }
        if self.gcx.missing_lang_items.borrow_mut().insert(item) {
            self.dcx()
                .struct_span_err(span, format!("requires the `{}` lang item", item.name()))
                .with_note(format!(
                    "the standard library defines it; until vsc-v2 can build `library/`, a crate can define it with `@lang(\"{}\")` on a {}",
                    item.name(),
                    item.target()
                ))
                .emit();
        }
        None
    }

    pub fn is_lang_item(self, def: DefId, item: LangItem) -> bool {
        self.lang_items().get(item) == Some(def)
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    fn report_cycle(self, what: &str, def: DefId) {
        self.dcx()
            .struct_span_err(self.def_span(def), format!("cycle detected when computing {what} `{}`", self.def_path_str(def)))
            .with_code(ErrCode(391))
            .emit();
    }

    pub fn generics_of(self, def: DefId) -> &'tcx Generics {
        self.caches.generics_of.get(
            def,
            || self.gcx.arena.generics.alloc((self.providers.generics_of)(self, def)),
            || {
                self.report_cycle("the generics of", def);
                self.gcx.arena.generics.alloc(Generics {
                    parent: None,
                    parent_count: 0,
                    own_params: Vec::new(),
                    param_def_id_to_index: FxHashMap::default(),
                    has_self: false,
                })
            },
        )
    }

    pub fn type_of(self, def: DefId) -> Ty<'tcx> {
        self.caches.type_of.get(
            def,
            || (self.providers.type_of)(self, def),
            || {
                self.report_cycle("the type of", def);
                self.types.error
            },
        )
    }

    pub fn fn_sig(self, def: DefId) -> PolyFnSig<'tcx> {
        self.caches.fn_sig.get(
            def,
            || (self.providers.fn_sig)(self, def),
            || {
                self.report_cycle("the signature of", def);
                let io = self.mk_type_list(&[self.types.error]);
                Binder::dummy(FnSig { inputs_and_output: io, c_variadic: false, safety: Safety::Safe, abi: None })
            },
        )
    }

    pub fn predicates_of(self, def: DefId) -> GenericPredicates<'tcx> {
        self.caches.predicates_of.get(
            def,
            || (self.providers.predicates_of)(self, def),
            || {
                self.report_cycle("the predicates of", def);
                GenericPredicates { parent: None, predicates: &[] }
            },
        )
    }

    pub fn adt_def(self, def: DefId) -> AdtDef<'tcx> {
        self.caches.adt_def.get(
            def,
            || AdtDef(self.gcx.arena.adt_defs.alloc((self.providers.adt_def)(self, def))),
            || unreachable!("an ADT's definition doesn't depend on itself"),
        )
    }

    pub fn impl_trait_ref(self, def: DefId) -> Option<TraitRef<'tcx>> {
        self.caches.impl_trait_ref.get(
            def,
            || (self.providers.impl_trait_ref)(self, def),
            || {
                self.report_cycle("the trait implemented by", def);
                None
            },
        )
    }

    /// The types in a body (a function's, constant's or static's), after
    /// inference. A function literal's are its enclosing body's.
    pub fn typeck(self, def: DefId) -> &'tcx TypeckResults<'tcx> {
        let root = self.typeck_root_def_id(def);
        self.caches.typeck.get(
            root,
            || self.gcx.arena.typeck_results.alloc((self.providers.typeck)(self, root)),
            || {
                self.report_cycle("the types in", root);
                self.gcx.arena.typeck_results.alloc(TypeckResults::new(root.expect_local()))
            },
        )
    }

    /// A body as THIR. Function literals have their own.
    pub fn thir_body(self, def: DefId) -> &'tcx crate::thir::Thir<'tcx> {
        self.caches.thir_body.get(
            def,
            || self.gcx.arena.thir.alloc((self.providers.thir_body)(self, def)),
            || unreachable!("building THIR doesn't depend on THIR"),
        )
    }

    /// A body's MIR, as built from THIR.
    pub fn mir_built(self, def: DefId) -> &'tcx crate::mir::Body<'tcx> {
        self.caches.mir_built.get(
            def,
            || self.gcx.arena.mir.alloc((self.providers.mir_built)(self, def)),
            || unreachable!("building MIR doesn't depend on itself"),
        )
    }

    /// The body a function literal or inline constant is type checked with.
    pub fn typeck_root_def_id(self, mut def: DefId) -> DefId {
        while matches!(self.def_kind(def), DefKind::Closure | DefKind::InlineConst) {
            def = self.parent(def);
        }
        def
    }

    /// The associated items of a trait or impl, in order.
    pub fn associated_items(self, def: DefId) -> &'tcx [AssocItem] {
        self.caches.associated_items.get(
            def,
            || self.gcx.arena.assoc_items.alloc((self.providers.associated_items)(self, def)).as_slice(),
            || &[],
        )
    }
}

fn intern_ty<'tcx>(arena: &'tcx Arena<'tcx>, interners: &Interners<'tcx>, kind: TyKind<'tcx>) -> Ty<'tcx> {
    if let Some(&k) = interners.ty.borrow().get(&kind) {
        return Ty(k);
    }
    let k = &*arena.ty_kinds.alloc(kind);
    interners.ty.borrow_mut().insert(k);
    Ty(k)
}

fn intern_region<'tcx>(arena: &'tcx Arena<'tcx>, interners: &Interners<'tcx>, kind: RegionKind) -> Region<'tcx> {
    if let Some(&k) = interners.region.borrow().get(&kind) {
        return Region(k);
    }
    let k = &*arena.region_kinds.alloc(kind);
    interners.region.borrow_mut().insert(k);
    Region(k)
}

fn intern_slice<'tcx, T: Copy + Eq + std::hash::Hash>(
    set: &RefCell<FxHashSet<&'tcx [T]>>,
    arena: &'tcx vsc_arena::TypedArena<T>,
    items: &[T],
) -> &'tcx [T] {
    if items.is_empty() {
        return &[];
    }
    if let Some(&s) = set.borrow().get(items) {
        return s;
    }
    let s = &*arena.alloc_from_iter(items.iter().copied());
    set.borrow_mut().insert(s);
    s
}
