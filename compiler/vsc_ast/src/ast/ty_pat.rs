//! AST type and pattern nodes.

use vsc_span::{Ident, Span};

use super::{
    AnonConst, AttrVec, Expr, Extern, FnDecl, GenericBounds, GenericParam, Lifetime, Mutability, P,
    Path, QSelf, Safety,
};
use crate::node_id::NodeId;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Ty {
    pub id: NodeId,
    pub kind: TyKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TyKind {
    /// `[T]`
    Slice(P<Ty>),
    /// `[T; N]`
    Array(P<Ty>, AnonConst),
    /// `*const T`, `*mut T`
    Ptr(MutTy),
    /// `&T`, `&mut T`, `&a T` (exception 13).
    Ref(Option<Lifetime>, MutTy),
    /// `unsafe extern "C" func(T) -> U`: a raw function pointer.
    FnPtr(P<FnPtrTy>),
    /// `func(T) -> U`: a function value, Rust's `Rc<dyn Fn(T) -> U>`
    /// (exception 14).
    FuncValue(P<FuncValueTy>),
    /// `never`, Rust's `!` (exception 2).
    Never,
    /// `(A, B)`, and `()`.
    Tup(Vec<P<Ty>>),
    /// `a.B<T>`, `<T as Trait>.Assoc`.
    Path(Option<P<QSelf>>, Path),
    /// `dyn A + B`
    TraitObject(GenericBounds),
    /// `impl A + B`
    ImplTrait(NodeId, GenericBounds),
    /// `(T)`
    Paren(P<Ty>),
    /// `_`
    Infer,
    /// The type of a `self` parameter written as `self`, `&self` or `var self`.
    ImplicitSelf,
    /// `...` as the last parameter of a C variadic function.
    CVarArgs,
    /// A type the parser already reported an error for.
    Err,
}

impl Ty {
    pub fn is_unit(&self) -> bool {
        matches!(&self.kind, TyKind::Tup(v) if v.is_empty())
    }
}

#[derive(Clone, Debug)]
pub struct MutTy {
    pub ty: P<Ty>,
    pub mutbl: Mutability,
}

#[derive(Clone, Debug)]
pub struct FnPtrTy {
    pub safety: Safety,
    pub ext: Extern,
    pub generic_params: Vec<GenericParam>,
    pub decl: P<FnDecl>,
    pub decl_span: Span,
}

#[derive(Clone, Debug)]
pub struct FuncValueTy {
    pub generic_params: Vec<GenericParam>,
    pub decl: P<FnDecl>,
    pub decl_span: Span,
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Pat {
    pub id: NodeId,
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum PatKind {
    /// `_`
    Wild,
    /// `x`, `ref x`, `ref mut x`, `var x` (exception 5), `x @ pat`.
    Ident(BindingMode, Ident, Option<P<Pat>>),
    /// `Path { a, b: pat, .. }`
    Struct(Option<P<QSelf>>, Path, Vec<PatField>, PatFieldsRest),
    /// `Path(a, b)`
    TupleStruct(Option<P<QSelf>>, Path, Vec<P<Pat>>),
    /// `A | B`
    Or(Vec<P<Pat>>),
    /// `Status.Done`, `<T>.Assoc`
    Path(Option<P<QSelf>>, Path),
    /// `(a, b)`
    Tuple(Vec<P<Pat>>),
    /// `&pat`, `&mut pat`
    Ref(P<Pat>, Mutability),
    /// `1`, `-1`, `"s"`, `true`
    Expr(P<Expr>),
    /// `a..b`, `a..=b`, `a..`, `..=b`
    Range(Option<P<Expr>>, Option<P<Expr>>, RangeEnd),
    /// `[a, .., b]`
    Slice(Vec<P<Pat>>),
    /// `..`
    Rest,
    /// `(pat)`
    Paren(P<Pat>),
    /// A pattern the parser already reported an error for.
    Err,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindingMode(pub ByRef, pub Mutability);

impl BindingMode {
    pub const NONE: BindingMode = BindingMode(ByRef::No, Mutability::Not);
    pub const VAR: BindingMode = BindingMode(ByRef::No, Mutability::Mut);
    pub const REF: BindingMode = BindingMode(ByRef::Yes(Mutability::Not), Mutability::Not);
    pub const REF_MUT: BindingMode = BindingMode(ByRef::Yes(Mutability::Mut), Mutability::Not);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByRef {
    Yes(Mutability),
    No,
}

#[derive(Clone, Debug)]
pub struct PatField {
    pub ident: Ident,
    pub pat: P<Pat>,
    /// Written as just the name: `{ x }`.
    pub is_shorthand: bool,
    pub attrs: AttrVec,
    pub id: NodeId,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatFieldsRest {
    /// `..`
    Rest,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeEnd {
    /// `..=`
    Included,
    /// `..`
    Excluded,
}

