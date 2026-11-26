//! AST items, visibility/safety/mutability qualifiers, functions, structs, enums, traits, and impls.

use vsc_span::{Ident, Span, Symbol};

use super::{
    AnonConst, AttrVec, Block, Expr, GenericBounds, Generics, InlineAsm, P, Pat, PatKind,
    TraitRef, Ty, TyKind,
};
use crate::node_id::NodeId;

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mutability {
    Not,
    Mut,
}

impl Mutability {
    pub fn is_mut(self) -> bool {
        self == Mutability::Mut
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Safety {
    Unsafe(Span),
    /// `safe`, only on items in an `extern` block.
    Safe(Span),
    Default,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Const {
    Yes(Span),
    No,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extern {
    None,
    /// `extern`
    Implicit(Span),
    /// `extern "C"`
    Explicit(StrLit, Span),
}

/// A string literal used as an ABI name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StrLit {
    pub symbol: Symbol,
    pub span: Span,
}

/// Exception 6: `public` is Rust's `pub`, and `package` is `pub(crate)`.
#[derive(Clone, Debug)]
pub struct Visibility {
    pub kind: VisibilityKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisibilityKind {
    /// `public`
    Public,
    /// `package`: visible in the whole crate.
    Package,
    /// Nothing written: visible in this package.
    Inherited,
}

impl Visibility {
    pub fn inherited(span: Span) -> Visibility {
        Visibility { kind: VisibilityKind::Inherited, span }
    }
}

#[derive(Clone, Debug)]
pub struct Item<K = ItemKind> {
    pub attrs: AttrVec,
    pub id: NodeId,
    pub span: Span,
    pub vis: Visibility,
    pub kind: K,
}

pub type AssocItem = Item<AssocItemKind>;
pub type ForeignItem = Item<ForeignItemKind>;

#[derive(Clone, Debug)]
pub enum ItemKind {
    Fn(P<Fn>),
    TyAlias(P<TyAlias>),
    Struct(Ident, Generics, VariantData),
    /// `class Name { … }`: Rust's `Rc<RefCell<…>>` (exception 3).
    Class(Ident, Generics, VariantData),
    Enum(Ident, Generics, EnumDef),
    Union(Ident, Generics, VariantData),
    Const(P<ConstItem>),
    Static(P<StaticItem>),
    Trait(P<Trait>),
    /// `implement` blocks, and flat receivers, which are one-method blocks
    /// (exception 8).
    Impl(P<Impl>),
    /// `unsafe extern "C" { … }`
    ForeignMod(ForeignMod),
    /// `globalAsm(…)`
    GlobalAsm(P<InlineAsm>),
}

impl ItemKind {
    pub fn ident(&self) -> Option<Ident> {
        match self {
            ItemKind::Fn(f) => Some(f.ident),
            ItemKind::TyAlias(t) => Some(t.ident),
            ItemKind::Struct(i, ..) | ItemKind::Class(i, ..) | ItemKind::Enum(i, ..) | ItemKind::Union(i, ..) => {
                Some(*i)
            }
            ItemKind::Const(c) => Some(c.ident),
            ItemKind::Static(s) => Some(s.ident),
            ItemKind::Trait(t) => Some(t.ident),
            ItemKind::Impl(_) | ItemKind::ForeignMod(_) | ItemKind::GlobalAsm(_) => None,
        }
    }

    pub fn descr(&self) -> &'static str {
        match self {
            ItemKind::Fn(_) => "function",
            ItemKind::TyAlias(_) => "type alias",
            ItemKind::Struct(..) => "struct",
            ItemKind::Class(..) => "class",
            ItemKind::Enum(..) => "enum",
            ItemKind::Union(..) => "union",
            ItemKind::Const(_) => "constant",
            ItemKind::Static(_) => "static",
            ItemKind::Trait(_) => "trait",
            ItemKind::Impl(_) => "implementation",
            ItemKind::ForeignMod(_) => "extern block",
            ItemKind::GlobalAsm(_) => "global asm",
        }
    }
}

#[derive(Clone, Debug)]
pub enum AssocItemKind {
    Const(P<ConstItem>),
    Fn(P<Fn>),
    Type(P<TyAlias>),
}

#[derive(Clone, Debug)]
pub enum ForeignItemKind {
    Static(P<StaticItem>),
    Fn(P<Fn>),
    /// `type FILE`: an opaque foreign type (Rust's unstable extern types).
    TyAlias(P<TyAlias>),
}

#[derive(Clone, Debug)]
pub struct Fn {
    pub ident: Ident,
    pub generics: Generics,
    pub sig: FnSig,
    pub body: Option<P<Block>>,
}

#[derive(Clone, Debug)]
pub struct FnSig {
    pub header: FnHeader,
    pub decl: P<FnDecl>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug)]
pub struct FnHeader {
    pub safety: Safety,
    pub asyncness: Option<Span>,
    pub constness: Const,
    pub ext: Extern,
}

impl Default for FnHeader {
    fn default() -> Self {
        FnHeader { safety: Safety::Default, asyncness: None, constness: Const::No, ext: Extern::None }
    }
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub inputs: Vec<Param>,
    pub output: FnRetTy,
}

impl FnDecl {
    pub fn has_self(&self) -> bool {
        self.inputs.first().is_some_and(|p| p.is_self() || p.flat_receiver)
    }

    pub fn c_variadic(&self) -> bool {
        self.inputs.last().is_some_and(|p| matches!(p.ty.kind, TyKind::CVarArgs))
    }
}

#[derive(Clone, Debug)]
pub struct Param {
    pub attrs: AttrVec,
    pub ty: P<Ty>,
    pub pat: P<Pat>,
    pub id: NodeId,
    pub span: Span,
    /// The receiver of a flat method: `t` in `func (t: &Task) IsOpen()`
    /// (exception 8). It plays the part of Rust's `self`.
    pub flat_receiver: bool,
}

impl Param {
    pub fn is_self(&self) -> bool {
        matches!(&self.pat.kind, PatKind::Ident(_, ident, _) if ident.name == vsc_span::kw::SelfLower)
    }
}

#[derive(Clone, Debug)]
pub enum FnRetTy {
    /// Nothing written: `()`. The span is where the type would go.
    Default(Span),
    Ty(P<Ty>),
}

#[derive(Clone, Debug)]
pub struct TyAlias {
    pub ident: Ident,
    pub generics: Generics,
    pub bounds: GenericBounds,
    pub ty: Option<P<Ty>>,
}

#[derive(Clone, Debug)]
pub struct ConstItem {
    /// `_` for `const _: T = …`.
    pub ident: Ident,
    pub ty: P<Ty>,
    pub expr: Option<P<Expr>>,
}

#[derive(Clone, Debug)]
pub struct StaticItem {
    pub ident: Ident,
    pub ty: P<Ty>,
    pub safety: Safety,
    /// `static var` (exception 5).
    pub mutability: Mutability,
    pub expr: Option<P<Expr>>,
}

#[derive(Clone, Debug)]
pub struct Trait {
    pub safety: Safety,
    pub ident: Ident,
    pub generics: Generics,
    pub bounds: GenericBounds,
    pub items: Vec<P<AssocItem>>,
}

#[derive(Clone, Debug)]
pub struct Impl {
    pub generics: Generics,
    pub safety: Safety,
    /// `!Trait`
    pub negative: bool,
    pub of_trait: Option<TraitRef>,
    pub self_ty: P<Ty>,
    pub items: Vec<P<AssocItem>>,
    pub origin: ImplOrigin,
}

/// How an impl was written (exception 8).
#[derive(Clone, Debug)]
pub enum ImplOrigin {
    /// `implement … { … }`
    Block,
    /// `func (t: &Task) Method()`: a one-method inherent impl. The impl has no
    /// generics of its own: in Go's style, each bare name among the receiver
    /// type's arguments (`T` in `&Stack<T>`) introduces a parameter, and
    /// name resolution decides whether it's a type or a lifetime parameter
    /// from the type's own declaration.
    FlatReceiver { introduced_params: Vec<Ident> },
}

#[derive(Clone, Debug)]
pub struct ForeignMod {
    /// `unsafe`, required as in Rust 2024.
    pub safety: Safety,
    pub abi: Option<StrLit>,
    pub items: Vec<P<ForeignItem>>,
}

#[derive(Clone, Debug)]
pub struct EnumDef {
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub attrs: AttrVec,
    pub id: NodeId,
    pub span: Span,
    pub vis: Visibility,
    pub ident: Ident,
    pub data: VariantData,
    pub disr_expr: Option<AnonConst>,
}

#[derive(Clone, Debug)]
pub enum VariantData {
    /// `{ a: T; b: U }` (fields separated by line breaks, exception 10).
    Struct { fields: Vec<FieldDef> },
    /// `(T, U)`
    Tuple(Vec<FieldDef>, NodeId),
    /// No fields.
    Unit(NodeId),
}

impl VariantData {
    pub fn fields(&self) -> &[FieldDef] {
        match self {
            VariantData::Struct { fields } | VariantData::Tuple(fields, _) => fields,
            VariantData::Unit(_) => &[],
        }
    }
}

#[derive(Clone, Debug)]
pub struct FieldDef {
    pub attrs: AttrVec,
    pub id: NodeId,
    pub span: Span,
    pub vis: Visibility,
    /// `None` for tuple fields.
    pub ident: Option<Ident>,
    pub ty: P<Ty>,
}
