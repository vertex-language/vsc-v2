//! x86_64 System V ABI (`rustc_target::callconv::x86_64`).

use vsc_abi::{
    BackendRepr, Float, HasDataLayout, Primitive, Reg, RegKind, Size, TyAbiInterface, TyAndLayout,
    Variants,
};

use super::{ArgAbi, CastTarget, FnAbi, Uniform};
use crate::spec::HasTargetSpec;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Class {
    Int,
    Sse,
    SseUp,
    X87,
    X87Up,
}

impl Class {
    fn upper_half(self) -> Class {
        match self {
            Class::Sse | Class::SseUp => Class::SseUp,
            Class::X87 | Class::X87Up => Class::X87Up,
            Class::Int => Class::Int,
        }
    }

    fn merge(self, other: Self) -> Result<Self, Memory> {
        match (self, other) {
            (Class::X87 | Class::X87Up, Class::Sse | Class::SseUp)
            | (Class::Sse | Class::SseUp, Class::X87 | Class::X87Up) => Err(Memory),
            _ => Ok(Ord::min(self, other)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Memory;

const MAX_EIGHTBYTES: usize = 512 / 64;

fn classify_arg<'a, Ty, C>(
    cx: &C,
    arg: &ArgAbi<'a, Ty>,
) -> Result<[Option<Class>; MAX_EIGHTBYTES], Memory>
where
    Ty: TyAbiInterface<'a, C> + Copy,
    C: HasDataLayout,
{
    fn classify<'a, Ty, C>(
        cx: &C,
        layout: TyAndLayout<'a, Ty>,
        cls: &mut [Option<Class>],
        off: Size,
    ) -> Result<(), Memory>
    where
        Ty: TyAbiInterface<'a, C> + Copy,
        C: HasDataLayout,
    {
        if !off.is_aligned(layout.align) {
            if !layout.is_zst() {
                return Err(Memory);
            }
            return Ok(());
        }

        let mut c = match layout.backend_repr {
            BackendRepr::Scalar(scalar) => match scalar.primitive() {
                Primitive::Int(..) | Primitive::Pointer(_) => Class::Int,
                Primitive::Float(Float::X87F80) => Class::X87,
                Primitive::Float(_) => Class::Sse,
            },
            BackendRepr::SimdVector { .. } => Class::Sse,
            BackendRepr::ScalarPair { .. } | BackendRepr::Memory { .. } => {
                for i in 0..layout.fields.count() {
                    let field_off = off + layout.fields.offset(i);
                    classify(cx, layout.field(cx, i), cls, field_off)?;
                }
                if let Variants::Multiple { variants, .. } = &layout.variants {
                    for variant_idx in variants.indices() {
                        classify(cx, layout.for_variant(cx, variant_idx), cls, off)?;
                    }
                }
                return Ok(());
            }
        };

        let first = (off.bytes() / 8) as usize;
        let last = ((off.bytes() + layout.size.bytes().saturating_sub(1)) / 8) as usize;
        for slot in &mut cls[first..=last] {
            *slot = Some(match *slot {
                Some(old) => old.merge(c)?,
                None => c,
            });
            c = c.upper_half();
        }
        Ok(())
    }

    let n = arg.layout.size.bytes().div_ceil(8) as usize;
    if n == 0 || n > MAX_EIGHTBYTES {
        return Err(Memory);
    }
    if arg.layout.size.bytes() > 16 && arg.layout.fields.count() > 0 && !matches!(arg.layout.backend_repr, BackendRepr::SimdVector { .. }) {
        return Err(Memory);
    }

    let mut cls = [None; MAX_EIGHTBYTES];
    classify(cx, arg.layout, &mut cls[..n], Size::ZERO)?;
    Ok(cls)
}

fn reg_component(cls: &[Option<Class>], i: &mut usize, size: Size) -> Option<Reg> {
    if *i >= cls.len() {
        return None;
    }
    let c = cls[*i]?;
    *i += 1;
    match c {
        Class::Int => Some(if size.bytes() == 1 {
            Reg::i8()
        } else if size.bytes() == 2 {
            Reg::i16()
        } else if size.bytes() <= 4 {
            Reg::i32()
        } else {
            Reg::i64()
        }),
        Class::Sse => {
            let mut vec_bytes = 8u64;
            while *i < cls.len() && cls[*i] == Some(Class::SseUp) {
                *i += 1;
                vec_bytes += 8;
            }
            if vec_bytes == 8 {
                if size.bytes() <= 4 { Some(Reg::f32()) } else { Some(Reg::f64()) }
            } else {
                Some(Reg { kind: RegKind::Vector { hint_vector_elem: Primitive::Float(Float::F64) }, size: Size::from_bytes(vec_bytes) })
            }
        }
        Class::SseUp | Class::X87 | Class::X87Up => Some(Reg::i64()),
    }
}

fn cast_target(cls: &[Option<Class>], size: Size) -> CastTarget {
    let mut i = 0;
    let total = size.bytes();
    let lo_size = Size::from_bytes(total.min(8));
    let lo = reg_component(cls, &mut i, lo_size).unwrap_or_else(Reg::i64);
    if i < cls.len() && cls[i].is_some() {
        let hi_size = Size::from_bytes(total.saturating_sub(8).max(1));
        let hi = reg_component(cls, &mut i, hi_size).unwrap_or_else(Reg::i64);
        CastTarget::pair(lo, hi)
    } else {
        CastTarget::from(Uniform::new(lo, size))
    }
}

const MAX_INT_REGS: usize = 6;
const MAX_SSE_REGS: usize = 8;

pub(crate) fn compute_abi_info<'a, Ty, C>(cx: &C, fn_abi: &mut FnAbi<'a, Ty>)
where
    Ty: TyAbiInterface<'a, C> + Copy,
    C: HasDataLayout + HasTargetSpec,
{
    let mut int_regs = MAX_INT_REGS;
    let mut sse_regs = MAX_SSE_REGS;

    let mut handle = |arg: &mut ArgAbi<'a, Ty>, is_arg: bool| {
        if !arg.layout.is_sized() {
            return;
        }
        if is_arg && arg.layout.pass_indirectly_in_non_rustic_abis(cx) {
            int_regs = int_regs.saturating_sub(1);
            arg.make_indirect();
            return;
        }
        if is_arg && arg.layout.is_x87_f80(cx) {
            arg.cast_to(Reg::x87_f80());
            return;
        }
        let mut cls_or_mem = classify_arg(cx, arg);

        if is_arg
            && let Ok(cls) = cls_or_mem
        {
            if cls.contains(&Some(Class::X87)) {
                cls_or_mem = Err(Memory);
            } else {
                let mut need_int = 0usize;
                let mut need_sse = 0usize;
                for c in cls {
                    match c {
                        Some(Class::Int) => need_int += 1,
                        Some(Class::Sse) => need_sse += 1,
                        _ => {}
                    }
                }
                match (int_regs.checked_sub(need_int), sse_regs.checked_sub(need_sse)) {
                    (Some(li), Some(ls)) => {
                        int_regs = li;
                        sse_regs = ls;
                    }
                    _ => {
                        if arg.layout.is_aggregate() {
                            cls_or_mem = Err(Memory);
                        }
                    }
                }
            }
        }

        match cls_or_mem {
            Err(Memory) => {
                if is_arg {
                    arg.pass_by_stack_offset(None);
                } else {
                    arg.make_indirect();
                    int_regs = int_regs.saturating_sub(1);
                }
            }
            Ok(ref cls) => {
                if arg.layout.is_aggregate() {
                    let size = arg.layout.size;
                    arg.cast_to(cast_target(cls, size));
                } else if is_arg || cx.target_spec().is_like_darwin {
                    arg.extend_integer_width_to(32);
                }
                if !is_arg && arg.layout.backend_repr.is_bool() {
                    arg.extend_integer_width_to(8);
                }
            }
        }
    };

    if !fn_abi.ret.is_ignore() {
        handle(&mut fn_abi.ret, false);
    }
    for arg in fn_abi.args.iter_mut() {
        if !arg.is_ignore() {
            handle(arg, true);
        }
    }
}
