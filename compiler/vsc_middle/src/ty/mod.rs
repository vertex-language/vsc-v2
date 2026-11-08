//! Semantic types (rustc's `rustc_middle::ty` and `rustc_type_ir`).
//!
//! Types, regions, constants and generic argument lists are interned in
//! the `TyCtxt`'s arena: two equal types are the same reference, so
//! comparing and hashing them is comparing addresses.
//!
//! Vertex's exceptions mostly end before here. What reaches types:
//!
//! - Exception 2: `string` is the `String` lang item, `&string` is `&str`;
//!   `cstring` and `&cstring` likewise. `hir_ty_lowering` decides.
//! - Exception 3: a use of a `class` type is the `Class<T>` lang item
//!   around the class's own struct type.
//! - Exception 13: origins are regions. `[a, b]` is a region outlived by
//!   both, recorded on the function signature's binder (`BoundVar`).
//! - Exception 14: `func(T) -> U` is the `Func` lang item applied to the
//!   signature as a function pointer type.

mod adt;
mod context;
pub mod fold;
mod generics;
mod print;
mod typeck_results;

use std::fmt;
use std::hash::{Hash, Hasher};

pub use adt::{AdtDef, AdtDefData, AdtKind, FieldDef, VariantDef};
pub use context::{CommonRegions, CommonTypes, CtxtInputs, DefInfo, GlobalCtxt, TyCtxt};
pub use generics::{GenericParamDef, GenericParamDefKind, Generics};
pub use typeck_results::{Adjust, Adjustment, TypeckResults};
pub use vsc_ast::Mutability;
pub use vsc_hir::def::{FloatTy, IntTy, UintTy};
use vsc_span::Symbol;
use vsc_span::def_id::DefId;

/// Implements equality and hashing by address, for interned references.
macro_rules! interned {
    ($name:ident, $kind:ident) => {
        #[derive(Clone, Copy)]
        pub struct $name<'tcx>(pub(crate) &'tcx $kind<'tcx>);

        impl<'tcx> $name<'tcx> {
            #[inline]
            pub fn kind(self) -> &'tcx $kind<'tcx> {
                self.0
            }
        }

        impl PartialEq for $name<'_> {
            #[inline]
            fn eq(&self, other: &Self) -> bool {
                std::ptr::eq(self.0, other.0)
            }
        }
        impl Eq for $name<'_> {}

        impl Hash for $name<'_> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                (self.0 as *const $kind<'_>).hash(state)
            }
        }

        impl fmt::Debug for $name<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Debug::fmt(self.0, f)
            }
        }
    };
}

interned!(Ty, TyKind);
interned!(Const, ConstKind);

/// A region (Rust's lifetime; Vertex's origin, exception 13).
#[derive(Clone, Copy)]
pub struct Region<'tcx>(pub(crate) &'tcx RegionKind);

impl<'tcx> Region<'tcx> {
    pub fn kind(self) -> &'tcx RegionKind {
        self.0
    }
}
impl PartialEq for Region<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0, other.0)
    }
}
impl Eq for Region<'_> {}
impl Hash for Region<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.0 as *const RegionKind).hash(state)
    }
}
impl fmt::Debug for Region<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0, f)
    }
}

