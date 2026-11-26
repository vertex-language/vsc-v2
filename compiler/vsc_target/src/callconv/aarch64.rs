//! AArch64 C calling convention (`AAPCS` / `DarwinPCS` / `Win64`).

use std::iter;

use vsc_abi::{BackendRepr, HasDataLayout, Primitive, Reg, RegKind, TyAbiInterface};

use super::{ArgAbi, FnAbi, Uniform};
use crate::spec::{Arch, HasTargetSpec, RustcAbi, Target};

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum AbiKind {
    AAPCS,
    DarwinPCS,
    Win64,
}

fn is_homogeneous_aggregate<'a, Ty, C>(cx: &C, arg: &mut ArgAbi<'a, Ty>) -> Option<Uniform>
where
    Ty: TyAbiInterface<'a, C> + Copy,
    C: HasDataLayout + HasTargetSpec,
{
    arg.layout.homogeneous_aggregate(cx).ok().and_then(|ha| ha.unit()).and_then(|unit| {
        let size = arg.layout.size;
        if size > unit.size.checked_mul(4, cx.data_layout())? {
            return None;
        }
        let valid_unit = match unit.kind {
            RegKind::Integer => false,
            RegKind::Float => cx.target_spec().rustc_abi != Some(RustcAbi::Softfloat),
            RegKind::Vector { .. } => unit.size.bits() == 64 || unit.size.bits() == 128,
        };
        valid_unit.then_some(Uniform::consecutive(unit, size))
    })
}

fn softfloat_float_abi<Ty>(target: &Target, arg: &mut ArgAbi<'_, Ty>) {
    if target.rustc_abi != Some(RustcAbi::Softfloat) {
        return;
    }
    if let BackendRepr::Scalar(s) = arg.layout.backend_repr
        && let Primitive::Float(f) = s.primitive()
    {
        arg.cast_to(Reg { kind: RegKind::Integer, size: f.size() });
    } else if let BackendRepr::ScalarPair { a, b, .. } = arg.layout.backend_repr
        && (a.primitive().is_float() || b.primitive().is_float())
    {
        if arg.layout.size.bits() <= target.pointer_width as u64 {
            arg.cast_to(Reg { kind: RegKind::Integer, size: arg.layout.size });
        } else {
            arg.make_indirect();
        }
    }
}

fn classify_ret<'a, Ty, C>(cx: &C, ret: &mut ArgAbi<'a, Ty>, kind: AbiKind)
where
    Ty: TyAbiInterface<'a, C> + Copy,
    C: HasDataLayout + HasTargetSpec,
{
    if !ret.layout.is_sized() {
        return;
    }
    if !ret.layout.is_aggregate() {
        if kind == AbiKind::DarwinPCS {
            ret.extend_integer_width_to(32);
        }
        softfloat_float_abi(cx.target_spec(), ret);
        return;
    }
    if let Some(uniform) = is_homogeneous_aggregate(cx, ret) {
        ret.cast_to(uniform);
        return;
    }
    let size = ret.layout.size;
    if size.bits() <= 128 {
        ret.cast_to(Uniform::new(Reg::i64(), size));
    } else {
        ret.make_indirect();
    }
}

fn classify_arg<'a, Ty, C>(cx: &C, arg: &mut ArgAbi<'a, Ty>, kind: AbiKind)
where
    Ty: TyAbiInterface<'a, C> + Copy,
    C: HasDataLayout + HasTargetSpec,
{
    if !arg.layout.is_sized() {
        return;
    }
    if arg.layout.pass_indirectly_in_non_rustic_abis(cx) {
        arg.make_indirect();
        return;
    }
    if !arg.layout.is_aggregate() {
        if kind == AbiKind::DarwinPCS {
            arg.extend_integer_width_to(32);
        }
        softfloat_float_abi(cx.target_spec(), arg);
        return;
    }
    if let Some(uniform) = is_homogeneous_aggregate(cx, arg) {
        arg.cast_to(uniform);
        return;
    }
    let size = arg.layout.size;
    let align = if kind == AbiKind::AAPCS {
        arg.layout.unadjusted_abi_align
    } else {
        arg.layout.align
    };
    if size.bits() <= 128 {
        if align.bits() == 128 {
            arg.cast_to(Uniform::new(Reg::i128(), size));
        } else {
            arg.cast_to(Uniform::new(Reg::i64(), size));
        }
    } else {
        arg.make_indirect();
    }
}

pub(crate) fn compute_abi_info<'a, Ty, C>(cx: &C, fn_abi: &mut FnAbi<'a, Ty>, kind: AbiKind)
where
    Ty: TyAbiInterface<'a, C> + Copy,
    C: HasDataLayout + HasTargetSpec,
{
    if !fn_abi.ret.is_ignore() {
        classify_ret(cx, &mut fn_abi.ret, kind);
    }

    let c_variadic = fn_abi.c_variadic;
    let fixed_count = fn_abi.fixed_count as usize;
    let is_arm64ec = cx.target_spec().arch == Arch::Arm64EC;

    for (idx, arg) in fn_abi.args.iter_mut().enumerate() {
        if arg.is_ignore() {
            continue;
        }
        if is_arm64ec && c_variadic && idx >= fixed_count {
            let size = arg.layout.size.bytes();
            if size > 8 || !size.is_power_of_two() {
                arg.make_indirect();
                continue;
            }
        }
        classify_arg(cx, arg, kind);
    }
}

pub(crate) fn compute_rust_abi_info<'a, Ty, C>(cx: &C, fn_abi: &mut FnAbi<'a, Ty>)
where
    Ty: TyAbiInterface<'a, C> + Copy,
    C: HasDataLayout + HasTargetSpec,
{
    for arg in fn_abi.args.iter_mut().chain(iter::once(&mut fn_abi.ret)) {
        softfloat_float_abi(cx.target_spec(), arg);
    }
}
