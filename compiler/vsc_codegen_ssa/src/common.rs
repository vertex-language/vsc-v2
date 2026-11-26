//! Shared backend helpers and predicates (`rustc_codegen_ssa::common`).

use vsc_abi::BackendRepr;
use vsc_middle::mir::BinOp;
use vsc_middle::ty::layout::TyAndLayout;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntPredicate {
    IntEQ,
    IntNE,
    IntUGT,
    IntUGE,
    IntULT,
    IntULE,
    IntSGT,
    IntSGE,
    IntSLT,
    IntSLE,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RealPredicate {
    RealPredicateFalse,
    RealOEQ,
    RealOGT,
    RealOGE,
    RealOLT,
    RealOLE,
    RealONE,
    RealORD,
    RealUNO,
    RealUEQ,
    RealUGT,
    RealUGE,
    RealULT,
    RealULE,
    RealUNE,
    RealPredicateTrue,
}

pub fn int_predicate(op: BinOp, signed: bool) -> IntPredicate {
    match (op, signed) {
        (BinOp::Eq, _) => IntPredicate::IntEQ,
        (BinOp::Ne, _) => IntPredicate::IntNE,
        (BinOp::Lt, true) => IntPredicate::IntSLT,
        (BinOp::Lt, false) => IntPredicate::IntULT,
        (BinOp::Le, true) => IntPredicate::IntSLE,
        (BinOp::Le, false) => IntPredicate::IntULE,
        (BinOp::Gt, true) => IntPredicate::IntSGT,
        (BinOp::Gt, false) => IntPredicate::IntUGT,
        (BinOp::Ge, true) => IntPredicate::IntSGE,
        (BinOp::Ge, false) => IntPredicate::IntUGE,
        other => panic!("not an integer comparison op: {other:?}"),
    }
}

pub fn real_predicate(op: BinOp) -> RealPredicate {
    match op {
        BinOp::Eq => RealPredicate::RealOEQ,
        BinOp::Ne => RealPredicate::RealUNE,
        BinOp::Lt => RealPredicate::RealOLT,
        BinOp::Le => RealPredicate::RealOLE,
        BinOp::Gt => RealPredicate::RealOGT,
        BinOp::Ge => RealPredicate::RealUGE,
        other => panic!("not a float comparison op: {other:?}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MemFlags(u8);

impl MemFlags {
    pub const VOLATILE: Self = Self(1 << 0);
    pub const NON_TEMPORAL: Self = Self(1 << 1);
    pub const UNALIGNED: Self = Self(1 << 2);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

#[inline]
pub fn type_is_immediate<'tcx>(layout: TyAndLayout<'tcx>) -> bool {
    matches!(layout.backend_repr, BackendRepr::Scalar(_) | BackendRepr::SimdVector { .. })
}

#[inline]
pub fn type_is_scalar_pair<'tcx>(layout: TyAndLayout<'tcx>) -> bool {
    matches!(layout.backend_repr, BackendRepr::ScalarPair { .. })
}

#[inline]
pub fn type_is_zero_sized<'tcx>(layout: TyAndLayout<'tcx>) -> bool {
    layout.is_zst()
}
