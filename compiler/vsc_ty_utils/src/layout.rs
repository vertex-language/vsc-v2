//! Layout calculation (`rustc_ty_utils::layout`).

use vsc_ast::token::LitKind;
use vsc_hir::def::DefKind;
use vsc_hir::{self as hir, LangItem, UnOp};
use vsc_index::{Idx, IndexVec};
use vsc_middle::ty::layout::{
    AddressSpace, Align, FieldIdx, HasDataLayout, Integer, IntegerExt, LayoutCalculator,
    LayoutCalculatorError, LayoutCx, LayoutError, LayoutOf, Primitive, ReprOptions, Scalar,
    TyAndLayout, VariantIdx, WrappingRange, float_to_abi,
};
use vsc_middle::ty::{AdtKind, ConstKind, Ty, TyCtxt, TyKind};
use vsc_span::def_id::DefId;

fn map_error<'tcx>(
    ty: Ty<'tcx>,
    err: LayoutCalculatorError<vsc_middle::ty::layout::Layout<'tcx>>,
) -> LayoutError<'tcx> {
    match err {
        LayoutCalculatorError::SizeOverflow => LayoutError::SizeOverflow(ty),
        LayoutCalculatorError::UnexpectedUnsized(_)
        | LayoutCalculatorError::EmptyUnion
        | LayoutCalculatorError::ReprConflict => LayoutError::Unknown(ty),
        LayoutCalculatorError::ZeroLengthSimdType
        | LayoutCalculatorError::OversizedSimdType { .. }
        | LayoutCalculatorError::NonPrimitiveSimdType(_) => LayoutError::InvalidSimd(ty),
    }
}

fn eval_int_lit_text(text: &str) -> Option<i128> {
    let clean: String = text.chars().filter(|&c| c != '_').collect();
    if let Some(hex) = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")) {
        return i128::from_str_radix(hex, 16).ok();
    }
    if let Some(bin) = clean.strip_prefix("0b").or_else(|| clean.strip_prefix("0B")) {
        return i128::from_str_radix(bin, 2).ok();
    }
    if let Some(oct) = clean.strip_prefix("0o").or_else(|| clean.strip_prefix("0O")) {
        return i128::from_str_radix(oct, 8).ok();
    }
    clean.parse().ok()
}

fn eval_hir_const_int(tcx: TyCtxt<'_>, expr: &hir::Expr) -> Option<i128> {
    match &expr.kind {
        hir::ExprKind::Lit(lit) if lit.kind == LitKind::Integer => {
            eval_int_lit_text(lit.symbol.as_str())
        }
        hir::ExprKind::Unary(UnOp::Neg, inner) => eval_hir_const_int(tcx, inner).map(|v| -v),
        hir::ExprKind::Unary(UnOp::Not, inner) => eval_hir_const_int(tcx, inner).map(|v| !v),
        hir::ExprKind::Cast(inner, _) => eval_hir_const_int(tcx, inner),
        hir::ExprKind::Binary(op, lhs, rhs) => {
            let l = eval_hir_const_int(tcx, lhs)?;
            let r = eval_hir_const_int(tcx, rhs)?;
            use vsc_ast::BinOpKind::*;
            match op.node {
                Add => l.checked_add(r),
                Sub => l.checked_sub(r),
                Mul => l.checked_mul(r),
                Div => l.checked_div(r),
                Rem => l.checked_rem(r),
                BitAnd => Some(l & r),
                BitOr => Some(l | r),
                BitXor => Some(l ^ r),
                Shl => u32::try_from(r).ok().and_then(|s| l.checked_shl(s)),
                Shr => u32::try_from(r).ok().and_then(|s| l.checked_shr(s)),
                _ => None,
            }
        }
        hir::ExprKind::Block(b, _) => b.expr.as_deref().and_then(|e| eval_hir_const_int(tcx, e)),
        hir::ExprKind::Path(hir::QPath::Resolved(_, p)) => {
            if let hir::def::Res::Def(DefKind::Const | DefKind::AssocConst, def_id) = p.res
                && let Some(local) = def_id.as_local()
                && let Some(body_id) = tcx.body_owned_by(local)
            {
                return eval_hir_const_int(tcx, &tcx.hir_crate.body(body_id).value);
            }
            None
        }
        hir::ExprKind::ConstBlock(cb) => {
            eval_hir_const_int(tcx, &tcx.hir_crate.body(cb.body).value)
        }
        _ => None,
    }
}

