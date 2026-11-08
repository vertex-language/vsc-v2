//! The interpreter's values and memory.
//!
//! Memory is a set of allocations, each holding one value tree (a local, a
//! static, a class object, a string literal). A pointer is an allocation
//! and a path into its tree; a slice pointer also has a start and length
//! within the array it points into. This is simpler than Miri's byte-level
//! memory and enough for safe code and simple raw pointers; reinterpreting
//! memory (transmutes, pointer arithmetic) is reported as unsupported.

use std::rc::Rc;

use vsc_middle::ty::{GenericArgsRef, Ty};
use vsc_span::def_id::DefId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AllocId(pub usize);

#[derive(Clone, PartialEq, Debug)]
pub struct Pointer {
    pub alloc: AllocId,
    pub path: Vec<usize>,
}

/// An integer: its bits (masked to its size) and its type's shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Int {
    pub bits: u128,
    pub size: u32,
    pub signed: bool,
}

impl Int {
    pub fn mask(size: u32) -> u128 {
        if size >= 128 { u128::MAX } else { (1u128 << size) - 1 }
    }

    pub fn new(value: i128, size: u32, signed: bool) -> Int {
        Int { bits: (value as u128) & Int::mask(size), size, signed }
    }

    pub fn from_bits(bits: u128, size: u32, signed: bool) -> Int {
        Int { bits: bits & Int::mask(size), size, signed }
    }

    /// The value, sign-extended for signed types.
    pub fn to_i128(self) -> i128 {
        if self.signed && self.size < 128 {
            let shift = 128 - self.size;
            ((self.bits << shift) as i128) >> shift
        } else {
            self.bits as i128
        }
    }

    pub fn min(size: u32, signed: bool) -> i128 {
        if signed { -(1i128 << (size - 1).min(126)) * if size == 128 { 2 } else { 1 } } else { 0 }
    }

    /// Does `v` fit in the type?
    pub fn fits(v: i128, size: u32, signed: bool) -> bool {
        if size >= 128 {
            return true;
        }
        if signed {
            let min = -(1i128 << (size - 1));
            let max = (1i128 << (size - 1)) - 1;
            v >= min && v <= max
        } else {
            v >= 0 && (v as u128) <= Int::mask(size)
        }
    }
}

#[derive(Clone, Debug)]
pub enum Value<'tcx> {
    Uninit,
    Bool(bool),
    Char(char),
    Int(Int),
    F32(f32),
    F64(f64),
    Tuple(Vec<Value<'tcx>>),
    Array(Vec<Value<'tcx>>),
    /// A struct, union or enum value: the variant and its fields.
    Adt { variant: usize, fields: Vec<Value<'tcx>> },
    /// A reference or raw pointer to a sized value.
    Ptr(Pointer),
    /// A reference to a slice: the array it's in, a start and a length.
    SlicePtr { array: Pointer, start: usize, len: usize },
    /// A function item or function pointer.
    FnDef(DefId, GenericArgsRef<'tcx>),
    /// A function literal: the arguments of the body it was made in, and
    /// its captures.
    Closure { def: DefId, args: GenericArgsRef<'tcx>, captures: Vec<Value<'tcx>> },
    /// A `func` value (exception 14).
    Func(Box<Value<'tcx>>),
    /// A class value (exception 3): a shared object.
    Class(AllocId),
    /// A `Vec`'s elements, kept natively until the library exists.
    Vec(Vec<Value<'tcx>>),
    /// A `string` (Rust's `String`), kept natively.
    Str(String),
    /// The format built-ins' values.
    FmtArgs(Rc<FmtArgs<'tcx>>),
    FmtArg(Rc<FmtArg<'tcx>>),
    Placeholder(Placeholder),
}

impl<'tcx> Value<'tcx> {
    pub fn unit() -> Self {
        Value::Tuple(Vec::new())
    }
}

/// How an argument is formatted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FmtTrait {
    Display,
    Debug,
    LowerExp,
    UpperExp,
    Octal,
    Pointer,
    Binary,
    LowerHex,
    UpperHex,
    /// A width or precision argument.
    Usize,
}

#[derive(Debug)]
pub struct FmtArg<'tcx> {
    pub trait_: FmtTrait,
    /// A pointer to the value.
    pub value: Value<'tcx>,
    /// The value's type.
    pub ty: Ty<'tcx>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Count {
    Is(usize),
    Param(usize),
    Implied,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placeholder {
    pub position: usize,
    pub fill: char,
    /// 0 left, 1 right, 2 center, 3 unknown (the type's default).
    pub align: u8,
    pub flags: u32,
    pub precision: Count,
    pub width: Count,
}

#[derive(Debug)]
pub struct FmtArgs<'tcx> {
    pub pieces: Vec<String>,
    pub args: Vec<Rc<FmtArg<'tcx>>>,
    pub placeholders: Option<Vec<Placeholder>>,
}

/// Where a place is: an allocation, a path into it, and, for a slice, the
/// part of the array it covers.
#[derive(Clone, Debug)]
pub struct Location {
    pub alloc: AllocId,
    pub path: Vec<usize>,
    pub slice: Option<(usize, usize)>,
}

impl Location {
    pub fn of(alloc: AllocId) -> Self {
        Location { alloc, path: Vec::new(), slice: None }
    }
}
