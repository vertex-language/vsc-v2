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
    pub target_spec: vsc_target::spec::Target,
    pub data_layout: vsc_abi::TargetDataLayout,
    pub options: vsc_session::config::Options,
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
    bound_vars: RefCell<FxHashSet<&'tcx [BoundVar]>>,
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
    mir_borrowck: QueryCache<DefId, ()>,
    optimized_mir: QueryCache<DefId, &'tcx crate::mir::Body<'tcx>>,
    closure_upvars: QueryCache<DefId, &'tcx [Ty<'tcx>]>,
    coroutine_layout: QueryCache<DefId, &'tcx crate::mir::CoroutineLayout<'tcx>>,
    item_bounds: QueryCache<DefId, GenericPredicates<'tcx>>,
    repr_options: QueryCache<DefId, vsc_abi::ReprOptions>,
    adt_destructor: QueryCache<DefId, Option<DefId>>,
    layout_of: QueryCache<
        Ty<'tcx>,
        Result<crate::ty::TyAndLayout<'tcx>, crate::ty::layout::LayoutError<'tcx>>,
    >,
    fn_abi_of_fn_ptr: QueryCache<
        (PolyFnSig<'tcx>, &'tcx [Ty<'tcx>]),
        Result<
            &'tcx crate::ty::layout::FnAbi<'tcx, Ty<'tcx>>,
            crate::ty::layout::FnAbiError<'tcx>,
        >,
    >,
    fn_abi_of_instance: QueryCache<
        (crate::ty::Instance<'tcx>, &'tcx [Ty<'tcx>]),
        Result<
            &'tcx crate::ty::layout::FnAbi<'tcx, Ty<'tcx>>,
            crate::ty::layout::FnAbiError<'tcx>,
        >,
    >,
    resolve_instance_raw:
        QueryCache<(DefId, GenericArgsRef<'tcx>), Option<crate::ty::Instance<'tcx>>>,
    needs_drop_raw: QueryCache<Ty<'tcx>, bool>,
    codegen_fn_attrs: QueryCache<DefId, &'tcx crate::middle::codegen_fn_attrs::CodegenFnAttrs>,
    symbol_name: QueryCache<crate::ty::Instance<'tcx>, crate::ty::SymbolName<'tcx>>,
    entry_fn: QueryCache<(), Option<(DefId, crate::ty::EntryFnType)>>,
    collect_and_partition_mono_items: QueryCache<(), &'tcx [crate::mono::CodegenUnit<'tcx>]>,
}

pub struct CommonTypes<'tcx> {
    pub unit: Ty<'tcx>,
    pub bool: Ty<'tcx>,
    pub char: Ty<'tcx>,
    pub isize: Ty<'tcx>,
    pub usize: Ty<'tcx>,
    pub i8: Ty<'tcx>,
    pub i16: Ty<'tcx>,
    pub i32: Ty<'tcx>,
    pub i64: Ty<'tcx>,
    pub i128: Ty<'tcx>,
    pub u8: Ty<'tcx>,
    pub u16: Ty<'tcx>,
    pub u32: Ty<'tcx>,
    pub u64: Ty<'tcx>,
    pub u128: Ty<'tcx>,
    pub f32: Ty<'tcx>,
    pub f64: Ty<'tcx>,
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
    pub fn create(
        arena: &'tcx Arena<'tcx>,
        gcx_cell: &'tcx OnceCell<GlobalCtxt<'tcx>>,
        inputs: CtxtInputs<'tcx>,
    ) -> TyCtxt<'tcx> {
        let mut options = vsc_session::config::Options::default();
        options.target_triple = Some(inputs.target.triple.to_string());
        Self::create_with_options(arena, gcx_cell, inputs, options)
    }

    pub fn create_with_options(
        arena: &'tcx Arena<'tcx>,
        gcx_cell: &'tcx OnceCell<GlobalCtxt<'tcx>>,
        inputs: CtxtInputs<'tcx>,
        options: vsc_session::config::Options,
    ) -> TyCtxt<'tcx> {
        let interners = Interners::default();
        let mk = |k: TyKind<'tcx>| intern_ty(arena, &interners, k);
        let types = CommonTypes {
            unit: mk(TyKind::Tuple(&[])),
            bool: mk(TyKind::Bool),
            char: mk(TyKind::Char),
            isize: mk(TyKind::Int(IntTy::Isize)),
            usize: mk(TyKind::Uint(UintTy::Usize)),
            i8: mk(TyKind::Int(IntTy::I8)),
            i16: mk(TyKind::Int(IntTy::I16)),
            i32: mk(TyKind::Int(IntTy::I32)),
            i64: mk(TyKind::Int(IntTy::I64)),
            i128: mk(TyKind::Int(IntTy::I128)),
            u8: mk(TyKind::Uint(UintTy::U8)),
            u16: mk(TyKind::Uint(UintTy::U16)),
            u32: mk(TyKind::Uint(UintTy::U32)),
            u64: mk(TyKind::Uint(UintTy::U64)),
            u128: mk(TyKind::Uint(UintTy::U128)),
            f32: mk(TyKind::Float(FloatTy::F32)),
            f64: mk(TyKind::Float(FloatTy::F64)),
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
        let target_spec = inputs.target.to_target().expect("target info must map to Target spec");
        let data_layout = target_spec
            .parse_data_layout()
            .expect("target spec data layout must parse");
        let gcx = gcx_cell.get_or_init(|| GlobalCtxt {
            arena,
            sess: inputs.sess,
            target: inputs.target,
            target_spec,
            data_layout,
            options,
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

    pub fn overflow_checks(self) -> bool {
        self.gcx.options.cg.overflow_checks()
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

    pub fn mk_bound_vars(self, vars: &[BoundVar]) -> &'tcx [BoundVar] {
        intern_slice(&self.gcx.interners.bound_vars, &self.gcx.arena.bound_vars, vars)
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
                    GenericParamDefKind::Lifetime => GenericArg::Region(self.mk_early_region(p.index, p.name)),
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
                DefKind::Coroutine => format!("{}.{{async}}", self.def_path_str(parent.to_def_id())),
                DefKind::OpaqueTy => format!("{}.{{opaque}}", self.def_path_str(parent.to_def_id())),
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

    pub fn mir_borrowck(self, def: DefId) {
        self.caches.mir_borrowck.get(def, || (self.providers.mir_borrowck)(self, def), || ());
    }

    pub fn optimized_mir(self, def: DefId) -> &'tcx crate::mir::Body<'tcx> {
        self.caches.optimized_mir.get(
            def,
            || self.gcx.arena.mir.alloc((self.providers.optimized_mir)(self, def)),
            || panic!("cycle computing optimized_mir({def:?})"),
        )
    }

    pub fn closure_upvars(self, def: DefId) -> &'tcx [Ty<'tcx>] {
        self.caches.closure_upvars.get(def, || (self.providers.closure_upvars)(self, def), || &[])
    }

    pub fn coroutine_layout(self, def: DefId) -> &'tcx crate::mir::CoroutineLayout<'tcx> {
        self.caches.coroutine_layout.get(
            def,
            || self.gcx.arena.coroutine_layouts.alloc((self.providers.coroutine_layout)(self, def)),
            || self.gcx.arena.coroutine_layouts.alloc(crate::mir::CoroutineLayout::default()),
        )
    }

    pub fn item_bounds(self, def: DefId) -> GenericPredicates<'tcx> {
        self.caches.item_bounds.get(
            def,
            || (self.providers.item_bounds)(self, def),
            || GenericPredicates { parent: None, predicates: &[] },
        )
    }

    /// Looks up the concrete hidden type inferred for an opaque `impl Trait` / `async func` return type.
    pub fn opaque_hidden_ty(self, opaque_def: DefId) -> Option<Ty<'tcx>> {
        let local = opaque_def.as_local()?;
        let parent = self.opt_parent(opaque_def)?;
        let root = self.typeck_root_def_id(parent);
        if self.body_owned_by(root.as_local()?).is_none() {
            return None;
        }
        self.typeck(root).concrete_opaque_types.get(&local).copied()
    }

    /// Recursively replaces `TyKind::Opaque(def, args)` with its concrete hidden type.
    pub fn reveal_opaque(self, ty: Ty<'tcx>) -> Ty<'tcx> {
        struct Reveal<'tcx> {
            tcx: TyCtxt<'tcx>,
            depth: u32,
        }
        impl<'tcx> super::fold::TypeFolder<'tcx> for Reveal<'tcx> {
            fn tcx(&self) -> TyCtxt<'tcx> {
                self.tcx
            }
            fn fold_ty(&mut self, ty: Ty<'tcx>) -> Ty<'tcx> {
                if self.depth > 32 {
                    return ty;
                }
                if let TyKind::Opaque(def, args) = *ty.kind() {
                    if let Some(parent_fn) = self.tcx.opt_parent(def)
                        && self.tcx.opt_parent(parent_fn).is_some_and(|p| self.tcx.def_kind(p) == DefKind::Trait)
                        && let Some(inst) = super::Instance::try_resolve(self.tcx, parent_fn, args)
                        && let super::InstanceKind::Item(impl_fn) = inst.def
                        && impl_fn != parent_fn
                    {
                        let impl_ret = self.tcx.erase_regions(
                            self.tcx.instantiate(
                                self.tcx.fn_sig(impl_fn).skip_binder().output(),
                                inst.args,
                            ),
                        );
                        self.depth += 1;
                        let out = self.fold_ty(impl_ret);
                        self.depth -= 1;
                        return out;
                    }
                    if let Some(hidden) = self.tcx.opaque_hidden_ty(def) {
                        let inst = self.tcx.erase_regions(self.tcx.instantiate(hidden, args));
                        self.depth += 1;
                        let out = self.fold_ty(inst);
                        self.depth -= 1;
                        return out;
                    }
                }
                super::fold::super_fold_ty(self, ty)
            }
        }
        use super::fold::TypeFolder;
        Reveal { tcx: self, depth: 0 }.fold_ty(ty)
    }

    pub fn repr_options(self, def: DefId) -> vsc_abi::ReprOptions {
        self.caches.repr_options.get(
            def,
            || (self.providers.repr_options)(self, def),
            vsc_abi::ReprOptions::default,
        )
    }

    pub fn adt_destructor(self, def: DefId) -> Option<DefId> {
        self.caches.adt_destructor.get(def, || (self.providers.adt_destructor)(self, def), || None)
    }

    pub fn layout_of(
        self,
        ty: Ty<'tcx>,
    ) -> Result<crate::ty::TyAndLayout<'tcx>, crate::ty::layout::LayoutError<'tcx>> {
        self.caches.layout_of.get(
            ty,
            || (self.providers.layout_of)(self, ty),
            || Err(crate::ty::layout::LayoutError::Unknown(ty)),
        )
    }

    pub fn fn_abi_of_fn_ptr(
        self,
        key: (PolyFnSig<'tcx>, &'tcx [Ty<'tcx>]),
    ) -> Result<
        &'tcx crate::ty::layout::FnAbi<'tcx, Ty<'tcx>>,
        crate::ty::layout::FnAbiError<'tcx>,
    > {
        self.caches.fn_abi_of_fn_ptr.get(
            key,
            || (self.providers.fn_abi_of_fn_ptr)(self, key),
            || Err(crate::ty::layout::FnAbiError::Layout(crate::ty::layout::LayoutError::Unknown(
                self.types.error,
            ))),
        )
    }

    pub fn fn_abi_of_instance(
        self,
        key: (crate::ty::Instance<'tcx>, &'tcx [Ty<'tcx>]),
    ) -> Result<
        &'tcx crate::ty::layout::FnAbi<'tcx, Ty<'tcx>>,
        crate::ty::layout::FnAbiError<'tcx>,
    > {
        self.caches.fn_abi_of_instance.get(
            key,
            || (self.providers.fn_abi_of_instance)(self, key),
            || Err(crate::ty::layout::FnAbiError::Layout(crate::ty::layout::LayoutError::Unknown(
                self.types.error,
            ))),
        )
    }

    pub fn resolve_instance_raw(
        self,
        key: (DefId, GenericArgsRef<'tcx>),
    ) -> Option<crate::ty::Instance<'tcx>> {
        self.caches.resolve_instance_raw.get(
            key,
            || (self.providers.resolve_instance_raw)(self, key),
            || None,
        )
    }

    pub fn needs_drop_raw(self, ty: Ty<'tcx>) -> bool {
        self.caches.needs_drop_raw.get(ty, || (self.providers.needs_drop_raw)(self, ty), || false)
    }

    pub fn codegen_fn_attrs(
        self,
        def: DefId,
    ) -> &'tcx crate::middle::codegen_fn_attrs::CodegenFnAttrs {
        self.caches.codegen_fn_attrs.get(
            def,
            || self.gcx.arena.codegen_fn_attrs.alloc((self.providers.codegen_fn_attrs)(self, def)),
            || self.gcx.arena.codegen_fn_attrs.alloc(Default::default()),
        )
    }

    pub fn symbol_name(self, instance: crate::ty::Instance<'tcx>) -> crate::ty::SymbolName<'tcx> {
        self.caches.symbol_name.get(
            instance,
            || (self.providers.symbol_name)(self, instance),
            || crate::ty::SymbolName::new(self, "cycle_symbol"),
        )
    }

    pub fn entry_fn(self, _: ()) -> Option<(DefId, crate::ty::EntryFnType)> {
        self.caches.entry_fn.get((), || (self.providers.entry_fn)(self, ()), || None)
    }

    pub fn collect_and_partition_mono_items(
        self,
        _: (),
    ) -> &'tcx [crate::mono::CodegenUnit<'tcx>] {
        self.caches.collect_and_partition_mono_items.get(
            (),
            || (self.providers.collect_and_partition_mono_items)(self, ()),
            || &[],
        )
    }

    pub fn alloc_layout(
        self,
        data: vsc_abi::LayoutData<vsc_abi::FieldIdx, vsc_abi::VariantIdx>,
    ) -> vsc_abi::Layout<'tcx> {
        vsc_abi::Layout(self.gcx.arena.layouts.alloc(data))
    }

    pub fn alloc_fn_abi(
        self,
        abi: crate::ty::layout::FnAbi<'tcx, Ty<'tcx>>,
    ) -> &'tcx crate::ty::layout::FnAbi<'tcx, Ty<'tcx>> {
        self.gcx.arena.fn_abis.alloc(abi)
    }

    pub fn alloc_codegen_units(
        self,
        units: Vec<crate::mono::CodegenUnit<'tcx>>,
    ) -> &'tcx [crate::mono::CodegenUnit<'tcx>] {
        self.gcx.arena.codegen_units.alloc(units).as_slice()
    }

    pub fn alloc_str(self, s: &str) -> &'tcx str {
        self.gcx.arena.strings.alloc(s.to_owned()).as_str()
    }

    pub fn alloc_bytes(self, bytes: Vec<u8>) -> &'tcx [u8] {
        self.gcx.arena.bytes.alloc(bytes).as_slice()
    }

    /// The body a function literal or inline constant is type checked with.
    pub fn typeck_root_def_id(self, mut def: DefId) -> DefId {
        while matches!(self.def_kind(def), DefKind::Closure | DefKind::Coroutine | DefKind::InlineConst) {
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
