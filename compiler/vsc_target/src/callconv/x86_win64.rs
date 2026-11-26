//! Windows x64 calling convention (`rustc_target::callconv::x86_win64`).

use vsc_abi::{BackendRepr, Integer, Primitive, Reg, Size, TyAbiInterface};

use super::{ArgAbi, FnAbi};
use crate::spec::{HasTargetSpec, RustcAbi};

pub(crate) fn compute_abi_info<'a, Ty, C: HasTargetSpec>(cx: &C, fn_abi: &mut FnAbi<'a, Ty>)
where
    Ty: TyAbiInterface<'a, C> + Copy,
{
    let fixup = |a: &mut ArgAbi<'_, Ty>, is_ret: bool| match a.layout.backend_repr {
        BackendRepr::Memory { sized: false } => {}
        BackendRepr::ScalarPair { .. } | BackendRepr::Memory { sized: true } => {
            match a.layout.size.bits() {
                8 => a.cast_to(Reg::i8()),
                16 => a.cast_to(Reg::i16()),
                32 => a.cast_to(Reg::i32()),
                64 => a.cast_to(Reg::i64()),
                _ => a.make_indirect(),
            }
        }
        BackendRepr::SimdVector { .. } => {}
        BackendRepr::Scalar(scalar) => {
            if is_ret && matches!(scalar.primitive(), Primitive::Int(Integer::I128, _)) {
                if cx.target_spec().rustc_abi != Some(RustcAbi::Softfloat) {
                    a.cast_to(Reg::opaque_vector(Size::from_bits(128)));
                }
            } else if a.layout.size.bytes() > 8 {
                a.make_indirect();
            } else {
                a.extend_integer_width_to(32);
            }
        }
    };

    if fn_abi.ret.is_ignore() {
        if fn_abi.ret.layout.is_repr_c() {
            fn_abi.ret.make_indirect_from_ignore();
        }
    } else {
        fixup(&mut fn_abi.ret, true);
    }

    for arg in fn_abi.args.iter_mut() {
        if arg.is_ignore() {
            if arg.layout.is_repr_c() {
                arg.make_indirect_from_ignore();
            }
            continue;
        }
        if arg.layout.pass_indirectly_in_non_rustic_abis(cx) {
            arg.make_indirect();
            continue;
        }
        fixup(arg, false);
    }
}
