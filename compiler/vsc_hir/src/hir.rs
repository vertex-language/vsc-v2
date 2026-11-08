//! HIR nodes (rustc's `rustc_hir::hir`).
//!
//! Each item, associated item and foreign item is an *owner*: its nodes are
//! numbered from 0 within it, and its bodies (function bodies, constant
//! values) are stored with it.

use vsc_ast::token::Lit;
pub use vsc_ast::{BorrowKind, ByRef, CaptureBy, Mutability, RangeEnd, Safety, UnOp};
use vsc_data_structures::FxHashMap;
use vsc_index::IndexVec;
use vsc_span::def_id::LocalDefId;
use vsc_span::{Ident, Span, Spanned, Symbol};

use crate::LangItem;
use crate::def::{LifetimeRes, Res};
use crate::hir_id::{HirId, ItemLocalId};

pub type BinOp = Spanned<vsc_ast::BinOpKind>;

// ---------------------------------------------------------------------------
// The crate and its owners
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Crate {
    /// Every owner's nodes, by definition. Definitions that aren't owners
    /// (fields, parameters, locals' closures) are `None`.
    pub owners: IndexVec<LocalDefId, Option<OwnerInfo>>,
}

impl Crate {
    pub fn owner(&self, def: LocalDefId) -> &OwnerInfo {
        self.owners[def].as_ref().unwrap_or_else(|| panic!("{def:?} isn't a HIR owner"))
    }

    pub fn item(&self, id: ItemId) -> &Item {
        match &self.owner(id.owner_id).node {
            OwnerNode::Item(i) => i,
            _ => panic!("{:?} isn't an item", id.owner_id),
        }
    }

    pub fn body(&self, id: BodyId) -> &Body {
        &self.owner(id.hir_id.owner).bodies[&id.hir_id.local_id]
    }

    pub fn assoc_item(&self, def: LocalDefId) -> &AssocItem {
        match &self.owner(def).node {
            OwnerNode::AssocItem(i) => i,
            _ => panic!("{def:?} isn't an associated item"),
        }
    }

    pub fn foreign_item(&self, def: LocalDefId) -> &ForeignItem {
        match &self.owner(def).node {
            OwnerNode::ForeignItem(i) => i,
            _ => panic!("{def:?} isn't a foreign item"),
        }
    }
}

#[derive(Debug)]
pub struct OwnerInfo {
    pub node: OwnerNode,
    pub bodies: FxHashMap<ItemLocalId, Body>,
    /// Attributes, by the node they're on (still in their AST form).
    pub attrs: FxHashMap<ItemLocalId, Vec<vsc_ast::Attribute>>,
}

