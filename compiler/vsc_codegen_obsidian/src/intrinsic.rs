//! Intrinsic, built-in primitive operator, and lang-item call lowering (`vsc_codegen_obsidian::intrinsic`).

use vsc_codegen_ssa::mir::{OperandRef, OperandValue, PlaceRef};
use vsc_hir::LangItem;
use vsc_middle::mir::{BinOp, UnOp};
use vsc_middle::ty::{AdtDef, GenericArgsRef, Ty, TyKind};
use vsc_span::Symbol;
use vsc_span::def_id::DefId;
use vsc_trait_selection::{OpTrait, fn_trait_kind, op_trait};

use crate::builder::Builder;
use crate::callee::codegen_fn_trait_call;
use crate::common::discriminant_value;
use crate::value::ObsVal;

/// Extracts the `@intrinsic("...")` name on `def_id`, if present.
pub fn intrinsic_name<'tcx>(
    cx: &crate::context::CodegenCx<'tcx>,
    def_id: DefId,
) -> Option<Symbol> {
    let local = def_id.as_local()?;
    cx.tcx.owner_attrs(local).iter().find_map(|a| {
        let vsc_ast::AttrKind::Normal(n) = &a.kind else {
            return None;
        };
        if a.name()?.as_str() != "intrinsic" {
            return None;
        }
        if let vsc_ast::AttrArgs::Delimited(d) = &n.args {
            let text = vsc_ast_pretty::pprust::tts_to_string(&d.tokens);
            return Some(Symbol::intern(text.trim().trim_matches('"')));
        }
        None
    })
}

/// Attempts to lower an `InstanceKind::Intrinsic(def_id)` or intercepted lang-item call directly.
pub fn try_codegen_intrinsic<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    def_id: DefId,
    fn_args: GenericArgsRef<'tcx>,
    args: &[OperandRef<'tcx, ObsVal>],
    dest: PlaceRef<'tcx, ObsVal>,
) -> bool {
    let tcx = bx.cx.tcx;

    if tcx
        .opt_parent(def_id)
        .is_some_and(|p| fn_trait_kind(tcx, p).is_some())
        && args.len() == 2
    {
        codegen_fn_trait_call(bx, args[0], args[1], dest);
        return true;
    }

    if let Some(parent) = tcx.opt_parent(def_id)
        && op_trait(tcx, parent).is_some()
    {
        return codegen_primitive_op_trait(bx, def_id, fn_args, args, dest);
    }

    if let Some(name) = intrinsic_name(bx.cx, def_id) {
        return codegen_named_intrinsic(bx, name.as_str(), fn_args, args, dest);
    }

    let li = tcx.lang_items();
    if Some(def_id) == li.get(LangItem::Panic)
        || Some(def_id) == li.get(LangItem::PanicFmt)
        || Some(def_id) == li.get(LangItem::AssertFailed)
    {
        bx.call_panic_abort("panicked at runtime");
        return true;
    }

    false
}

