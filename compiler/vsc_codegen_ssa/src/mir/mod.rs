//! Backend-independent SSA MIR value & place representations (`rustc_codegen_ssa::mir`).

use vsc_abi::{Align, Size};
use vsc_middle::ty::layout::TyAndLayout;

/// Backend value held in an SSA operand.
#[derive(Clone, Copy, Debug)]
pub enum OperandValue<V> {
    /// Value held directly in memory at the given pointer (`Ref(ptr, meta, align)`).
    Ref(PlaceValue<V>),
    /// Scalar immediate value (`bool`, ints, floats, thin pointers).
    Immediate(V),
    /// Two-scalar immediate pair (fat pointers `&str`/`&[T]`, `Func<F>`, scalar-pair structs/enums).
    Pair(V, V),
    /// Zero-sized value (needs no runtime bits).
    ZeroSized,
}

/// Address + optional fat-pointer metadata + alignment for a place in memory.
#[derive(Clone, Copy, Debug)]
pub struct PlaceValue<V> {
    pub llval: V,
    pub llextra: Option<V>,
    pub align: Align,
}

impl<V> PlaceValue<V> {
    pub fn new_sized(llval: V, align: Align) -> Self {
        Self { llval, llextra: None, align }
    }

    pub fn new_unsized(llval: V, llextra: V, align: Align) -> Self {
        Self { llval, llextra: Some(llextra), align }
    }
}

/// Typed place in memory.
#[derive(Clone, Copy, Debug)]
pub struct PlaceRef<'tcx, V> {
    pub val: PlaceValue<V>,
    pub layout: TyAndLayout<'tcx>,
}

impl<'tcx, V> PlaceRef<'tcx, V> {
    pub fn new_sized(llval: V, layout: TyAndLayout<'tcx>) -> Self {
        Self { val: PlaceValue::new_sized(llval, layout.align), layout }
    }
}

/// Typed operand value.
#[derive(Clone, Copy, Debug)]
pub struct OperandRef<'tcx, V> {
    pub val: OperandValue<V>,
    pub layout: TyAndLayout<'tcx>,
}

/// Byte offset and align helper when projecting into a place.
#[inline]
pub fn align_for_offset(base_align: Align, offset: Size) -> Align {
    let combined = base_align.bytes() | offset.bytes();
    let pow2 = 1u64 << combined.trailing_zeros().min(63);
    Align::from_bytes(pow2.min(base_align.bytes())).unwrap_or(Align::ONE)
}
