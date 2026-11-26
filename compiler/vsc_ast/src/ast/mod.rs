//! AST nodes.
//!
//! Names follow rustc's `rustc_ast::ast`. Where Vertex adds something, the doc
//! comment names the exception it comes from.

mod expr;
mod item;
mod ty_pat;

pub use expr::*;
pub use item::*;
pub use ty_pat::*;

use vsc_span::{Ident, Span, Symbol};

use crate::node_id::NodeId;
use crate::token::{self, CommentKind, Delimiter};
use crate::tokenstream::{DelimSpan, TokenTree};

pub type P<T> = Box<T>;
pub type AttrVec = Vec<Attribute>;

// ---------------------------------------------------------------------------
// Crates, packages and source files (exception 1)
// ---------------------------------------------------------------------------

/// A crate: the package tree under the folder of its root file, plus any
/// loaded source-level external crates (such as `core`).
#[derive(Clone, Debug)]
pub struct Crate {
    pub root: Package,
    pub extern_crates: Vec<Package>,
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