fn codegen_named_intrinsic<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    name: &str,
    fn_args: GenericArgsRef<'tcx>,
    args: &[OperandRef<'tcx, ObsVal>],
    dest: PlaceRef<'tcx, ObsVal>,
) -> bool {
    match name {
        "slice_len" => {
            if let Some(slice_op) = args.first()
                && let OperandValue::Pair(_, len) = slice_op.val
            {
                bx.store_operand(OperandValue::Immediate(len), dest);
                return true;
            }
            false
        }
        "transmute" => {
            if let Some(src) = args.first() {
                bx.codegen_transmute(*src, dest);
                return true;
            }
            false
        }
        "size_of" => {
            let ty = fn_args.iter().find_map(|a| match a {
                vsc_middle::ty::GenericArg::Type(t) => Some(*t),
                _ => None,
            });
            if let Some(ty) = ty {
                let bytes = bx.cx.layout_of(ty).size.bytes();
                bx.store_operand(OperandValue::Immediate(bx.cx.const_usize(bytes)), dest);
                return true;
            }
            false
        }
        "align_of" => {
            let ty = fn_args.iter().find_map(|a| match a {
                vsc_middle::ty::GenericArg::Type(t) => Some(*t),
                _ => None,
            });
            if let Some(ty) = ty {
                let bytes = bx.cx.layout_of(ty).align.bytes();
                bx.store_operand(OperandValue::Immediate(bx.cx.const_usize(bytes)), dest);
                return true;
            }
            false
        }
        "needs_drop" => {
            let ty = fn_args.iter().find_map(|a| match a {
                vsc_middle::ty::GenericArg::Type(t) => Some(*t),
                _ => None,
            });
            if let Some(ty) = ty {
                let needs = bx.cx.tcx.needs_drop_raw(ty);
                bx.store_operand(OperandValue::Immediate(bx.cx.const_bool(needs)), dest);
                return true;
            }
            false
        }
        "black_box" => {
            if let Some(src) = args.first() {
                bx.store_operand(src.val, dest);
                return true;
            }
            false
        }
        "unreachable" => {
            bx.unreachable();
            true
        }
        "sqrtf32" | "sqrtf64" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.fsqrt(v);
                bx.store_operand(OperandValue::Immediate(res), dest);
                return true;
            }
            false
        }
        "fabsf32" | "fabsf64" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.fabs(v);
                bx.store_operand(OperandValue::Immediate(res), dest);
                return true;
            }
            false
        }
        "floorf32" | "floorf64" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.ffloor(v);
                bx.store_operand(OperandValue::Immediate(res), dest);
                return true;
            }
            false
        }
        "ceilf32" | "ceilf64" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.fceil(v);
                bx.store_operand(OperandValue::Immediate(res), dest);
                return true;
            }
            false
        }
        "truncf32" | "truncf64" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.ftrunc(v);
                bx.store_operand(OperandValue::Immediate(res), dest);
                return true;
            }
            false
        }
        "roundevenf32" | "roundevenf64" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.fnearest(v);
                bx.store_operand(OperandValue::Immediate(res), dest);
                return true;
            }
            false
        }
        "ctlz" | "ctlz_nonzero" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.clz(v);
                let out = bx.intcast(res, bx.cx.immediate_obs_type(dest.layout), false);
                bx.store_operand(OperandValue::Immediate(out), dest);
                return true;
            }
            false
        }
        "cttz" | "cttz_nonzero" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.ctz(v);
                let out = bx.intcast(res, bx.cx.immediate_obs_type(dest.layout), false);
                bx.store_operand(OperandValue::Immediate(out), dest);
                return true;
            }
            false
        }
        "ctpop" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.popcnt(v);
                let out = bx.intcast(res, bx.cx.immediate_obs_type(dest.layout), false);
                bx.store_operand(OperandValue::Immediate(out), dest);
                return true;
            }
            false
        }
        "bswap" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.byte_swap(v);
                bx.store_operand(OperandValue::Immediate(res), dest);
                return true;
            }
            false
        }
        "bitreverse" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(v) = arg.val
            {
                let res = bx.bit_reverse(v);
                bx.store_operand(OperandValue::Immediate(res), dest);
                return true;
            }
            false
        }
        "rotate_left" => {
            if args.len() == 2
                && let (OperandValue::Immediate(v), OperandValue::Immediate(amt)) =
                    (args[0].val, args[1].val)
            {
                let res = bx.rotl(v, amt);
                bx.store_operand(OperandValue::Immediate(res), dest);
                return true;
            }
            false
        }
        "rotate_right" => {
            if args.len() == 2
                && let (OperandValue::Immediate(v), OperandValue::Immediate(amt)) =
                    (args[0].val, args[1].val)
            {
                let res = bx.rotr(v, amt);
                bx.store_operand(OperandValue::Immediate(res), dest);
                return true;
            }
            false
        }
        "copy_nonoverlapping" | "copy" => {
            if args.len() == 3
                && let (
                    OperandValue::Immediate(src),
                    OperandValue::Immediate(dst),
                    OperandValue::Immediate(count),
                ) = (args[0].val, args[1].val, args[2].val)
            {
                let elem_bytes = pointee_size_bytes(bx, args[0].layout.ty);
                let count_u = bx.intcast(count, bx.cx.type_isize(), false);
                let byte_len = if elem_bytes == 1 {
                    count_u
                } else {
                    bx.mul(count_u, bx.cx.const_usize(elem_bytes))
                };
                bx.memcpy_dyn(dst, src, byte_len, name == "copy");
                return true;
            }
            false
        }
        "write_bytes" => {
            if args.len() == 3
                && let (
                    OperandValue::Immediate(dst),
                    OperandValue::Immediate(byte_val),
                    OperandValue::Immediate(count),
                ) = (args[0].val, args[1].val, args[2].val)
            {
                let elem_bytes = pointee_size_bytes(bx, args[0].layout.ty);
                let count_u = bx.intcast(count, bx.cx.type_isize(), false);
                let byte_len = if elem_bytes == 1 {
                    count_u
                } else {
                    bx.mul(count_u, bx.cx.const_usize(elem_bytes))
                };
                bx.memset_dyn(dst, byte_val, byte_len);
                return true;
            }
            false
        }
        "volatile_load" => {
            if let Some(arg) = args.first()
                && let OperandValue::Immediate(ptr) = arg.val
            {
                let src = PlaceRef::new_sized(ptr, dest.layout);
                let val = bx.load_operand(src);
                bx.store_operand(val.val, dest);
                return true;
            }
            false
        }
        "volatile_store" => {
            if args.len() == 2
                && let OperandValue::Immediate(ptr) = args[0].val
            {
                let place = PlaceRef::new_sized(ptr, args[1].layout);
                bx.store_operand(args[1].val, place);
                return true;
            }
            false
        }
        "spin_loop" | "spin_hint" => {
            bx.spin_hint();
            true
        }
        "abort" => {
            bx.call_panic_abort("explicit abort");
            true
        }
        other if other.starts_with("atomic_") => codegen_atomic_intrinsic(bx, other, args, dest),
        _ => false,
    }
}

