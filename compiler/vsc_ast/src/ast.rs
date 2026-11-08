//! AST nodes.
//!
//! Names follow rustc's `rustc_ast::ast`. Where Vertex adds something, the doc
//! comment names the exception it comes from.

use vsc_span::{Ident, Span, Symbol};

use crate::node_id::NodeId;
use crate::token::{self, CommentKind, Delimiter};
use crate::tokenstream::{DelimSpan, TokenTree};

pub type P<T> = Box<T>;
pub type AttrVec = Vec<Attribute>;

// ---------------------------------------------------------------------------
// Crates, packages and source files (exception 1)
// ---------------------------------------------------------------------------

/// A crate: the package tree under the folder of its root file.
#[derive(Clone, Debug)]
pub struct Crate {
    pub root: Package,
    pub id: NodeId,
    pub span: Span,
}

/// A package: every `.vs` file in one folder (Rust's `mod`). Its subfolders
/// with Vertex files in them are its child packages.
#[derive(Clone, Debug)]
pub struct Package {
    /// The folder's name; for the crate root, the crate's name.
    pub name: Ident,
    pub id: NodeId,
    pub files: Vec<SourceFile>,
    pub children: Vec<Package>,
    pub dir: std::path::PathBuf,
    pub span: Span,
}

impl Package {
    /// Every item in the package, from all its files.
    pub fn items(&self) -> impl Iterator<Item = &P<Item>> {
        self.files.iter().flat_map(|f| f.items.iter())
    }
}

/// One `.vs` file: an optional `package` declaration, imports, then items.
/// A folder's files together make one package (a Rust `mod`).
#[derive(Clone, Debug)]
pub struct SourceFile {
    /// `//!` doc comments at the top of the file.
    pub inner_attrs: AttrVec,
    pub package: Option<PackageDecl>,
    pub imports: Vec<Import>,
    pub items: Vec<P<Item>>,
    pub span: Span,
}

/// `package main`. Attributes on it apply to the whole package, which is
/// where Rust's inner attributes (`#![…]`) go.
#[derive(Clone, Debug)]
pub struct PackageDecl {
    pub attrs: AttrVec,
    pub name: Ident,
    pub span: Span,
}

/// One imported item in `import { HashMap, Error as IoError } from "std/io"`.
#[derive(Clone, Debug)]
pub struct ImportItem {
    pub ident: Ident,
    pub rename: Option<Ident>,
    pub span: Span,
}