#[derive(Debug)]
pub enum OwnerNode {
    /// The crate root's package.
    Crate(Mod),
    Item(Item),
    AssocItem(AssocItem),
    ForeignItem(ForeignItem),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ItemId {
    pub owner_id: LocalDefId,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BodyId {
    pub hir_id: HirId,
}

/// A function's parameters and value, or a constant's value.
#[derive(Debug)]
pub struct Body {
    pub params: Vec<Param>,
    pub value: Expr,
}

#[derive(Debug)]
pub struct Param {
    pub hir_id: HirId,
    pub pat: Pat,
    pub ty_span: Span,
    pub span: Span,
}

/// A package: its items, from all its files. Child packages are items too.
#[derive(Debug)]
pub struct Mod {
    pub item_ids: Vec<ItemId>,
    pub spans: Span,
}

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Item {
    pub owner_id: LocalDefId,
    /// Empty for impls, extern blocks and global asm.
    pub ident: Ident,
    pub kind: ItemKind,
    pub span: Span,
    pub vis_span: Span,
}

impl Item {
    pub fn hir_id(&self) -> HirId {
        HirId::make_owner(self.owner_id)
    }
}

#[derive(Debug)]
pub enum ItemKind {
    /// A child package.
    Mod(Mod),
    Fn { sig: FnSig, generics: Generics, body: BodyId },
    Static(Ty, Mutability, BodyId),
    Const(Ty, Generics, BodyId),
    TyAlias(Ty, Generics),
    Enum(EnumDef, Generics),
    Struct(VariantData, Generics),
    /// `class` (exception 3): a struct whose values are shared and
    /// reference-counted. Type checking gives it `Rc<RefCell<…>>` semantics.
    Class(VariantData, Generics),
    Union(VariantData, Generics),
    Trait { safety: Safety, generics: Generics, bounds: GenericBounds, items: Vec<LocalDefId> },
    Impl(Impl),
    ForeignMod { abi: Option<Symbol>, items: Vec<LocalDefId> },
    GlobalAsm(InlineAsm),
}

impl ItemKind {
    pub fn descr(&self) -> &'static str {
        match self {
            ItemKind::Mod(_) => "package",
            ItemKind::Fn { .. } => "function",
            ItemKind::Static(..) => "static",
            ItemKind::Const(..) => "constant",
            ItemKind::TyAlias(..) => "type alias",
            ItemKind::Enum(..) => "enum",
            ItemKind::Struct(..) => "struct",
            ItemKind::Class(..) => "class",
            ItemKind::Union(..) => "union",
            ItemKind::Trait { .. } => "trait",
            ItemKind::Impl(_) => "implementation",
            ItemKind::ForeignMod { .. } => "extern block",
            ItemKind::GlobalAsm(_) => "global asm",
        }
    }
}

#[derive(Debug)]
pub struct Impl {
    pub safety: Safety,
    pub negative: bool,
    pub generics: Generics,
    pub of_trait: Option<TraitRef>,
    pub self_ty: Ty,
    pub items: Vec<LocalDefId>,
    /// Written as a flat receiver (exception 8).
    pub flat_receiver: bool,
}

#[derive(Debug)]
pub struct AssocItem {
    pub owner_id: LocalDefId,
    pub ident: Ident,
    pub generics: Generics,
    pub kind: AssocItemKind,
    pub span: Span,
    pub vis_span: Span,
}

#[derive(Debug)]
pub enum AssocItemKind {
    Const(Ty, Option<BodyId>),
    /// A method or associated function. In a trait, `body` is `None` when
    /// there's no default.
    Fn(FnSig, Option<BodyId>),
    Type(GenericBounds, Option<Ty>),
}

#[derive(Debug)]
pub struct ForeignItem {
    pub owner_id: LocalDefId,
    pub ident: Ident,
    pub kind: ForeignItemKind,
    pub span: Span,
    pub vis_span: Span,
}

#[derive(Debug)]
pub enum ForeignItemKind {
    Fn(FnSig, Vec<Ident>, Generics),
    Static(Ty, Mutability, Safety),
    Type,
}

#[derive(Debug)]
pub struct FnSig {
    pub header: FnHeader,
    pub decl: FnDecl,
    pub span: Span,
}

#[derive(Clone, Copy, Debug)]
pub struct FnHeader {
    pub safety: Safety,
    pub is_async: bool,
    pub is_const: bool,
    /// The ABI, `None` for Rust's.
    pub abi: Option<Symbol>,
}

#[derive(Debug)]
pub struct FnDecl {
    pub inputs: Vec<Ty>,
    pub output: FnRetTy,
    pub c_variadic: bool,
    pub implicit_self: ImplicitSelfKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImplicitSelfKind {
    /// `self`
    Imm,
    /// `var self`
    Mut,
    /// `&self`
    RefImm,
    /// `&mut self`
    RefMut,
    /// A flat receiver, or `self: T` (exception 8).
    Explicit,
    None,
}

#[derive(Debug)]
pub enum FnRetTy {
    DefaultReturn(Span),
    Return(Ty),
}

#[derive(Debug)]
pub struct EnumDef {
    pub variants: Vec<Variant>,
}

#[derive(Debug)]
pub struct Variant {
    pub ident: Ident,
    pub hir_id: HirId,
    pub def_id: LocalDefId,
    pub data: VariantData,
    pub disr_expr: Option<AnonConst>,
    pub span: Span,
}

#[derive(Debug)]
pub enum VariantData {
    Struct { fields: Vec<FieldDef> },
    Tuple(Vec<FieldDef>, HirId, LocalDefId),
    Unit(HirId, LocalDefId),
}

impl VariantData {
    pub fn fields(&self) -> &[FieldDef] {
        match self {
            VariantData::Struct { fields } | VariantData::Tuple(fields, ..) => fields,
            VariantData::Unit(..) => &[],
        }
    }