fn eval_const_def_int(tcx: TyCtxt<'_>, def_id: DefId) -> Option<i128> {
    let local = def_id.as_local()?;
    let body_id = match tcx.hir_node(local)? {
        vsc_middle::hir::map::Node::AnonConst(ac) => ac.body,
        _ => tcx.body_owned_by(local)?,
    };
    eval_hir_const_int(tcx, &tcx.hir_crate.body(body_id).value)
}

fn scalar_layout<'tcx>(
    tcx: TyCtxt<'tcx>,
    ty: Ty<'tcx>,
    scalar: Scalar,
) -> TyAndLayout<'tcx> {
    let cx = LayoutCx::new(tcx);
    let data = vsc_abi::LayoutData::scalar(&cx, scalar);
    TyAndLayout { ty, layout: tcx.alloc_layout(data) }
}

fn pointer_valid_range(cx: &LayoutCx<'_>, non_null: bool) -> WrappingRange {
    let dl = cx.data_layout();
    let max = dl.pointer_size().unsigned_int_max();
    if non_null {
        WrappingRange { start: 1, end: max }
    } else {
        WrappingRange { start: 0, end: max }
    }
}

pub(crate) fn layout_of<'tcx>(
    tcx: TyCtxt<'tcx>,
    ty: Ty<'tcx>,
) -> Result<TyAndLayout<'tcx>, LayoutError<'tcx>> {
    let ty = tcx.erase_regions(ty);
    let cx = LayoutCx::new(tcx);
    let dl = cx.data_layout();

    let alloc = |data| Ok(TyAndLayout { ty, layout: tcx.alloc_layout(data) });

    match *ty.kind() {
        TyKind::Error => Err(LayoutError::ReferencesError),
        TyKind::Param(_) | TyKind::Infer(_) => Err(LayoutError::TooGeneric(ty)),
        TyKind::Alias(alias) => {
            let infcx = vsc_infer::InferCtxt::new(tcx);
            let param_env = vsc_trait_selection::ParamEnv::default();
            let selcx = vsc_trait_selection::SelectionCtxt::new(&infcx, &param_env);
            if let Some((norm, _)) = selcx.normalize_projection(alias, vsc_span::DUMMY_SP) {
                let norm = tcx.reveal_opaque(tcx.erase_regions(infcx.resolve_vars_if_possible(norm)));
                if norm != ty && norm != tcx.types.error {
                    return cx.layout_of(norm);
                }
            }
            Err(LayoutError::TooGeneric(ty))
        }

        TyKind::Never => alloc(vsc_abi::LayoutData::never_type(&cx)),

        TyKind::Bool => Ok(scalar_layout(
            tcx,
            ty,
            Scalar::Initialized {
                value: Primitive::Int(Integer::I8, false),
                valid_range: WrappingRange { start: 0, end: 1 },
            },
        )),

        TyKind::Char => Ok(scalar_layout(
            tcx,
            ty,
            Scalar::Initialized {
                value: Primitive::Int(Integer::I32, false),
                valid_range: WrappingRange { start: 0, end: 0x10_ffff },
            },
        )),

        TyKind::Int(i) => {
            let int = Integer::from_int_ty(&cx, i);
            Ok(scalar_layout(
                tcx,
                ty,
                Scalar::Initialized {
                    value: Primitive::Int(int, true),
                    valid_range: WrappingRange::full(int.size()),
                },
            ))
        }

        TyKind::Uint(u) => {
            let int = Integer::from_uint_ty(&cx, u);
            Ok(scalar_layout(
                tcx,
                ty,
                Scalar::Initialized {
                    value: Primitive::Int(int, false),
                    valid_range: WrappingRange::full(int.size()),
                },
            ))
        }

        TyKind::Float(f) => {
            let fl = float_to_abi(f);
            Ok(scalar_layout(
                tcx,
                ty,
                Scalar::Initialized {
                    value: Primitive::Float(fl),
                    valid_range: WrappingRange::full(fl.size()),
                },
            ))
        }

        TyKind::FnDef(..) => alloc(vsc_abi::LayoutData::unit(&cx, true)),

        TyKind::FnPtr(_) => {
            let valid_range = pointer_valid_range(&cx, true);
            Ok(scalar_layout(
                tcx,
                ty,
                Scalar::Initialized {
                    value: Primitive::Pointer(AddressSpace::DATA),
                    valid_range,
                },
            ))
        }

        TyKind::Str => {
            let elem = vsc_abi::LayoutData::scalar(
                &cx,
                Scalar::Initialized {
                    value: Primitive::Int(Integer::I8, false),
                    valid_range: WrappingRange::full(Integer::I8.size()),
                },
            );
            alloc(vsc_abi::LayoutData::array_like(&cx, &elem, None))
        }

        TyKind::Slice(elem) => {
            let elem_layout = cx.layout_of(elem)?;
            alloc(vsc_abi::LayoutData::array_like(&cx, elem_layout.layout.0, None))
        }

        TyKind::Array(elem, len_const) => {
            let elem_layout = cx.layout_of(elem)?;
            let count = match *len_const.kind() {
                ConstKind::Value(_, n) => u64::try_from(n).map_err(|_| LayoutError::SizeOverflow(ty))?,
                ConstKind::Unevaluated(def_id, _) => {
                    let n = eval_const_def_int(tcx, def_id).ok_or(LayoutError::Unknown(ty))?;
                    u64::try_from(n).map_err(|_| LayoutError::SizeOverflow(ty))?
                }
                ConstKind::Param(_) => return Err(LayoutError::TooGeneric(ty)),
                ConstKind::Error => return Err(LayoutError::ReferencesError),
            };
            let data = vsc_abi::LayoutData::array_like(&cx, elem_layout.layout.0, Some(count));
            if elem_layout.size.bytes().checked_mul(count).is_none() {
                return Err(LayoutError::SizeOverflow(ty));
            }
            alloc(data)
        }

        TyKind::RawPtr(pointee, _) | TyKind::Ref(_, pointee, _) => {
            let non_null = matches!(ty.kind(), TyKind::Ref(..));
            let ptr_scalar = Scalar::Initialized {
                value: Primitive::Pointer(AddressSpace::DATA),
                valid_range: pointer_valid_range(&cx, non_null),
            };
            match *pointee.kind() {
                TyKind::Str | TyKind::Slice(_) | TyKind::Dynamic(..) => {
                    let meta_scalar = if matches!(pointee.kind(), TyKind::Dynamic(..)) {
                        Scalar::Initialized {
                            value: Primitive::Pointer(AddressSpace::DATA),
                            valid_range: pointer_valid_range(&cx, true),
                        }
                    } else {
                        let ptr_int = dl.ptr_sized_integer();
                        Scalar::Initialized {
                            value: Primitive::Int(ptr_int, false),
                            valid_range: WrappingRange::full(ptr_int.size()),
                        }
                    };
                    alloc(vsc_abi::LayoutData::scalar_pair(&cx, ptr_scalar, meta_scalar))
                }
                _ => Ok(scalar_layout(tcx, ty, ptr_scalar)),
            }
        }

        TyKind::Dynamic(..) | TyKind::Foreign(_) => {
            let mut data = vsc_abi::LayoutData::unit(&cx, false);
            data.align = Align::ONE;
            data.unadjusted_abi_align = Align::ONE;
            alloc(data)
        }

        TyKind::Tuple(tys) => {
            let mut fields = IndexVec::with_capacity(tys.len());
            for &field_ty in tys {
                fields.push(cx.layout_of(field_ty)?.layout);
            }
            let variants = IndexVec::from_raw(vec![fields]);
            let calc = LayoutCalculator::new(cx);
            let data = calc
                .layout_of_struct_or_enum(
                    &ReprOptions::default(),
                    &variants,
                    false,
                    false,
                    |_, _| unreachable!(),
                    std::iter::empty(),
                    true,
                )
                .map_err(|e| map_error(ty, e))?;
            alloc(data)
        }

        TyKind::Opaque(..) => {
            let revealed = tcx.reveal_opaque(ty);
            if revealed == ty {
                return Err(LayoutError::Unknown(ty));
            }
            cx.layout_of(revealed)
        }

        TyKind::Coroutine(def_id, args) => {
            let coro = tcx.coroutine_layout(def_id);
            let parent_args = if args.len() > 1 { &args[..args.len() - 1] } else { &[] };
            let mut variants: IndexVec<VariantIdx, IndexVec<FieldIdx, _>> =
                IndexVec::with_capacity(coro.variant_fields.len());
            for vfields in &coro.variant_fields {
                let mut fields = IndexVec::with_capacity(vfields.len());
                for &raw_ty in vfields {
                    let fty = tcx.reveal_opaque(tcx.erase_regions(tcx.instantiate(raw_ty, parent_args)));
                    fields.push(cx.layout_of(fty)?.layout);
                }
                variants.push(fields);
            }
            let repr = ReprOptions::default();
            let discrs = (0..coro.variant_fields.len()).map(|i| (VariantIdx::new(i), i as u128));
            let calc = LayoutCalculator::new(cx);
            let data = calc
                .layout_of_struct_or_enum(
                    &repr,
                    &variants,
                    true,
                    true,
                    |min_neg, max_pos| {
                        Integer::discr_range_of_repr(dl, &repr, min_neg.start, max_pos.end)
                    },
                    discrs,
                    true,
                )
                .map_err(|e| map_error(ty, e))?;
            alloc(data)
        }

        TyKind::Closure(def_id, _) => {
            let upvars = tcx.closure_upvars(def_id);
            let mut fields = IndexVec::with_capacity(upvars.len());
            for &upvar_ty in upvars {
                fields.push(cx.layout_of(upvar_ty)?.layout);
            }
            let variants = IndexVec::from_raw(vec![fields]);
            let calc = LayoutCalculator::new(cx);
            let data = calc
                .layout_of_struct_or_enum(
                    &ReprOptions::default(),
                    &variants,
                    false,
                    false,
                    |_, _| unreachable!(),
                    std::iter::empty(),
                    true,
                )
                .map_err(|e| map_error(ty, e))?;
            alloc(data)
        }

        TyKind::Adt(adt_def, args) => {
            // `Class<T>` is a heap reference-counted pointer to `T`.
            if tcx.is_lang_item(adt_def.did(), LangItem::Class) && args.len() == 1 {
                let valid_range = pointer_valid_range(&cx, true);
                return Ok(scalar_layout(
                    tcx,
                    ty,
                    Scalar::Initialized {
                        value: Primitive::Pointer(AddressSpace::DATA),
                        valid_range,
                    },
                ));
            }

            // `Func<F>` is a fat pair `(fn_ptr, env_ptr)`.
            if tcx.is_lang_item(adt_def.did(), LangItem::Func) && args.len() == 1 {
                let fn_ptr = Scalar::Initialized {
                    value: Primitive::Pointer(AddressSpace::DATA),
                    valid_range: pointer_valid_range(&cx, true),
                };
                let env_ptr = Scalar::Initialized {
                    value: Primitive::Pointer(AddressSpace::DATA),
                    valid_range: pointer_valid_range(&cx, false),
                };
                return alloc(vsc_abi::LayoutData::scalar_pair(&cx, fn_ptr, env_ptr));
            }

            let repr = adt_def.repr(tcx);
            let is_special_no_niche = tcx.is_lang_item(adt_def.did(), LangItem::UnsafeCell);

            if adt_def.kind() == AdtKind::Enum && adt_def.variants().is_empty() {
                return alloc(vsc_abi::LayoutData::never_type(&cx));
            }

            let mut variants: IndexVec<VariantIdx, IndexVec<FieldIdx, _>> =
                IndexVec::with_capacity(adt_def.variants().len());
            for variant in adt_def.variants() {
                let mut fields = IndexVec::with_capacity(variant.fields.len());
                for field in &variant.fields {
                    let field_ty = field.ty(tcx, args);
                    fields.push(cx.layout_of(field_ty)?.layout);
                }
                variants.push(fields);
            }

            let calc = LayoutCalculator::new(cx);
            if adt_def.kind() == AdtKind::Union {
                let data = calc.layout_of_union(&repr, &variants).map_err(|e| map_error(ty, e))?;
                return alloc(data);
            }

            let is_enum = adt_def.kind() == AdtKind::Enum;
            let mut discrs = Vec::with_capacity(adt_def.variants().len());
            if is_enum {
                let mut next_discr: i128 = 0;
                for (idx, variant) in adt_def.variants().iter().enumerate() {
                    if let Some(discr_def) = variant.discr
                        && let Some(explicit) = eval_const_def_int(tcx, discr_def)
                    {
                        next_discr = explicit;
                    }
                    let val_u128 = next_discr as u128;
                    discrs.push((VariantIdx::new(idx), val_u128));
                    next_discr = next_discr.wrapping_add(1);
                }
            }

            let data = calc
                .layout_of_struct_or_enum(
                    &repr,
                    &variants,
                    is_enum,
                    is_special_no_niche,
                    |min_neg, max_pos| {
                        Integer::discr_range_of_repr(dl, &repr, min_neg.start, max_pos.end)
                    },
                    discrs.into_iter(),
                    true,
                )
                .map_err(|e| map_error(ty, e))?;
            alloc(data)
        }
    }
}