fn pointee_size_bytes<'a, 'tcx>(bx: &Builder<'a, 'tcx>, ptr_ty: Ty<'tcx>) -> u64 {
    match *ptr_ty.kind() {
        TyKind::RawPtr(inner, _) | TyKind::Ref(_, inner, _) => {
            bx.cx.layout_of(inner).size.bytes()
        }
        _ => 1,
    }
}

fn parse_mem_order(s: &str) -> obsidian::ir::MemOrder {
    match s {
        "relaxed" => obsidian::ir::MemOrder::Relaxed,
        "acquire" => obsidian::ir::MemOrder::Acquire,
        "release" => obsidian::ir::MemOrder::Release,
        "acqrel" => obsidian::ir::MemOrder::AcqRel,
        _ => obsidian::ir::MemOrder::SeqCst,
    }
}

fn codegen_atomic_intrinsic<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    name: &str,
    args: &[OperandRef<'tcx, ObsVal>],
    dest: PlaceRef<'tcx, ObsVal>,
) -> bool {
    let rest = &name["atomic_".len()..];
    if let Some(ord_str) = rest
        .strip_prefix("fence_")
        .or_else(|| rest.strip_prefix("singlethreadfence_"))
    {
        bx.mem_fence(parse_mem_order(ord_str));
        return true;
    }
    if let Some(ord_str) = rest.strip_prefix("load_") {
        if let Some(arg) = args.first()
            && let OperandValue::Immediate(ptr) = arg.val
        {
            let ty = bx.cx.layout_obs_type(dest.layout);
            let raw = bx.atomic_load(ty, parse_mem_order(ord_str), ptr);
            let imm = bx.from_immediate(raw, dest.layout);
            bx.store_operand(OperandValue::Immediate(imm), dest);
            return true;
        }
        return false;
    }
    if let Some(ord_str) = rest.strip_prefix("store_") {
        if args.len() == 2
            && let (OperandValue::Immediate(ptr), OperandValue::Immediate(val)) =
                (args[0].val, args[1].val)
        {
            let mem_val = bx.to_immediate(val, args[1].layout);
            bx.atomic_store(parse_mem_order(ord_str), ptr, mem_val);
            return true;
        }
        return false;
    }
    if let Some(ord_part) = rest
        .strip_prefix("cxchg_")
        .or_else(|| rest.strip_prefix("cxchgweak_"))
    {
        if args.len() == 3
            && let (
                OperandValue::Immediate(ptr),
                OperandValue::Immediate(exp),
                OperandValue::Immediate(des),
            ) = (args[0].val, args[1].val, args[2].val)
        {
            let succ_ord = ord_part.split('_').next().unwrap_or("seqcst");
            let (old, ok) = bx.atomic_cas(parse_mem_order(succ_ord), ptr, exp, des);
            let pair = if dest.layout.fields.count() >= 2
                && dest.layout.fields.offset(0) > dest.layout.fields.offset(1)
            {
                OperandValue::Pair(ok, old)
            } else {
                OperandValue::Pair(old, ok)
            };
            bx.store_operand(pair, dest);
            return true;
        }
        return false;
    }

    let rmw_table: &[(&str, obsidian::ir::AtomicRmwOp)] = &[
        ("xadd_", obsidian::ir::AtomicRmwOp::Add),
        ("xsub_", obsidian::ir::AtomicRmwOp::Sub),
        ("and_", obsidian::ir::AtomicRmwOp::And),
        ("or_", obsidian::ir::AtomicRmwOp::Or),
        ("xor_", obsidian::ir::AtomicRmwOp::Xor),
        ("nand_", obsidian::ir::AtomicRmwOp::Nand),
        ("xchg_", obsidian::ir::AtomicRmwOp::Xchg),
        ("max_", obsidian::ir::AtomicRmwOp::MaxS),
        ("min_", obsidian::ir::AtomicRmwOp::MinS),
        ("umax_", obsidian::ir::AtomicRmwOp::MaxU),
        ("umin_", obsidian::ir::AtomicRmwOp::MinU),
    ];
    for &(prefix, rmw_op) in rmw_table {
        if let Some(ord_str) = rest.strip_prefix(prefix) {
            if args.len() == 2
                && let (OperandValue::Immediate(ptr), OperandValue::Immediate(val)) =
                    (args[0].val, args[1].val)
            {
                let old = bx.atomic_rmw(rmw_op, parse_mem_order(ord_str), ptr, val);
                bx.store_operand(OperandValue::Immediate(old), dest);
                return true;
            }
            return false;
        }
    }
    false
}