pub type GenericArgsRef<'tcx> = &'tcx [GenericArg<'tcx>];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TyKind<'tcx> {
    Bool,
    Char,
    Int(IntTy),
    Uint(UintTy),
    Float(FloatTy),
    /// Rust's `str`: Vertex writes `&string` for `&str` (exception 2).
    Str,
    Adt(AdtDef<'tcx>, GenericArgsRef<'tcx>),
    /// `type FILE` in an extern block.
    Foreign(DefId),
    Array(Ty<'tcx>, Const<'tcx>),
    Slice(Ty<'tcx>),
    RawPtr(Ty<'tcx>, Mutability),
    Ref(Region<'tcx>, Ty<'tcx>, Mutability),
    /// The zero-sized type of a function item.
    FnDef(DefId, GenericArgsRef<'tcx>),
    FnPtr(PolyFnSig<'tcx>),
    /// `dyn Trait + [a]`
    Dynamic(&'tcx [ExistentialPredicate<'tcx>], Region<'tcx>),
    /// A function literal's type (exception 14).
    Closure(DefId, GenericArgsRef<'tcx>),
    /// `never` (exception 2), Rust's `!`.
    Never,
    Tuple(&'tcx [Ty<'tcx>]),
    /// `<T as Trait>::Assoc`
    Alias(AliasTy<'tcx>),
    Param(ParamTy),
    /// A type being inferred, inside type checking.
    Infer(InferTy),
    /// An error already reported.
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum InferTy {
    TyVar(u32),
    /// An integer literal's type: `{integer}`.
    IntVar(u32),
    /// A float literal's type: `{float}`.
    FloatVar(u32),
    /// A `_` written in a type inside a body; type checking replaces each
    /// with a new type variable.
    Fresh(u32),
}

/// A type parameter: its index in the generics (parents' first) and name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ParamTy {
    pub index: u32,
    pub name: Symbol,
}

/// A projection, `<T as Trait>::Assoc`: the associated type and its
/// arguments, the trait's (`Self` first) then the item's own.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct AliasTy<'tcx> {
    pub def_id: DefId,
    pub args: GenericArgsRef<'tcx>,
}

impl<'tcx> AliasTy<'tcx> {
    pub fn self_ty(self) -> Ty<'tcx> {
        self.args[0].expect_ty()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RegionKind {
    /// An origin parameter of an item, `src: Origin`.
    ReEarlyParam(EarlyParamRegion),
    /// A region bound by a function signature's binder: an elided input
    /// reference, or `[a, b]`.
    ReBound(DebruijnIndex, BoundRegion),
    /// `[static]`
    ReStatic,
    /// A region in a body, left for type checking and borrow checking.
    ReErased,
    ReError,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct EarlyParamRegion {
    pub index: u32,
    pub name: Symbol,
}

/// How many binders out a bound region's binder is (0 = innermost).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DebruijnIndex(pub u32);

impl DebruijnIndex {
    pub const INNERMOST: DebruijnIndex = DebruijnIndex(0);
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BoundRegion {
    /// The index in the binder's `bound_vars`, which holds its name.
    pub var: u32,
}

/// A variable bound by a `Binder`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BoundVar<'tcx> {
    /// The parameter an origin named it by (`[a]`), for printing; `Empty`
    /// if it was only elided.
    pub name: Symbol,
    /// Exception 13's `[a, b]`: the variables (by index) that outlive this
    /// one. Empty for ordinary regions.
    pub outlived_by: &'tcx [u32],
}

/// A value with bound regions: a function signature, whose elided input
/// references are each their own region.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Binder<'tcx, T> {
    pub value: T,
    pub bound_vars: &'tcx [BoundVar<'tcx>],
}

impl<'tcx, T> Binder<'tcx, T> {
    pub fn dummy(value: T) -> Self {
        Binder { value, bound_vars: &[] }
    }

    pub fn skip_binder(self) -> T {
        self.value
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Safety {
    Safe,
    Unsafe,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FnSig<'tcx> {
    /// The inputs, then the output.
    pub inputs_and_output: &'tcx [Ty<'tcx>],
    pub c_variadic: bool,
    pub safety: Safety,
    /// `None` for Rust's ABI.
    pub abi: Option<Symbol>,
}

impl<'tcx> FnSig<'tcx> {
    pub fn inputs(&self) -> &'tcx [Ty<'tcx>] {
        &self.inputs_and_output[..self.inputs_and_output.len() - 1]
    }

    pub fn output(&self) -> Ty<'tcx> {
        *self.inputs_and_output.last().unwrap()
    }
}

pub type PolyFnSig<'tcx> = Binder<'tcx, FnSig<'tcx>>;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ConstKind<'tcx> {
    /// A const parameter.
    Param(ParamConst),
    /// A known value, as its bits.
    Value(Ty<'tcx>, u128),
    /// An expression to evaluate: an anonymous constant's body.
    Unevaluated(DefId, GenericArgsRef<'tcx>),
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ParamConst {
    pub index: u32,
    pub name: Symbol,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GenericArg<'tcx> {
    Region(Region<'tcx>),
    Type(Ty<'tcx>),
    Const(Const<'tcx>),
}

impl<'tcx> GenericArg<'tcx> {
    #[track_caller]
    pub fn expect_ty(self) -> Ty<'tcx> {
        match self {
            GenericArg::Type(t) => t,
            other => panic!("expected a type, found {other:?}"),
        }
    }
}

/// `T: Trait<A>`: the trait and its arguments, `Self` first.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TraitRef<'tcx> {
    pub def_id: DefId,
    pub args: GenericArgsRef<'tcx>,
}

impl<'tcx> TraitRef<'tcx> {
    pub fn self_ty(self) -> Ty<'tcx> {
        self.args[0].expect_ty()
    }
}

/// A trait in a `dyn` type, without `Self`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ExistentialPredicate<'tcx> {
    Trait { def_id: DefId, args: GenericArgsRef<'tcx> },
    /// `Iterator<Item = T>`'s `Item = T`.
    Projection { def_id: DefId, args: GenericArgsRef<'tcx>, term: Ty<'tcx> },
}

/// What an item's where clauses and bounds require.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Clause<'tcx> {
    Trait(TraitRef<'tcx>),
    /// `<T as Trait>::Assoc == U`
    Projection(AliasTy<'tcx>, Ty<'tcx>),
    /// `T: [a]`
    TypeOutlives(Ty<'tcx>, Region<'tcx>),
    /// `a: [b]`
    RegionOutlives(Region<'tcx>, Region<'tcx>),
}

/// An item's predicates: its parent's, then its own.
#[derive(Clone, Copy, Debug)]
pub struct GenericPredicates<'tcx> {
    pub parent: Option<DefId>,
    pub predicates: &'tcx [(Clause<'tcx>, vsc_span::Span)],
}

/// One associated item of a trait or impl.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AssocItem {
    pub def_id: DefId,
    pub name: Symbol,
    pub kind: AssocKind,
    /// The trait or impl it's in.
    pub container: DefId,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssocKind {
    Const,
    Fn { has_self: bool },
    Type,
}
