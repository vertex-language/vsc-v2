//! Shared Obsidian target mapping, predicate conversion, and MIR discriminant lookup.

use obsidian::ir;
use vsc_codegen_ssa::common::{IntPredicate, RealPredicate};
use vsc_middle::thir;
use vsc_middle::ty::{AdtDef, TyCtxt};
use vsc_span::def_id::DefId;

/// Maps a Vertex [`vsc_target::spec::Target`] to an [`obsidian::ir::Target`]
/// across the 3 host OSes (`Linux`, `macOS`, `Windows`) and 2 architectures (`x86_64`, `arm64`).
pub fn obsidian_target_for_spec(spec: &vsc_target::spec::Target) -> ir::Target {
    let is_arm64 = spec.arch == vsc_target::spec::Arch::AArch64
        || spec.target_triple.starts_with("aarch64")
        || spec.target_triple.starts_with("arm64");
    let os = spec.options.os.as_str();
    let is_macos = os == "macos"
        || os == "ios"
        || spec.target_triple.contains("apple")
        || spec.target_triple.contains("darwin");
    let is_windows = os == "windows" || spec.target_triple.contains("windows");

    match (is_arm64, is_macos, is_windows) {
        (false, true, false) => ir::Target::x86_64_macos(),
        (false, false, true) => ir::Target::x86_64_windows(),
        (false, _, _) => ir::Target::x86_64_linux(),
        (true, true, false) => ir::Target::aarch64_macos(),
        (true, false, true) => ir::Target::aarch64_windows(),
        (true, _, _) => ir::Target::aarch64_linux(),
    }
}

/// Converts a backend-agnostic integer predicate to [`obsidian::ir::IntCC`].
pub const fn int_predicate_to_obs(op: IntPredicate) -> ir::IntCC {
    match op {
        IntPredicate::IntEQ => ir::IntCC::Eq,
        IntPredicate::IntNE => ir::IntCC::Ne,
        IntPredicate::IntUGT => ir::IntCC::Ugt,
        IntPredicate::IntUGE => ir::IntCC::Uge,
        IntPredicate::IntULT => ir::IntCC::Ult,
        IntPredicate::IntULE => ir::IntCC::Ule,
        IntPredicate::IntSGT => ir::IntCC::Sgt,
        IntPredicate::IntSGE => ir::IntCC::Sge,
        IntPredicate::IntSLT => ir::IntCC::Slt,
        IntPredicate::IntSLE => ir::IntCC::Sle,
    }
}

/// Converts a backend-agnostic floating-point predicate to [`obsidian::ir::FloatCC`].
pub const fn real_predicate_to_obs(op: RealPredicate) -> ir::FloatCC {
    match op {
        RealPredicate::RealOEQ | RealPredicate::RealUEQ => ir::FloatCC::Eq,
        RealPredicate::RealOGT | RealPredicate::RealUGT => ir::FloatCC::Gt,
        RealPredicate::RealOGE | RealPredicate::RealUGE => ir::FloatCC::Ge,
        RealPredicate::RealOLT | RealPredicate::RealULT => ir::FloatCC::Lt,
        RealPredicate::RealOLE | RealPredicate::RealULE => ir::FloatCC::Le,
        RealPredicate::RealONE | RealPredicate::RealUNE => ir::FloatCC::Ne,
        RealPredicate::RealORD | RealPredicate::RealPredicateTrue => ir::FloatCC::Ord,
        RealPredicate::RealUNO | RealPredicate::RealPredicateFalse => ir::FloatCC::Uno,
    }
}

/// Computes the signed integer discriminant for enum `variant`, matching `vsc_mir_build`.
pub fn discriminant_value(tcx: TyCtxt<'_>, adt: AdtDef<'_>, variant: usize) -> i128 {
    let mut value: i128 = -1;
    for (i, v) in adt.variants().iter().enumerate() {
        value = match v.discr.and_then(|d| eval_literal_discriminant(tcx, d)) {
            Some(x) => x,
            None => value + 1,
        };
        if i == variant {
            return value;
        }
    }
    value
}

fn eval_literal_discriminant(tcx: TyCtxt<'_>, anon: DefId) -> Option<i128> {
    let thir = tcx.thir_body(anon);
    let mut e = &thir.exprs[thir.value?];
    loop {
        match &e.kind {
            thir::ExprKind::Literal { lit, neg } => {
                let v = thir::parse_int_lit(lit.symbol.as_str())? as i128;
                return Some(if *neg { -v } else { v });
            }
            thir::ExprKind::Use { source } => e = &thir.exprs[*source],
            thir::ExprKind::Block { block } => e = &thir.exprs[thir.blocks[*block].expr?],
            _ => return None,
        }
    }
}