fn codegen_primitive_op_trait<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    def_id: DefId,
    _fn_args: GenericArgsRef<'tcx>,
    args: &[OperandRef<'tcx, ObsVal>],
    dest: PlaceRef<'tcx, ObsVal>,
) -> bool {
    let tcx = bx.cx.tcx;
    let method = tcx.item_name(def_id);
    let name = method.as_str();
    let assign_binop = match name {
        "addAssign" | "add_assign" => Some(BinOp::Add),
        "subAssign" | "sub_assign" => Some(BinOp::Sub),
        "mulAssign" | "mul_assign" => Some(BinOp::Mul),
        "divAssign" | "div_assign" => Some(BinOp::Div),
        "remAssign" | "rem_assign" => Some(BinOp::Rem),
        "bitAndAssign" | "bitand_assign" => Some(BinOp::BitAnd),
        "bitOrAssign" | "bitor_assign" => Some(BinOp::BitOr),
        "bitXorAssign" | "bitxor_assign" => Some(BinOp::BitXor),
        "shlAssign" | "shl_assign" => Some(BinOp::Shl),
        "shrAssign" | "shr_assign" => Some(BinOp::Shr),
        _ => None,
    };

    if let Some(op) = assign_binop
        && args.len() == 2
        && let OperandValue::Immediate(ptr) = args[0].val
        && let TyKind::Ref(_, inner_ty, _) = *args[0].layout.ty.kind()
    {
        let place = PlaceRef::new_sized(ptr, bx.cx.layout_of(inner_ty));
        let cur = bx.load_operand(place);
        let res = bx.codegen_binary_op(op, cur, args[1], place.layout.ty);
        bx.store_operand(res, place);
        return true;
    }

    let Some(parent) = tcx.opt_parent(def_id) else {
        return false;
    };
    let Some(kind) = op_trait(tcx, parent) else {
        return false;
    };

    match kind {
        OpTrait::Arith | OpTrait::Bit | OpTrait::Shift if args.len() == 2 => {
            let op = match name {
                "add" => BinOp::Add,
                "sub" => BinOp::Sub,
                "mul" => BinOp::Mul,
                "div" => BinOp::Div,
                "rem" => BinOp::Rem,
                "bitAnd" | "bitand" => BinOp::BitAnd,
                "bitOr" | "bitor" => BinOp::BitOr,
                "bitXor" | "bitxor" => BinOp::BitXor,
                "shl" => BinOp::Shl,
                "shr" => BinOp::Shr,
                _ => return false,
            };
            let res = bx.codegen_binary_op(op, args[0], args[1], dest.layout.ty);
            bx.store_operand(res, dest);
            true
        }
        OpTrait::Neg | OpTrait::Not if args.len() == 1 => {
            let op = if kind == OpTrait::Neg {
                UnOp::Neg
            } else {
                UnOp::Not
            };
            let res = bx.codegen_unary_op(op, args[0], dest.layout.ty);
            bx.store_operand(res, dest);
            true
        }
        OpTrait::Cmp if args.len() == 2 => {
            let lhs = deref_scalar_arg(bx, args[0]);
            let rhs = deref_scalar_arg(bx, args[1]);
            match name {
                "eq" | "ne" | "lt" | "le" | "gt" | "ge" => {
                    let op = match name {
                        "eq" => BinOp::Eq,
                        "ne" => BinOp::Ne,
                        "lt" => BinOp::Lt,
                        "le" => BinOp::Le,
                        "gt" => BinOp::Gt,
                        _ => BinOp::Ge,
                    };
                    let res = bx.codegen_binary_op(op, lhs, rhs, dest.layout.ty);
                    bx.store_operand(res, dest);
                    true
                }
                "partialCmp" | "partial_cmp" | "cmp" => {
                    codegen_partial_cmp(bx, lhs, rhs, dest);
                    true
                }
                _ => false,
            }
        }
        _ => false,
    }
}

