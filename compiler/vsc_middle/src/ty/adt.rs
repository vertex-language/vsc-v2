//! Structs, enums, unions and classes as types (rustc's `ty/adt.rs`).

use std::fmt;
use std::hash::{Hash, Hasher};

use vsc_hir::def::CtorKind;
use vsc_span::Symbol;
use vsc_span::def_id::DefId;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AdtKind {
    Struct,
    Union,
    Enum,
}

/// An algebraic data type, interned: compared and hashed by address.
#[derive(Clone, Copy)]
pub struct AdtDef<'tcx>(pub(crate) &'tcx AdtDefData);

#[derive(Debug)]
pub struct AdtDefData {
    pub did: DefId,
    pub kind: AdtKind,
    /// Declared with `class` (exception 3). The type is the struct; uses
    /// of it are wrapped in the `Class` lang item.
    pub is_class: bool,
    /// One variant for structs and unions.
    pub variants: Vec<VariantDef>,
}

#[derive(Debug)]
pub struct VariantDef {
    /// The variant's definition, or the struct's.
    pub def_id: DefId,
    pub ctor: Option<(CtorKind, DefId)>,
    pub name: Symbol,
    /// The discriminant's anonymous constant, when written.
    pub discr: Option<DefId>,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug)]
pub struct FieldDef {
    pub did: DefId,
    pub name: Symbol,
}

impl<'tcx> AdtDef<'tcx> {
    pub fn did(self) -> DefId {
        self.0.did
    }

    pub fn kind(self) -> AdtKind {
        self.0.kind
    }

    pub fn is_class(self) -> bool {
        self.0.is_class
    }

    pub fn variants(self) -> &'tcx [VariantDef] {
        &self.0.variants
    }

    pub fn is_enum(self) -> bool {
        self.0.kind == AdtKind::Enum
    }

    /// The only variant of a struct or union.
    #[track_caller]
    pub fn non_enum_variant(self) -> &'tcx VariantDef {
        assert!(!self.is_enum(), "non_enum_variant of an enum");
        &self.0.variants[0]
    }

    pub fn descr(self) -> &'static str {
        match self.0.kind {
            AdtKind::Struct if self.0.is_class => "class",
            AdtKind::Struct => "struct",
            AdtKind::Union => "union",
            AdtKind::Enum => "enum",
        }
    }
}

impl PartialEq for AdtDef<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0, other.0)
    }
}
impl Eq for AdtDef<'_> {}
impl Hash for AdtDef<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.0 as *const AdtDefData).hash(state)
    }
}
impl fmt::Debug for AdtDef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AdtDef({:?})", self.0.did)
    }
}
