//! Core type, value, function signature, and basic-block handles for `vsc_codegen_obsidian`.

use obsidian::ir;

/// Function signature descriptor interned inside [`crate::context::CodegenCx`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ObsFnSig {
    pub params: Vec<ir::Type>,
    pub returns: Vec<ir::Type>,
    pub call_conv: ir::CallConv,
    pub is_variadic: bool,
}

/// Lightweight, `Copy` type handle for Obsidian codegen.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ObsType {
    /// One of the `.obs` primitive scalar types (`Void`, `I1`, `I8`, `I16`, `I32`, `I64`, `F32`, `F64`, `Ptr`).
    Prim(ir::Type),
    /// Two-scalar pair (`{ a, b }`, e.g. fat pointers, checked arithmetic results, `ScalarPair` structs).
    Pair(ir::Type, ir::Type),
    /// Fixed-size array or byte aggregate (`[count x elem]`).
    Array { elem: ir::Type, count: u64 },
    /// Interned function signature ID in `CodegenCx::fn_sigs`.
    Func(u32),
}

impl ObsType {
    /// Returns the underlying primitive `.obs` type, or `Type::Ptr` for aggregates/functions.
    #[inline]
    pub const fn as_ir_type(self) -> ir::Type {
        match self {
            Self::Prim(ty) => ty,
            Self::Pair(a, _) => a,
            Self::Array { elem, .. } => elem,
            Self::Func(_) => ir::Type::Ptr,
        }
    }

    /// Returns the byte size of this type on a 64-bit target.
    #[inline]
    pub const fn byte_size(self) -> u64 {
        match self {
            Self::Prim(ir::Type::Void) => 0,
            Self::Prim(ir::Type::I1 | ir::Type::I8) => 1,
            Self::Prim(ir::Type::I16) => 2,
            Self::Prim(ir::Type::I32 | ir::Type::F32) => 4,
            Self::Prim(ir::Type::I64 | ir::Type::F64 | ir::Type::Ptr) => 8,
            Self::Prim(ir::Type::I128 | ir::Type::V128) => 16,
            Self::Pair(a, b) => Self::Prim(a).byte_size() + Self::Prim(b).byte_size(),
            Self::Array { elem, count } => Self::Prim(elem).byte_size() * count,
            Self::Func(_) => 8,
        }
    }
}

/// A single scalar value or symbol/stack-slot reference inside `vsc_codegen_obsidian`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ObsScalar {
    /// An SSA `ir::Value` inside the active `ir::Function`, paired with its `.obs` type.
    Ssa(ir::Value, ir::Type),
    /// A compile-time scalar integer or floating-point bit constant.
    ConstBits(ir::Type, u64),
    /// Address of a module-level symbol (`const`, `global`, or `fn`) plus a signed byte offset.
    SymAddr {
        sym: ir::Sym,
        is_const: bool,
        offset: i64,
    },
    /// Frame-relative address of an `Opcode::StackSlot` instruction plus a signed byte offset.
    StackSlot {
        slot_val: ir::Value,
        offset: i32,
    },
    /// Undefined / poison placeholder of type `ty` (materializes as zero bits).
    Undef(ir::Type),
}

impl ObsScalar {
    #[inline]
    pub const fn ty(self) -> ir::Type {
        match self {
            Self::Ssa(_, ty) | Self::ConstBits(ty, _) | Self::Undef(ty) => ty,
            Self::SymAddr { .. } | Self::StackSlot { .. } => ir::Type::Ptr,
        }
    }
}

/// Lightweight, `Copy` value handle for Obsidian codegen.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ObsVal {
    /// Single scalar value, constant, symbol address, or stack-slot pointer.
    Scalar(ObsScalar),
    /// Two-scalar aggregate (used for 2-register `Pair` returns and `const_struct` pairs).
    Pair(ObsScalar, ObsScalar),
}

impl ObsVal {
    #[inline]
    pub const fn ssa(val: ir::Value, ty: ir::Type) -> Self {
        Self::Scalar(ObsScalar::Ssa(val, ty))
    }

    #[inline]
    pub const fn const_bits(ty: ir::Type, bits: u64) -> Self {
        Self::Scalar(ObsScalar::ConstBits(ty, bits))
    }

    #[inline]
    pub const fn sym_addr(sym: ir::Sym, is_const: bool) -> Self {
        Self::Scalar(ObsScalar::SymAddr {
            sym,
            is_const,
            offset: 0,
        })
    }

    #[inline]
    pub const fn stack_slot(slot_val: ir::Value, offset: i32) -> Self {
        Self::Scalar(ObsScalar::StackSlot { slot_val, offset })
    }

    #[inline]
    pub const fn undef(ty: ir::Type) -> Self {
        Self::Scalar(ObsScalar::Undef(ty))
    }

    #[inline]
    pub const fn as_scalar(self) -> ObsScalar {
        match self {
            Self::Scalar(s) => s,
            Self::Pair(a, _) => a,
        }
    }

    #[inline]
    pub const fn ty(self) -> ObsType {
        match self {
            Self::Scalar(s) => ObsType::Prim(s.ty()),
            Self::Pair(a, b) => ObsType::Pair(a.ty(), b.ty()),
        }
    }

    #[inline]
    pub const fn as_sym(self) -> Option<(ir::Sym, bool, i64)> {
        match self {
            Self::Scalar(ObsScalar::SymAddr { sym, is_const, offset }) => Some((sym, is_const, offset)),
            _ => None,
        }
    }
}

/// Basic block handle targeting a specific function and block inside an `.obs` module.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ObsBlock {
    pub func_sym: ir::Sym,
    pub block: ir::Block,
}
