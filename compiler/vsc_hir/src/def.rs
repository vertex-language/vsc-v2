//! What a name refers to (rustc's `rustc_hir::def`).

use std::fmt;

use vsc_ast::{Mutability, NodeId};
use vsc_span::def_id::DefId;
use vsc_span::{Symbol, sym};

/// The two namespaces names live in, as in Rust. (Rust's third, for macros,
/// isn't needed: Vertex has no user macros.)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Namespace {
    /// Packages, types, traits, type parameters.
    TypeNS,
    /// Functions, constants, statics, locals, constructors.
    ValueNS,
}

impl Namespace {
    pub fn descr(self) -> &'static str {
        match self {
            Namespace::TypeNS => "type",
            Namespace::ValueNS => "value",
        }
    }
}

/// One value per namespace.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct PerNS<T> {
    pub type_ns: T,
    pub value_ns: T,
}

impl<T> PerNS<T> {
    pub fn get(&self, ns: Namespace) -> &T {
        match ns {
            Namespace::TypeNS => &self.type_ns,
            Namespace::ValueNS => &self.value_ns,
        }
    }

    pub fn get_mut(&mut self, ns: Namespace) -> &mut T {
        match ns {
            Namespace::TypeNS => &mut self.type_ns,
            Namespace::ValueNS => &mut self.value_ns,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CtorOf {
    Struct,
    Variant,
}

/// How a constructor is called.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CtorKind {
    /// `Meters(3.5)`
    Fn,
    /// `Marker`
    Const,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DefKind {
    // Type namespace.
    /// A package (Rust's `mod`), including the crate root.
    Mod,
    Struct,
    /// `class` (exception 3).
    Class,
    Union,
    Enum,
    Variant,
    Trait,
    TyAlias,
    /// `type FILE` in an extern block.
    ForeignTy,
    TyParam,
    /// An origin parameter, `src: Origin` (exception 13; Rust's lifetime parameter).
    OriginParam,
    AssocTy,

    // Value namespace.
    Fn,
    Const,
    ConstParam,
    Static { mutability: Mutability },
    Ctor(CtorOf, CtorKind),
    AssocFn,
    AssocConst,

    // Neither.
    Impl { of_trait: bool },
    ForeignMod,
    GlobalAsm,
    Field,
    /// A function literal (exception 14).
    Closure,
    AnonConst,
    InlineConst,
}

impl DefKind {
    pub fn descr(self) -> &'static str {
        match self {
            DefKind::Mod => "package",
            DefKind::Struct => "struct",
            DefKind::Class => "class",
            DefKind::Union => "union",
            DefKind::Enum => "enum",
            DefKind::Variant => "variant",
            DefKind::Trait => "trait",
            DefKind::TyAlias => "type alias",
            DefKind::ForeignTy => "foreign type",
            DefKind::TyParam => "type parameter",
            DefKind::OriginParam => "origin parameter",
            DefKind::AssocTy => "associated type",
            DefKind::Fn => "function",
            DefKind::Const => "constant",
            DefKind::ConstParam => "const parameter",
            DefKind::Static { .. } => "static",
            DefKind::Ctor(CtorOf::Struct, CtorKind::Fn) => "tuple struct",
            DefKind::Ctor(CtorOf::Struct, CtorKind::Const) => "unit struct",
            DefKind::Ctor(CtorOf::Variant, CtorKind::Fn) => "tuple variant",
            DefKind::Ctor(CtorOf::Variant, CtorKind::Const) => "unit variant",
            DefKind::AssocFn => "associated function",
            DefKind::AssocConst => "associated constant",
            DefKind::Impl { .. } => "implementation",
            DefKind::ForeignMod => "extern block",
            DefKind::GlobalAsm => "global asm",
            DefKind::Field => "field",
            DefKind::Closure => "function literal",
            DefKind::AnonConst => "constant expression",
            DefKind::InlineConst => "inline constant",
        }
    }

    /// The namespace a name of this kind lives in, if any.
    pub fn ns(self) -> Option<Namespace> {
        match self {
            DefKind::Mod
            | DefKind::Struct
            | DefKind::Class
            | DefKind::Union
            | DefKind::Enum
            | DefKind::Variant
            | DefKind::Trait
            | DefKind::TyAlias
            | DefKind::ForeignTy
            | DefKind::TyParam
            | DefKind::OriginParam
            | DefKind::AssocTy => Some(Namespace::TypeNS),
            DefKind::Fn
            | DefKind::Const
            | DefKind::ConstParam
            | DefKind::Static { .. }
            | DefKind::Ctor(..)
            | DefKind::AssocFn
            | DefKind::AssocConst => Some(Namespace::ValueNS),
            _ => None,
        }
    }

    /// Kinds whose names hold other names: packages, enums (variants) and
    /// traits (associated items).
    pub fn is_module_like(self) -> bool {
        matches!(self, DefKind::Mod | DefKind::Enum | DefKind::Trait)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum IntTy {
    /// `int`, Rust's `isize`.
    Isize,
    I8,
    I16,
    I32,
    I64,
    I128,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum UintTy {
    /// `uint`, Rust's `usize`.
    Usize,
    U8,
    U16,
    U32,
    U64,
    U128,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FloatTy {
    F32,
    F64,
}

/// The built-in types, by their Vertex names (exception 2).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PrimTy {
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    Bool,
    Char,
    /// `string`. Type lowering reads `&string` as Rust's `&str` and `string`
    /// alone as the `String` lang item.
    Str,
}

impl PrimTy {
    pub const ALL: [PrimTy; 17] = [
        PrimTy::Int(IntTy::Isize),
        PrimTy::Int(IntTy::I8),
        PrimTy::Int(IntTy::I16),
        PrimTy::Int(IntTy::I32),
        PrimTy::Int(IntTy::I64),
        PrimTy::Int(IntTy::I128),
        PrimTy::Uint(UintTy::Usize),
        PrimTy::Uint(UintTy::U8),
        PrimTy::Uint(UintTy::U16),
        PrimTy::Uint(UintTy::U32),
        PrimTy::Uint(UintTy::U64),
        PrimTy::Uint(UintTy::U128),
        PrimTy::Float(FloatTy::F32),
        PrimTy::Float(FloatTy::F64),
        PrimTy::Bool,
        PrimTy::Char,
        PrimTy::Str,
    ];

    pub fn name(self) -> Symbol {
        match self {
            PrimTy::Int(IntTy::Isize) => sym::int,
            PrimTy::Int(IntTy::I8) => sym::int8,
            PrimTy::Int(IntTy::I16) => sym::int16,
            PrimTy::Int(IntTy::I32) => sym::int32,
            PrimTy::Int(IntTy::I64) => sym::int64,
            PrimTy::Int(IntTy::I128) => sym::int128,
            PrimTy::Uint(UintTy::Usize) => sym::uint,
            PrimTy::Uint(UintTy::U8) => sym::uint8,
            PrimTy::Uint(UintTy::U16) => sym::uint16,
            PrimTy::Uint(UintTy::U32) => sym::uint32,
            PrimTy::Uint(UintTy::U64) => sym::uint64,
            PrimTy::Uint(UintTy::U128) => sym::uint128,
            PrimTy::Float(FloatTy::F32) => sym::float32,
            PrimTy::Float(FloatTy::F64) => sym::float64,
            PrimTy::Bool => sym::bool,
            PrimTy::Char => sym::char,
            PrimTy::Str => sym::string,
        }
    }

    pub fn from_name(name: Symbol) -> Option<PrimTy> {
        PrimTy::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Rust's name for the type, for diagnostics that compare with Rust.
    pub fn rust_name(self) -> &'static str {
        match self {
            PrimTy::Int(IntTy::Isize) => "isize",
            PrimTy::Int(IntTy::I8) => "i8",
            PrimTy::Int(IntTy::I16) => "i16",
            PrimTy::Int(IntTy::I32) => "i32",
            PrimTy::Int(IntTy::I64) => "i64",
            PrimTy::Int(IntTy::I128) => "i128",
            PrimTy::Uint(UintTy::Usize) => "usize",
            PrimTy::Uint(UintTy::U8) => "u8",
            PrimTy::Uint(UintTy::U16) => "u16",
            PrimTy::Uint(UintTy::U32) => "u32",
            PrimTy::Uint(UintTy::U64) => "u64",
            PrimTy::Uint(UintTy::U128) => "u128",
            PrimTy::Float(FloatTy::F32) => "f32",
            PrimTy::Float(FloatTy::F64) => "f64",
            PrimTy::Bool => "bool",
            PrimTy::Char => "char",
            PrimTy::Str => "str",
        }
    }
}

/// What a path resolved to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Res<Id = NodeId> {
    Def(DefKind, DefId),
    PrimTy(PrimTy),
    /// `Self` in a trait.
    SelfTyParam { trait_: DefId },
    /// `Self` in an impl: the impl's self type.
    SelfTyAlias { alias_to: DefId },
    /// `Self` as a constructor, in an impl of a tuple or unit struct.
    SelfCtor(DefId),
    /// A local binding: its pattern's id.
    Local(Id),
    /// A built-in call (`println`, `format`, `assert`, …), exception 12.
    Builtin(Symbol),
    /// A built-in type that isn't primitive: `cstring` and the C types
    /// (`c_int`, …). Lowering maps them to library types for the target.
    BuiltinTy(Symbol),
    /// Resolution failed and was reported.
    Err,
}

impl<Id> Res<Id> {
    /// The same resolution with locals' ids converted.
    pub fn map_id<R>(self, f: impl FnOnce(Id) -> R) -> Res<R> {
        match self {
            Res::Def(k, d) => Res::Def(k, d),
            Res::PrimTy(p) => Res::PrimTy(p),
            Res::SelfTyParam { trait_ } => Res::SelfTyParam { trait_ },
            Res::SelfTyAlias { alias_to } => Res::SelfTyAlias { alias_to },
            Res::SelfCtor(d) => Res::SelfCtor(d),
            Res::Local(id) => Res::Local(f(id)),
            Res::Builtin(s) => Res::Builtin(s),
            Res::BuiltinTy(s) => Res::BuiltinTy(s),
            Res::Err => Res::Err,
        }
    }

    pub fn def_id(&self) -> Option<DefId> {
        match *self {
            Res::Def(_, id) => Some(id),
            _ => None,
        }
    }

    pub fn descr(&self) -> &'static str {
        match self {
            Res::Def(kind, _) => kind.descr(),
            Res::PrimTy(_) => "builtin type",
            Res::SelfTyParam { .. } | Res::SelfTyAlias { .. } => "self type",
            Res::SelfCtor(_) => "self constructor",
            Res::Local(_) => "local variable",
            Res::Builtin(_) => "built-in",
            Res::BuiltinTy(_) => "builtin type",
            Res::Err => "unresolved item",
        }
    }

    pub fn ns(&self) -> Option<Namespace> {
        match self {
            Res::Def(kind, _) => kind.ns(),
            Res::PrimTy(_) | Res::SelfTyParam { .. } | Res::SelfTyAlias { .. } | Res::BuiltinTy(_) => {
                Some(Namespace::TypeNS)
            }
            Res::SelfCtor(_) | Res::Local(_) | Res::Builtin(_) => Some(Namespace::ValueNS),
            Res::Err => None,
        }
    }
}

/// A path's resolution and how many of its trailing segments it didn't
/// cover. `Task.New` gives `Task` with 1 unresolved segment (an associated
/// item, found by type checking); `self.items.Push` gives the local `self`
/// with 2 (a field, then a method), exception 7.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PartialRes {
    pub base_res: Res,
    pub unresolved_segments: usize,
}

impl PartialRes {
    pub fn new(base_res: Res) -> PartialRes {
        PartialRes { base_res, unresolved_segments: 0 }
    }

    pub fn with_unresolved_segments(base_res: Res, unresolved_segments: usize) -> PartialRes {
        PartialRes { base_res, unresolved_segments }
    }

    /// The resolution, if the path resolved completely.
    pub fn full_res(&self) -> Option<Res> {
        (self.unresolved_segments == 0).then_some(self.base_res)
    }
}

/// What an origin (`[a]`, exception 13) names.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum OriginRes<Id = NodeId> {
    /// An origin parameter: `src` in `struct S<src: Origin>`.
    Param(DefId),
    /// A function parameter: `a` in `func f(a: &string) -> &[a] string`. The
    /// id is the parameter's binding.
    FnParam(Id),
    /// `[static]`
    Static,
    /// Resolution failed and was reported.
    Err,
}

impl<Id> OriginRes<Id> {
    /// The same resolution with parameters' ids converted.
    pub fn map_id<R>(self, f: impl FnOnce(Id) -> R) -> OriginRes<R> {
        match self {
            OriginRes::Param(d) => OriginRes::Param(d),
            OriginRes::FnParam(id) => OriginRes::FnParam(f(id)),
            OriginRes::Static => OriginRes::Static,
            OriginRes::Err => OriginRes::Err,
        }
    }
}

impl fmt::Display for PrimTy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name().as_str())
    }
}