    /// The constructor's definition, for tuple and unit forms.
    pub fn ctor_def_id(&self) -> Option<LocalDefId> {
        match self {
            VariantData::Tuple(_, _, d) | VariantData::Unit(_, d) => Some(*d),
            VariantData::Struct { .. } => None,
        }
    }
}

#[derive(Debug)]
pub struct FieldDef {
    pub span: Span,
    pub vis_span: Span,
    pub ident: Ident,
    pub hir_id: HirId,
    pub def_id: LocalDefId,
    pub ty: Ty,
}

#[derive(Debug)]
pub struct AnonConst {
    pub hir_id: HirId,
    pub def_id: LocalDefId,
    pub body: BodyId,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Generics and bounds
// ---------------------------------------------------------------------------

#[derive(Default, Debug)]
pub struct Generics {
    pub params: Vec<GenericParam>,
    pub predicates: Vec<WherePredicate>,
    pub span: Span,
}

#[derive(Debug)]
pub struct GenericParam {
    pub hir_id: HirId,
    pub def_id: LocalDefId,
    pub name: Ident,
    pub kind: GenericParamKind,
    pub span: Span,
    /// Introduced by naming it in a flat receiver's type (exception 8).
    pub from_flat_receiver: bool,
}

#[derive(Debug)]
pub enum GenericParamKind {
    /// A lifetime parameter: `src` in `<src>` (exception 13).
    Lifetime,
    Type { default: Option<Ty> },
    Const { ty: Ty, default: Option<AnonConst> },
}

#[derive(Debug)]
pub enum WherePredicate {
    /// `T: Bound`, including bounds written on the parameter itself.
    Bound { hir_id: HirId, span: Span, bound_generic_params: Vec<GenericParam>, bounded_ty: Ty, bounds: GenericBounds },
    /// `a: [b]`: a lifetime outliving others (Rust's `'a: 'b`).
    Lifetime { span: Span, lifetime: Lifetime, bounds: Vec<Lifetime> },
}

pub type GenericBounds = Vec<GenericBound>;

#[derive(Debug)]
pub enum GenericBound {
    Trait(PolyTraitRef),
    Outlives(Lifetime),
    Use(Vec<Ident>, Span),
}

#[derive(Debug)]
pub struct PolyTraitRef {
    pub bound_generic_params: Vec<GenericParam>,
    pub maybe: bool,
    pub trait_ref: TraitRef,
    pub span: Span,
}

#[derive(Debug)]
pub struct TraitRef {
    pub path: Path,
    pub hir_ref_id: HirId,
}

/// A lifetime, Rust's without the `'` (exception 13).
#[derive(Clone, Debug)]
pub struct Lifetime {
    pub hir_id: HirId,
    pub span: Span,
    pub kind: LifetimeKind,
}

#[derive(Clone, Debug)]
pub enum LifetimeKind {
    /// Nothing written: elided, as in Rust.
    Elided,
    /// `a`, `static`.
    Named(Ident, LifetimeRes),
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Ty {
    pub hir_id: HirId,
    pub kind: TyKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum TyKind {
    Slice(Box<Ty>),
    Array(Box<Ty>, AnonConst),
    Ptr(MutTy),
    Ref(Lifetime, MutTy),
    /// `unsafe extern "C" func(…)`
    FnPtr(Box<FnPtrTy>),
    /// `func(T) -> U` (exception 14), Rust's `Rc<dyn Fn(T) -> U>`.
    FuncValue(Box<FuncValueTy>),
    Never,
    Tup(Vec<Ty>),
    Path(QPath),
    TraitObject(Vec<PolyTraitRef>, Option<Lifetime>),
    /// `impl Trait`
    OpaqueDef(GenericBounds),
    Infer,
    Err,
}

#[derive(Debug)]
pub struct MutTy {
    pub ty: Box<Ty>,
    pub mutbl: Mutability,
}

#[derive(Debug)]
pub struct FnPtrTy {
    pub safety: Safety,
    pub abi: Option<Symbol>,
    pub generic_params: Vec<GenericParam>,
    pub decl: FnDecl,
    pub param_idents: Vec<Option<Ident>>,
}

#[derive(Debug)]
pub struct FuncValueTy {
    pub generic_params: Vec<GenericParam>,
    pub decl: FnDecl,
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// What HIR paths resolve to: locals are named by their binding's `HirId`.
pub type HirRes = Res<HirId>;

#[derive(Debug)]
pub struct Path {
    pub span: Span,
    pub res: HirRes,
    pub segments: Vec<PathSegment>,
}

#[derive(Debug)]
pub struct PathSegment {
    pub ident: Ident,
    pub hir_id: HirId,
    pub res: HirRes,
    pub args: Option<Box<GenericArgs>>,
}

#[derive(Debug)]
pub struct GenericArgs {
    pub args: Vec<GenericArg>,
    pub constraints: Vec<AssocItemConstraint>,
    /// `Fn(A, B) -> C`
    pub parenthesized: bool,
    pub span: Span,
}

#[derive(Debug)]
pub enum GenericArg {
    Lifetime(Lifetime),
    Type(Ty),
    Const(ConstArg),
    Infer(Span),
}

/// A const generic argument (rustc's `ConstArg`).
#[derive(Debug)]
pub struct ConstArg {
    pub hir_id: HirId,
    pub kind: ConstArgKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum ConstArgKind {
    /// A const parameter or constant, named: `N` in `Stack<T, N>`. The
    /// parser reads it as a type; name resolution found a value.
    Path(QPath),
    /// `{ expr }`, or a literal.
    Anon(AnonConst),
}

#[derive(Debug)]
pub struct AssocItemConstraint {
    pub hir_id: HirId,
    pub ident: Ident,
    pub kind: AssocItemConstraintKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum AssocItemConstraintKind {
    Equality(Ty),
    Bound(GenericBounds),
}

#[derive(Debug)]
pub enum QPath {
    /// A fully resolved path, with an optional qualified self type.
    Resolved(Option<Box<Ty>>, Path),
    /// `Type.Name`: an associated item of a type, found by type checking.
    TypeRelative(Box<Ty>, PathSegment),
    /// A library item the compiler names by role.
    LangItem(LangItem, Span),
}

impl QPath {
    pub fn span(&self) -> Span {
        match self {
            QPath::Resolved(_, p) => p.span,
            QPath::TypeRelative(t, seg) => t.span.to(seg.ident.span),
            QPath::LangItem(_, span) => *span,
        }
    }
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Pat {
    pub hir_id: HirId,
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BindingMode(pub ByRef, pub Mutability);

#[derive(Debug)]
pub enum PatKind {
    Wild,
    /// A new binding. The `HirId` is the binding's own.
    Binding(BindingMode, HirId, Ident, Option<Box<Pat>>),
    Struct(QPath, Vec<PatField>, bool),
    /// `Path(a, .., b)`: the `Option` is where `..` is.
    TupleStruct(QPath, Vec<Pat>, Option<usize>),
    Or(Vec<Pat>),
    Path(QPath),
    Tuple(Vec<Pat>, Option<usize>),
    Ref(Box<Pat>, Mutability),
    Expr(Box<Expr>),
    Range(Option<Box<Expr>>, Option<Box<Expr>>, RangeEnd),
    /// `[a, rest @ .., z]`: before, the rest, after.
    Slice(Vec<Pat>, Option<Box<Pat>>, Vec<Pat>),
    Err,
}

#[derive(Debug)]
pub struct PatField {
    pub hir_id: HirId,
    pub ident: Ident,
    pub pat: Pat,
    pub is_shorthand: bool,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Expr {
    pub hir_id: HirId,
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum ExprKind {
    ConstBlock(ConstBlock),
    Array(Vec<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    MethodCall(Box<PathSegment>, Box<Expr>, Vec<Expr>, Span),
    Tup(Vec<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Lit(Lit),
    Cast(Box<Expr>, Ty),
    /// Drops its operand's temporaries at once (for desugarings).
    DropTemps(Box<Expr>),
    /// `let pat = expr`, in `if` conditions and guards.
    Let(Box<LetExpr>),
    If(Box<Expr>, Box<Expr>, Option<Box<Expr>>),
    Loop(Box<Block>, Option<Label>, LoopSource, Span),
    Match(Box<Expr>, Vec<Arm>, MatchSource),
    Closure(Box<Closure>),
    Block(Box<Block>, Option<Label>),
    Assign(Box<Expr>, Box<Expr>, Span),
    AssignOp(BinOp, Box<Expr>, Box<Expr>),
    Field(Box<Expr>, Ident),
    Index(Box<Expr>, Box<Expr>, Span),
    Path(QPath),
    AddrOf(BorrowKind, Mutability, Box<Expr>),
    Break(Destination, Option<Box<Expr>>),
    Continue(Destination),
    Ret(Option<Box<Expr>>),
    InlineAsm(Box<InlineAsm>),
    Struct(QPath, Vec<ExprField>, StructTailExpr),
    Repeat(Box<Expr>, AnonConst),
    /// `x.await`. Desugared to polling once async is implemented.
    Await(Box<Expr>),
    Err,
}

#[derive(Debug)]
pub struct ConstBlock {
    pub hir_id: HirId,
    pub def_id: LocalDefId,
    pub body: BodyId,
}

#[derive(Debug)]
pub struct LetExpr {
    pub span: Span,
    pub pat: Pat,
    pub ty: Option<Ty>,
    pub init: Expr,
}

#[derive(Debug)]
pub struct Arm {
    pub hir_id: HirId,
    pub span: Span,
    pub pat: Pat,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoopSource {
    Loop,
    While,
    ForLoop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchSource {
    Normal,
    /// The outer `match IntoIterator::into_iter(…)` of a `for` loop.
    ForLoopDesugar,
    /// `?`
    TryDesugar,
    /// The arguments of a format string, evaluated once.
    FormatArgs,
}

#[derive(Clone, Copy, Debug)]
pub struct Label {
    pub ident: Ident,
}

/// What `break` and `continue` leave: the loop or block, or an error.
#[derive(Clone, Copy, Debug)]
pub struct Destination {
    pub label: Option<Label>,
    /// `None` when it isn't known yet (an unlabeled `break` is checked in a
    /// later pass) or resolution failed.
    pub target_id: Option<HirId>,
}

#[derive(Debug)]
pub struct Closure {
    pub def_id: LocalDefId,
    pub capture_clause: CaptureBy,
    pub fn_decl: FnDecl,
    pub body: BodyId,
    pub fn_decl_span: Span,
    pub kind: ClosureKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosureKind {
    /// `func(…) { … }`
    Closure,
    /// `async func(…) { … }`
    AsyncClosure,
    /// `async { … }`
    AsyncBlock,
}

#[derive(Debug)]
pub struct ExprField {
    pub hir_id: HirId,
    pub ident: Ident,
    pub expr: Expr,
    pub span: Span,
    pub is_shorthand: bool,
}

#[derive(Debug)]
pub enum StructTailExpr {
    None,
    Base(Box<Expr>),
}

#[derive(Debug)]
pub struct InlineAsm {
    pub asm_macro: vsc_ast::AsmMacro,
    pub templates: Vec<(Lit, Span)>,
    pub operands: Vec<(InlineAsmOperand, Span)>,
    pub clobber_abis: Vec<(Symbol, Span)>,
    pub options: Vec<(Symbol, Span)>,
    pub span: Span,
}

#[derive(Debug)]
pub enum InlineAsmOperand {
    In { reg: Symbol, explicit: bool, expr: Expr },
    Out { reg: Symbol, explicit: bool, late: bool, expr: Option<Expr> },
    InOut { reg: Symbol, explicit: bool, late: bool, expr: Expr },
    SplitInOut { reg: Symbol, explicit: bool, late: bool, in_expr: Expr, out_expr: Option<Expr> },
    Const { anon_const: AnonConst },
    SymFn { expr: Expr },
    Label { block: Block },
}

// ---------------------------------------------------------------------------
// Statements and blocks
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Block {
    pub hir_id: HirId,
    pub stmts: Vec<Stmt>,
    /// The block's value (exception 10: its last expression statement). When
    /// the block's type is `()`, type checking discards this value.
    pub expr: Option<Box<Expr>>,
    pub rules: BlockCheckMode,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockCheckMode {
    Default,
    /// `unsafe { … }`
    Unsafe,
    /// A block the compiler made that may do unsafe things.
    CompilerGenerated,
}

#[derive(Debug)]
pub struct Stmt {
    pub hir_id: HirId,
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug)]
pub enum StmtKind {
    Let(Box<LetStmt>),
    Item(ItemId),
    /// An expression whose value is discarded.
    Semi(Expr),
}

#[derive(Debug)]
pub struct LetStmt {
    pub hir_id: HirId,
    pub pat: Pat,
    pub ty: Option<Ty>,
    pub init: Option<Expr>,
    pub els: Option<Box<Block>>,
    pub span: Span,
}