/// `import "std/fs"`, `import geo from "./geometry"`,
/// `import { HashMap } from "std/collections"`, or
/// `import http, { Error as HttpError } from "net/http"`.
#[derive(Clone, Debug)]
pub struct Import {
    /// The name the package is bound to, if given.
    pub rename: Option<Ident>,
    /// Named items imported from the package (`{ … }`), if given.
    pub items: Option<Vec<ImportItem>>,
    /// The import path, without quotes.
    pub path: Symbol,
    pub path_span: Span,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Attributes (exception 9)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Attribute {
    pub kind: AttrKind,
    pub style: token::AttrStyle,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum AttrKind {
    /// `@path`, `@path(…)`, `@path = expr`, `@unsafe(path …)`.
    Normal(P<NormalAttr>),
    /// `/// text` or `/** text */`.
    DocComment(CommentKind, Symbol),
}

#[derive(Clone, Debug)]
pub struct NormalAttr {
    pub unsafety: Safety,
    pub path: Path,
    pub args: AttrArgs,
}

#[derive(Clone, Debug)]
pub enum AttrArgs {
    /// `@inline`
    Empty,
    /// `@derive(Debug)`: the tokens between the delimiters.
    Delimited(DelimArgs),
    /// `@doc = "…"`
    Eq { eq_span: Span, expr: P<Expr> },
}

#[derive(Clone, Debug)]
pub struct DelimArgs {
    pub dspan: DelimSpan,
    pub delim: Delimiter,
    pub tokens: Vec<TokenTree>,
}

impl Attribute {
    /// The attribute's name, if it's a one-segment path (`@derive` → `derive`).
    pub fn name(&self) -> Option<Symbol> {
        match &self.kind {
            AttrKind::Normal(n) if n.path.segments.len() == 1 => Some(n.path.segments[0].ident.name),
            _ => None,
        }
    }

    pub fn is_doc_comment(&self) -> bool {
        matches!(self.kind, AttrKind::DocComment(..))
    }
}

// ---------------------------------------------------------------------------
// Paths (exception 7)
// ---------------------------------------------------------------------------

/// `a.b.C`. In expressions, the trailing segments may turn out to be fields
/// or a method: the parser can't tell `pkg.Name` from `value.field`, so name
/// resolution decides, as Go does.
#[derive(Clone, Debug)]
pub struct Path {
    pub span: Span,
    pub segments: Vec<PathSegment>,
}

impl Path {
    pub fn from_ident(ident: Ident) -> Path {
        Path { span: ident.span, segments: vec![PathSegment::from_ident(ident)] }
    }

    pub fn is_ident(&self, name: Symbol) -> bool {
        self.segments.len() == 1 && self.segments[0].args.is_none() && self.segments[0].ident.name == name
    }

    /// The single identifier, if the path is one plain name.
    pub fn as_ident(&self) -> Option<Ident> {
        match &self.segments[..] {
            [seg] if seg.args.is_none() => Some(seg.ident),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PathSegment {
    pub ident: Ident,
    pub id: NodeId,
    /// `<T, U>` or `(T) -> U`. Only type paths have these.
    pub args: Option<P<GenericArgs>>,
}

impl PathSegment {
    pub fn from_ident(ident: Ident) -> PathSegment {
        PathSegment { ident, id: crate::DUMMY_NODE_ID, args: None }
    }
}

#[derive(Clone, Debug)]
pub enum GenericArgs {
    /// `<A, B, Item = C>`
    AngleBracketed(AngleBracketedArgs),
    /// `(A, B) -> C`, as in `Fn(int64) -> int64`.
    Parenthesized(ParenthesizedArgs),
}

impl GenericArgs {
    pub fn span(&self) -> Span {
        match self {
            GenericArgs::AngleBracketed(a) => a.span,
            GenericArgs::Parenthesized(p) => p.span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct AngleBracketedArgs {
    pub span: Span,
    pub args: Vec<AngleBracketedArg>,
}

#[derive(Clone, Debug)]
pub enum AngleBracketedArg {
    Arg(GenericArg),
    /// `Item = T` or `Item: Bound`.
    Constraint(AssocItemConstraint),
}

/// A generic argument. A bare name could be a type, a const or a lifetime;
/// the parser reads it as a type and name resolution fixes it up, as rustc
/// does for const arguments.
#[derive(Clone, Debug)]
pub enum GenericArg {
    /// `static`, or a name name resolution found to be a lifetime.
    Lifetime(Lifetime),
    Type(P<Ty>),
    Const(AnonConst),
}

#[derive(Clone, Debug)]
pub struct AssocItemConstraint {
    pub id: NodeId,
    pub ident: Ident,
    pub gen_args: Option<GenericArgs>,
    pub kind: AssocItemConstraintKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum AssocItemConstraintKind {
    Equality { term: P<Ty> },
    Bound { bounds: GenericBounds },
}

#[derive(Clone, Debug)]
pub struct ParenthesizedArgs {
    pub span: Span,
    pub inputs: Vec<P<Ty>>,
    pub inputs_span: Span,
    pub output: FnRetTy,
}

/// `<T as Trait>` at the start of a qualified path.
#[derive(Clone, Debug)]
pub struct QSelf {
    pub ty: P<Ty>,
    pub path_span: Span,
    /// How many of the path's segments belong to the trait (`<T as a.Trait>.X`
    /// has 2).
    pub position: usize,
}

// ---------------------------------------------------------------------------
// Lifetimes (exception 13)
// ---------------------------------------------------------------------------

/// A lifetime, Rust's without the `'` (exception 13): `a`, `static`, `_`.
#[derive(Clone, Copy, Debug)]
pub struct Lifetime {
    pub ident: Ident,
}

impl Lifetime {
    pub fn span(&self) -> Span {
        self.ident.span
    }
}

// ---------------------------------------------------------------------------
// Generics
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct Generics {
    pub params: Vec<GenericParam>,
    pub where_clause: WhereClause,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct GenericParam {
    pub id: NodeId,
    pub ident: Ident,
    pub attrs: AttrVec,
    pub bounds: GenericBounds,
    pub kind: GenericParamKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum GenericParamKind {
    /// `src: b` (Rust's `'src: 'b`). The `b` parts are bounds.
    Lifetime,
    Type { default: Option<P<Ty>> },
    Const { ty: P<Ty>, kw_span: Span, default: Option<AnonConst> },
}

pub type GenericBounds = Vec<GenericBound>;

#[derive(Clone, Debug)]
pub enum GenericBound {
    Trait(PolyTraitRef),
    /// `a`: the type outlives `a` (Rust's `T: 'a`).
    Outlives(Lifetime),
    /// `use<T, src>`
    Use(Vec<Ident>, Span),
}

impl GenericBound {
    pub fn span(&self) -> Span {
        match self {
            GenericBound::Trait(t) => t.span,
            GenericBound::Outlives(o) => o.span(),
            GenericBound::Use(_, span) => *span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PolyTraitRef {
    /// `for<a>`
    pub bound_generic_params: Vec<GenericParam>,
    pub modifiers: TraitBoundModifiers,
    pub trait_ref: TraitRef,
    pub span: Span,
    /// Written as `(Trait)`.
    pub parens: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct TraitBoundModifiers {
    /// `?Sized`
    pub maybe: bool,
}

#[derive(Clone, Debug)]
pub struct TraitRef {
    pub path: Path,
    pub ref_id: NodeId,
}

#[derive(Clone, Debug, Default)]
pub struct WhereClause {
    pub has_where_token: bool,
    pub predicates: Vec<WherePredicate>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct WherePredicate {
    pub id: NodeId,
    pub span: Span,
    pub kind: WherePredicateKind,
}

#[derive(Clone, Debug)]
pub enum WherePredicateKind {
    /// `T: Trait + a`, `for<a> &a T: Trait`.
    ///
    /// `a: [b]` (a lifetime outliving another, Rust's `'a: 'b`) parses as
    /// this too, with `a` as a type: only name resolution knows `a` is an
    /// lifetime.
    BoundPredicate { bound_generic_params: Vec<GenericParam>, bounded_ty: P<Ty>, bounds: GenericBounds },
}

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

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
    pub attrs: AttrVec,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    /// `[a, b]`
    Array(Vec<P<Expr>>),
    /// `[x; n]`
    Repeat(P<Expr>, AnonConst),
    /// `vec[…]`: Rust's `vec![…]` (exception 12). Holds an `Array` or `Repeat`.
    Vec(P<Expr>),
    /// `const { … }`
    ConstBlock(AnonConst),
    /// `f(a, b)`. A call whose callee is a path may turn out to be a method
    /// call once names are resolved (exception 7).
    Call(P<Expr>, Vec<P<Expr>>),
    /// `x.f(a)`, when the receiver isn't a plain path.
    MethodCall(P<MethodCall>),
    /// `(a, b)`
    Tup(Vec<P<Expr>>),
    Binary(BinOp, P<Expr>, P<Expr>),
    Unary(UnOp, P<Expr>),
    Lit(token::Lit),
    /// `x as T`
    Cast(P<Expr>, P<Ty>),
    /// `let pat = expr`, only in `if` and `while` conditions and guards.
    Let(P<Pat>, P<Expr>, Span),
    /// `if cond { … } else …`
    If(P<Expr>, P<Block>, Option<P<Expr>>),
    /// `'label: while cond { … }`
    While(P<Expr>, P<Block>, Option<Label>),
    /// `'label: for pat in iter { … }`
    ForLoop { pat: P<Pat>, iter: P<Expr>, body: P<Block>, label: Option<Label> },
    /// `'label: loop { … }`
    Loop(P<Block>, Option<Label>, Span),
    /// `match x { arms }`
    Match(P<Expr>, Vec<Arm>),
    /// `func(x) { … }` (exception 14).
    Closure(P<Closure>),
    /// `{ … }`, `'label: { … }`, `unsafe { … }` (see `Block::rules`).
    Block(P<Block>, Option<Label>),
    /// `async { … }`
    Async(P<Block>),
    /// `x.await`
    Await(P<Expr>, Span),
    /// `a = b`
    Assign(P<Expr>, P<Expr>, Span),
    /// `a += b`
    AssignOp(AssignOp, P<Expr>, P<Expr>),
    /// `x.field`, `x.0`, when the receiver isn't a plain path.
    Field(P<Expr>, Ident),
    /// `x[i]`
    Index(P<Expr>, P<Expr>, Span),
    /// `a..b` and friends.
    Range(Option<P<Expr>>, Option<P<Expr>>, RangeLimits),
    /// `_` on the left of an assignment.
    Underscore,
    /// `a.b.c`, `<T as Trait>.f`.
    Path(Option<P<QSelf>>, Path),
    /// `&x`, `&mut x`, `&raw const x`.
    AddrOf(BorrowKind, Mutability, P<Expr>),
    /// `break 'label value`
    Break(Option<Label>, Option<P<Expr>>),
    /// `continue 'label`
    Continue(Option<Label>),
    /// `return value`
    Ret(Option<P<Expr>>),
    /// `asm(…)`, `nakedAsm(…)` (exception 12).
    InlineAsm(P<InlineAsm>),
    /// A built-in call after expansion: `println(…)`, `format(…)`,
    /// `assert(…)` and the rest (exception 12). Before expansion these are
    /// ordinary `Call`s.
    Builtin(P<BuiltinCall>),
    /// `Point { x: 1, ..base }`
    Struct(P<StructExpr>),
    /// `(x)`
    Paren(P<Expr>),
    /// `x?`
    Try(P<Expr>),
    /// An expression the parser already reported an error for.
    Err,
}

impl Expr {
    /// Does the expression end with a block, so that it can stand as a
    /// statement without a terminator?
    pub fn is_block_like(&self) -> bool {
        matches!(
            self.kind,
            ExprKind::Block(..)
                | ExprKind::Async(..)
                | ExprKind::If(..)
                | ExprKind::While(..)
                | ExprKind::ForLoop { .. }
                | ExprKind::Loop(..)
                | ExprKind::Match(..)
                | ExprKind::ConstBlock(..)
        )
    }
}

#[derive(Clone, Debug)]
pub struct MethodCall {
    pub seg: PathSegment,
    pub receiver: P<Expr>,
    pub args: Vec<P<Expr>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StructExpr {
    pub qself: Option<P<QSelf>>,
    pub path: Path,
    pub fields: Vec<ExprField>,
    pub rest: StructRest,
}

#[derive(Clone, Debug)]
pub struct ExprField {
    pub attrs: AttrVec,
    pub id: NodeId,
    pub span: Span,
    pub ident: Ident,
    pub expr: P<Expr>,
    /// Written as just the name: `Task { id }`.
    pub is_shorthand: bool,
}

#[derive(Clone, Debug)]
pub enum StructRest {
    /// `..base`
    Base(P<Expr>),
    None,
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub attrs: AttrVec,
    pub pat: P<Pat>,
    pub guard: Option<P<Expr>>,
    pub body: P<Expr>,
    pub span: Span,
    pub id: NodeId,
}

#[derive(Clone, Copy, Debug)]
pub struct Label {
    pub ident: Ident,
}

/// A function literal (exception 14). There is no `move`: whether captures
/// are taken by value is inferred from whether the literal escapes.
#[derive(Clone, Debug)]
pub struct Closure {
    pub capture_clause: CaptureBy,
    pub asyncness: Option<Span>,
    pub fn_decl: P<FnDecl>,
    pub body: P<Block>,
    /// The `func(…) -> T` part.
    pub fn_decl_span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureBy {
    /// Captures by reference unless inference decides otherwise. Every
    /// function literal starts out like this.
    Ref,
    /// Captures by value.
    Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeLimits {
    /// `..`
    HalfOpen,
    /// `..=`
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorrowKind {
    /// `&x`
    Ref,
    /// `&raw const x`
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOpKind {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    BitXor,
    BitAnd,
    BitOr,
    Shl,
    Shr,
    Eq,
    Lt,
    Le,
    Ne,
    Ge,
    Gt,
}

impl BinOpKind {
    pub fn as_str(self) -> &'static str {
        use BinOpKind::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Rem => "%",
            And => "&&",
            Or => "||",
            BitXor => "^",
            BitAnd => "&",
            BitOr => "|",
            Shl => "<<",
            Shr => ">>",
            Eq => "==",
            Lt => "<",
            Le => "<=",
            Ne => "!=",
            Ge => ">=",
            Gt => ">",
        }
    }

    pub fn is_comparison(self) -> bool {
        matches!(self, BinOpKind::Eq | BinOpKind::Lt | BinOpKind::Le | BinOpKind::Ne | BinOpKind::Ge | BinOpKind::Gt)
    }
}

pub type BinOp = vsc_span::Spanned<BinOpKind>;
pub type AssignOp = vsc_span::Spanned<BinOpKind>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    /// `*`
    Deref,
    /// `!`
    Not,
    /// `-`
    Neg,
}

impl UnOp {
    pub fn as_str(self) -> &'static str {
        match self {
            UnOp::Deref => "*",
            UnOp::Not => "!",
            UnOp::Neg => "-",
        }
    }
}

/// An expression evaluated at compile time: array lengths, discriminants,
/// const generic arguments.
#[derive(Clone, Debug)]
pub struct AnonConst {
    pub id: NodeId,
    pub value: P<Expr>,
}

// ---------------------------------------------------------------------------
// Built-ins and format strings (exception 12)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct BuiltinCall {
    pub kind: BuiltinKind,
    /// The built-in's name, as written.
    pub name_span: Span,
}

#[derive(Clone, Debug)]
pub enum BuiltinKind {
    /// `print`, `println`, `eprint`, `eprintln`.
    Print { newline: bool, stderr: bool, args: P<FormatArgs> },
    /// `format(…)`: a `string`.
    Format(P<FormatArgs>),
    Panic(Option<P<FormatArgs>>),
    Todo(Option<P<FormatArgs>>),
    Unreachable(Option<P<FormatArgs>>),
    Assert { cond: P<Expr>, msg: Option<P<FormatArgs>> },
    AssertEq { left: P<Expr>, right: P<Expr>, msg: Option<P<FormatArgs>> },
}

impl BuiltinKind {
    pub fn name(&self) -> &'static str {
        match self {
            BuiltinKind::Print { newline: false, stderr: false, .. } => "print",
            BuiltinKind::Print { newline: true, stderr: false, .. } => "println",
            BuiltinKind::Print { newline: false, stderr: true, .. } => "eprint",
            BuiltinKind::Print { newline: true, stderr: true, .. } => "eprintln",
            BuiltinKind::Format(_) => "format",
            BuiltinKind::Panic(_) => "panic",
            BuiltinKind::Todo(_) => "todo",
            BuiltinKind::Unreachable(_) => "unreachable",
            BuiltinKind::Assert { .. } => "assert",
            BuiltinKind::AssertEq { .. } => "assertEq",
        }
    }
}

/// A checked format string and its arguments (rustc's `FormatArgs`).
#[derive(Clone, Debug)]
pub struct FormatArgs {
    pub span: Span,
    /// The string literal as written, for printing.
    pub lit: token::Lit,
    pub lit_span: Span,
    pub template: Vec<FormatArgsPiece>,
    /// Explicit arguments first (positional, then named), then the names
    /// captured from the string (`{name}`).
    pub arguments: Vec<FormatArgument>,
}

#[derive(Clone, Debug)]
pub enum FormatArgsPiece {
    Literal(Symbol),
    Placeholder(FormatPlaceholder),
}

#[derive(Clone, Debug)]
pub struct FormatPlaceholder {
    /// Index into `FormatArgs::arguments`.
    pub argument: usize,
    pub span: Span,
    pub format_trait: FormatTrait,
    pub format_options: FormatOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormatTrait {
    /// `{}`
    Display,
    /// `{:?}`
    Debug,
    /// `{:e}`
    LowerExp,
    /// `{:E}`
    UpperExp,
    /// `{:o}`
    Octal,
    /// `{:p}`
    Pointer,
    /// `{:b}`
    Binary,
    /// `{:x}`
    LowerHex,
    /// `{:X}`
    UpperHex,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormatOptions {
    pub width: Option<FormatCount>,
    pub precision: Option<FormatCount>,
    pub alignment: Option<FormatAlignment>,
    pub fill: Option<char>,
    pub sign: Option<FormatSign>,
    pub alternate: bool,
    pub zero_pad: bool,
    pub debug_hex: Option<FormatDebugHex>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatCount {
    Literal(usize),
    /// Index into `FormatArgs::arguments`.
    Argument(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatAlignment {
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatSign {
    Plus,
    Minus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatDebugHex {
    Lower,
    Upper,
}

#[derive(Clone, Debug)]
pub struct FormatArgument {
    pub kind: FormatArgumentKind,
    pub expr: P<Expr>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FormatArgumentKind {
    /// `format("{}", x)`
    Normal,
    /// `format("{n}", n = 5)`
    Named(Ident),
    /// `format("{n}")`: `n` taken from the scope.
    Captured(Ident),
}

// ---------------------------------------------------------------------------
// Inline assembly (exception 12)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsmMacro {
    /// `asm(…)`
    Asm,
    /// `nakedAsm(…)`
    NakedAsm,
    /// `globalAsm(…)`
    GlobalAsm,
}

#[derive(Clone, Debug)]
pub struct InlineAsm {
    pub asm_macro: AsmMacro,
    /// The template strings, unparsed.
    pub templates: Vec<(token::Lit, Span)>,
    pub operands: Vec<(InlineAsmOperand, Span)>,
    /// `clobberAbi("C")`
    pub clobber_abis: Vec<(Symbol, Span)>,
    /// `options(pure, nomem)`
    pub options: Vec<(Symbol, Span)>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum InlineAsmRegOrRegClass {
    /// `"eax"`
    Reg(Symbol),
    /// `reg`
    RegClass(Symbol),
}

#[derive(Clone, Debug)]
pub enum InlineAsmOperand {
    In { name: Option<Ident>, reg: InlineAsmRegOrRegClass, expr: P<Expr> },
    Out { name: Option<Ident>, reg: InlineAsmRegOrRegClass, late: bool, expr: Option<P<Expr>> },
    InOut { name: Option<Ident>, reg: InlineAsmRegOrRegClass, late: bool, expr: P<Expr> },
    SplitInOut {
        name: Option<Ident>,
        reg: InlineAsmRegOrRegClass,
        late: bool,
        in_expr: P<Expr>,
        out_expr: Option<P<Expr>>,
    },
    Const { name: Option<Ident>, anon_const: AnonConst },
    Sym { name: Option<Ident>, id: NodeId, qself: Option<P<QSelf>>, path: Path },
    Label { name: Option<Ident>, block: P<Block> },
}

// ---------------------------------------------------------------------------
// Statements and blocks (exception 10)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub id: NodeId,
    pub rules: BlockCheckMode,
    pub span: Span,
}

impl Block {
    /// The final expression, which is the block's value (Scala's rule): the
    /// last statement, when it is an expression statement.
    pub fn tail_expr(&self) -> Option<&Expr> {
        match self.stmts.last().map(|s| &s.kind) {
            Some(StmtKind::Expr(e)) => Some(e),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockCheckMode {
    Default,
    /// `unsafe { … }`
    Unsafe,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub id: NodeId,
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    /// `let x = e`, `var x = e`.
    Let(P<Local>),
    Item(P<Item>),
    /// An expression statement. Vertex has no `;` to tell a tail expression
    /// from a discarded one, so every expression statement is this kind; the
    /// type checker discards the value of a non-final one, and of a final one
    /// in a block whose type is `()`.
    Expr(P<Expr>),
}

#[derive(Clone, Debug)]
pub struct Local {
    pub id: NodeId,
    /// `var` instead of `let`: every binding in the pattern is mutable.
    pub is_var: bool,
    pub pat: P<Pat>,
    pub ty: Option<P<Ty>>,
    pub kind: LocalKind,
    pub span: Span,
    pub attrs: AttrVec,
}

#[derive(Clone, Debug)]
pub enum LocalKind {
    /// `let x`
    Decl,
    /// `let x = e`
    Init(P<Expr>),
    /// `let Some(x) = e else { … }`
    InitElse(P<Expr>, P<Block>),
}

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
