//! Translates `FnAbi` and `CastTarget` to [`ObsType`] and [`obsidian::ir::CallConv`] (`vsc_codegen_obsidian::abi`).

use obsidian::ir;
use vsc_abi::{CanonAbi, Primitive, Reg, RegKind, Size};
use vsc_middle::ty::Ty;
use vsc_target::callconv::{CastTarget, FnAbi, PassMode};

use crate::context::CodegenCx;
use crate::type_of::scalar_pair_element_obs_type;
use crate::value::ObsType;

/// Converts `CanonAbi` to [`obsidian::ir::CallConv`].
/// We use platform C ABI (`CallConv::C`) for all functions so direct and indirect calls
/// interoperate seamlessly across C FFI, closures, and trampolines on all 6 targets.
pub const fn to_obs_callconv(_conv: CanonAbi) -> ir::CallConv {
    ir::CallConv::C
}

/// Computes the [`ObsType::Func`] signature for `fn_abi`.
pub fn fn_abi_obs_type<'tcx>(cx: &CodegenCx<'tcx>, fn_abi: &FnAbi<'tcx, Ty<'tcx>>) -> ObsType {
    let mut arg_tys = Vec::with_capacity(fn_abi.args.len() + 1);
    let ret_ty = match &fn_abi.ret.mode {
        PassMode::Ignore => cx.type_void(),
        PassMode::Direct(_) => cx.immediate_obs_type(fn_abi.ret.layout),
        PassMode::Pair(_, _) => cx.type_struct(
            &[
                scalar_pair_element_obs_type(cx, fn_abi.ret.layout, 0, true),
                scalar_pair_element_obs_type(cx, fn_abi.ret.layout, 1, true),
            ],
            false,
        ),
        PassMode::Cast { cast, .. } => cast_target_obs_type(cx, cast),
        PassMode::Indirect { address_space, .. } => {
            arg_tys.push(cx.type_ptr_in(address_space.map_or(0, |a| a.0)));
            cx.type_void()
        }
    };

    for arg in &fn_abi.args {
        let arg_ty = match &arg.mode {
            PassMode::Ignore => continue,
            PassMode::Direct(_) => cx.immediate_obs_type(arg.layout),
            PassMode::Pair(_, _) => {
                arg_tys.push(scalar_pair_element_obs_type(cx, arg.layout, 0, true));
                arg_tys.push(scalar_pair_element_obs_type(cx, arg.layout, 1, true));
                continue;
            }
            PassMode::Cast { pad_i32_count, cast } => {
                for _ in 0..*pad_i32_count {
                    arg_tys.push(cx.type_i32());
                }
                cast_target_obs_type(cx, cast)
            }
            PassMode::Indirect { address_space, .. } => {
                cx.type_ptr_in(address_space.map_or(0, |a| a.0))
            }
        };
        arg_tys.push(arg_ty);
    }

    cx.type_func(&arg_tys, ret_ty, fn_abi.c_variadic)
}

/// Converts a single register class into an [`ObsType`].
pub fn reg_obs_type<'tcx>(cx: &CodegenCx<'tcx>, reg: &Reg) -> ObsType {
    match reg.kind {
        RegKind::Integer => cx.type_ix(reg.size.bits()),
        RegKind::Float => match reg.size.bits() {
            16 => cx.type_f16(),
            32 => cx.type_f32(),
            64 => cx.type_f64(),
            80 => cx.type_x87_f80(),
            128 => cx.type_f128(),
            other => panic!("unsupported float reg bit width {other}"),
        },
        RegKind::Vector { hint_vector_elem } => {
            let dl = &cx.tcx.data_layout;
            let elem_ty = match hint_vector_elem {
                Primitive::Int(int, _) => cx.type_ix(int.size().bits()),
                Primitive::Float(vsc_abi::Float::F16) => cx.type_f16(),
                Primitive::Float(vsc_abi::Float::F32) => cx.type_f32(),
                Primitive::Float(vsc_abi::Float::F64) => cx.type_f64(),
                Primitive::Float(vsc_abi::Float::X87F80) => cx.type_x87_f80(),
                Primitive::Float(vsc_abi::Float::F128) => cx.type_f128(),
                Primitive::Pointer(a) => cx.type_ptr_in(a.0),
            };
            let elem_bytes = hint_vector_elem.size(dl).bytes().max(1);
            cx.type_vector(elem_ty, reg.size.bytes() / elem_bytes)
        }
    }
}

/// Converts a target ABI `CastTarget` into its corresponding [`ObsType`].
pub fn cast_target_obs_type<'tcx>(cx: &CodegenCx<'tcx>, cast: &CastTarget) -> ObsType {
    let rest_unit = reg_obs_type(cx, &cast.rest.unit);
    let rest_count = if cast.rest.total == Size::ZERO {
        0
    } else {
        cast.rest.total.bytes().div_ceil(cast.rest.unit.size.bytes())
    };

    if cast.prefix.is_empty() {
        if rest_count <= 1 {
            return rest_unit;
        }
        if rest_count == 2 {
            return ObsType::Pair(rest_unit.as_ir_type(), rest_unit.as_ir_type());
        }
        return cx.type_array(rest_unit, rest_count);
    }

    let mut fields = Vec::with_capacity(cast.prefix.len() + rest_count as usize);
    for reg in &cast.prefix {
        fields.push(reg_obs_type(cx, reg));
    }
    for _ in 0..rest_count {
        fields.push(rest_unit);
    }
    cx.type_struct(&fields, false)
}