fn deref_scalar_arg<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    arg: OperandRef<'tcx, ObsVal>,
) -> OperandRef<'tcx, ObsVal> {
    if let TyKind::Ref(_, inner, _) = *arg.layout.ty.kind() {
        let inner_layout = bx.cx.layout_of(inner);
        if let OperandValue::Immediate(ptr) = arg.val
            && !inner_layout.is_unsized()
        {
            return bx.load_operand(PlaceRef::new_sized(ptr, inner_layout));
        }
    }
    arg
}

fn codegen_partial_cmp<'a, 'tcx>(
    bx: &mut Builder<'a, 'tcx>,
    lhs: OperandRef<'tcx, ObsVal>,
    rhs: OperandRef<'tcx, ObsVal>,
    dest: PlaceRef<'tcx, ObsVal>,
) {
    let tcx = bx.cx.tcx;
    let Some(ordering_def) = tcx.lang_items().get(LangItem::Ordering) else {
        bx.call_panic_abort("missing Ordering lang item");
        return;
    };
    let ord_adt: AdtDef<'tcx> = tcx.adt_def(ordering_def);
    let variant_discr = |vname: &str| -> u128 {
        let v = ord_adt
            .variants()
            .iter()
            .position(|v| v.name.as_str() == vname)
            .unwrap_or(0);
        discriminant_value(tcx, ord_adt, v) as u128
    };

    let lt_op = bx.codegen_binary_op(BinOp::Lt, lhs, rhs, tcx.types.bool);
    let gt_op = bx.codegen_binary_op(BinOp::Gt, lhs, rhs, tcx.types.bool);
    let eq_op = bx.codegen_binary_op(BinOp::Eq, lhs, rhs, tcx.types.bool);

    let (
        OperandValue::Immediate(is_lt),
        OperandValue::Immediate(is_gt),
        OperandValue::Immediate(is_eq),
    ) = (lt_op, gt_op, eq_op)
    else {
        return;
    };

    let dest_ty: Ty<'tcx> = dest.layout.ty;
    if let TyKind::Adt(adt, _) = *dest_ty.kind()
        && adt.did() == ordering_def
    {
        let lt_v = bx.cx.const_i8(variant_discr("Less") as i8);
        let eq_v = bx.cx.const_i8(variant_discr("Equal") as i8);
        let gt_v = bx.cx.const_i8(variant_discr("Greater") as i8);
        let sel = bx.select(is_lt, lt_v, bx.select(is_gt, gt_v, eq_v));
        bx.store(sel, dest.val.llval, dest.val.align);
        return;
    }

    let lt_val = variant_discr("Less");
    let eq_val = variant_discr("Equal");
    let gt_val = variant_discr("Greater");
    let ord_sel = bx.select(
        is_lt,
        bx.cx.const_u8(lt_val as u8),
        bx.select(
            is_gt,
            bx.cx.const_u8(gt_val as u8),
            bx.cx.const_u8(eq_val as u8),
        ),
    );
    let some_idx = tcx
        .lang_items()
        .get(LangItem::OptionSome)
        .and_then(|d| {
            if let TyKind::Adt(opt_adt, _) = *dest_ty.kind() {
                opt_adt.variants().iter().position(|v| v.def_id == d)
            } else {
                None
            }
        })
        .unwrap_or(1);
    let none_idx = 1usize.saturating_sub(some_idx);

    let valid = bx.or(is_lt, bx.or(is_gt, is_eq));
    let var_layout = dest
        .layout
        .for_variant(&bx.cx.layout_cx, vsc_abi::VariantIdx::from_usize(some_idx));
    if var_layout.fields.count() > 0 {
        let foff = var_layout.fields.offset(0).bytes();
        let fptr = bx.ptradd(dest.val.llval, foff);
        let falign =
            vsc_codegen_ssa::mir::align_for_offset(dest.val.align, var_layout.fields.offset(0));
        bx.store(ord_sel, fptr, falign);
    }

    // Store selected discriminant (`Some` when `valid`, `None` when unordered NaN) without splitting basic blocks:
    let tmp_some = bx.alloca_layout(dest.layout, "cmp_some_tmp");
    if var_layout.fields.count() > 0 {
        let foff = var_layout.fields.offset(0).bytes();
        let fptr = bx.ptradd(tmp_some.val.llval, foff);
        let falign =
            vsc_codegen_ssa::mir::align_for_offset(tmp_some.val.align, var_layout.fields.offset(0));
        bx.store(ord_sel, fptr, falign);
    }
    bx.codegen_set_discriminant(tmp_some, some_idx);

    let tmp_none = bx.alloca_layout(dest.layout, "cmp_none_tmp");
    bx.codegen_set_discriminant(tmp_none, none_idx);

    let src_ptr = bx.select(valid, tmp_some.val.llval, tmp_none.val.llval);
    bx.memcpy(
        dest.val.llval,
        dest.val.align,
        src_ptr,
        dest.val.align,
        dest.layout.size.bytes(),
    );
}
