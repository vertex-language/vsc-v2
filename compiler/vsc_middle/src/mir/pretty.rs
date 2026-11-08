//! Printing MIR as rustc's `-Z dump-mir` does, with Vertex's type names.

use std::fmt::Write;

use vsc_hir::def::PrimTy;
use vsc_index::Idx;

use super::*;
use crate::ty::{TyCtxt, TyKind};

impl<'tcx> TyCtxt<'tcx> {
    pub fn mir_to_string(self, body: &Body<'tcx>) -> String {
        let mut s = String::new();
        let args: Vec<String> =
            body.args_iter().map(|l| format!("{l:?}: {}", self.ty_to_string(body.local_decls[l].ty))).collect();
        let _ = writeln!(s, "fn {}({}) -> {} {{", self.def_path_str(body.source), args.join(", "), self.ty_to_string(body.return_ty()));
        for (name, local) in &body.var_debug_info {
            let _ = writeln!(s, "    debug {name} => {local:?};");
        }
        for (local, decl) in body.local_decls.iter_enumerated() {
            if local.index() >= 1 && local.index() <= body.arg_count {
                continue;
            }
            let m = if decl.mutability.is_mut() { "mut " } else { "" };
            let _ = writeln!(s, "    let {m}{local:?}: {};", self.ty_to_string(decl.ty));
        }
        for (bb, data) in body.basic_blocks.iter_enumerated() {
            let _ = writeln!(s, "\n    {bb:?}: {{");
            for st in &data.statements {
                match &st.kind {
                    StatementKind::Assign(a) => {
                        let _ = writeln!(s, "        {} = {};", self.place_str(&a.0), self.rvalue_str(&a.1));
                    }
                    StatementKind::SetDiscriminant { place, variant_index } => {
                        let _ = writeln!(s, "        discriminant({}) = {variant_index};", self.place_str(place));
                    }
                    StatementKind::Nop => {
                        let _ = writeln!(s, "        nop;");
                    }
                }
            }
            match &data.terminator {
                Some(t) => {
                    let _ = writeln!(s, "        {};", self.terminator_str(&t.kind));
                }
                None => {
                    let _ = writeln!(s, "        // no terminator");
                }
            }
            let _ = writeln!(s, "    }}");
        }
        s.push_str("}\n");
        s
    }

    pub fn place_str(self, p: &Place<'tcx>) -> String {
        let mut s = format!("{:?}", p.local);
        for elem in &p.projection {
            s = match elem {
                PlaceElem::Deref => format!("(*{s})"),
                PlaceElem::Field(i, ty) => format!("({s}.{i}: {})", self.ty_to_string(*ty)),
                PlaceElem::Index(l) => format!("{s}[{l:?}]"),
                PlaceElem::ConstantIndex { offset, min_length, from_end: false } => format!("{s}[{offset} of {min_length}]"),
                PlaceElem::ConstantIndex { offset, min_length, from_end: true } => format!("{s}[-{offset} of {min_length}]"),
                PlaceElem::Subslice { from, to, from_end: true } => format!("{s}[{from}:-{to}]"),
                PlaceElem::Subslice { from, to, from_end: false } => format!("{s}[{from}..{to}]"),
                PlaceElem::Downcast(v) => format!("({s} as variant#{v})"),
            };
        }
        s
    }

    pub fn operand_str(self, o: &Operand<'tcx>) -> String {
        match o {
            Operand::Copy(p) => self.place_str(p),
            Operand::Move(p) => format!("move {}", self.place_str(p)),
            Operand::Constant(c) => format!("const {}", self.const_str(c)),
        }
    }

    fn const_str(self, c: &ConstOperand<'tcx>) -> String {
        match c.value {
            ConstValue::Scalar(bits) => match *c.ty.kind() {
                TyKind::Bool => (bits != 0).to_string(),
                TyKind::Char => format!("{:?}", char::from_u32(bits as u32).unwrap_or('?')),
                TyKind::Int(i) => {
                    let w = int_width(self, PrimTy::Int(i));
                    let shift = 128 - w;
                    format!("{}_{}", ((bits << shift) as i128) >> shift, PrimTy::Int(i).name())
                }
                TyKind::Uint(u) => format!("{bits}_{}", PrimTy::Uint(u).name()),
                TyKind::Float(vsc_hir::def::FloatTy::F32) => format!("{}_float32", f32::from_bits(bits as u32)),
                TyKind::Float(vsc_hir::def::FloatTy::F64) => format!("{}_float64", f64::from_bits(bits as u64)),
                _ => format!("{bits:#x}: {}", self.ty_to_string(c.ty)),
            },
            ConstValue::ZeroSized => match *c.ty.kind() {
                TyKind::FnDef(d, _) => self.def_path_str(d),
                _ => format!("{}: {}", "ZeroSized", self.ty_to_string(c.ty)).replace("ZeroSized: ()", "()"),
            },
            ConstValue::Str(sym) => format!("\"{sym}\""),
            ConstValue::Unevaluated(d, _) => self.def_path_str(d),
            ConstValue::StaticRef(d) => format!("{{&{}}}", self.def_path_str(d)),
            ConstValue::Ty(k) => self.const_to_string(k),
        }
    }

    pub fn rvalue_str(self, r: &Rvalue<'tcx>) -> String {
        let ops = |v: &[Operand<'tcx>]| v.iter().map(|o| self.operand_str(o)).collect::<Vec<_>>().join(", ");
        match r {
            Rvalue::Use(o) => self.operand_str(o),
            Rvalue::Repeat(o, n) => format!("[{}; {}]", self.operand_str(o), self.const_to_string(*n)),
            Rvalue::Ref(BorrowKind::Shared, p) => format!("&{}", self.place_str(p)),
            Rvalue::Ref(BorrowKind::Mut, p) => format!("&mut {}", self.place_str(p)),
            Rvalue::RawPtr(m, p) => format!("&raw {} {}", if m.is_mut() { "mut" } else { "const" }, self.place_str(p)),
            Rvalue::Len(p) => format!("Len({})", self.place_str(p)),
            Rvalue::Cast(k, o, ty) => format!("{} as {} ({k:?})", self.operand_str(o), self.ty_to_string(*ty)),
            Rvalue::BinaryOp(op, b) => format!("{op:?}({}, {})", self.operand_str(&b.0), self.operand_str(&b.1)),
            Rvalue::CheckedBinaryOp(op, b) => format!("{op:?}WithOverflow({}, {})", self.operand_str(&b.0), self.operand_str(&b.1)),
            Rvalue::UnaryOp(op, o) => format!("{op:?}({})", self.operand_str(o)),
            Rvalue::Discriminant(p) => format!("discriminant({})", self.place_str(p)),
            Rvalue::ClassNew(o) => format!("ClassNew({})", self.operand_str(o)),
            Rvalue::Aggregate(kind, v) => match &**kind {
                AggregateKind::Array(_) => format!("[{}]", ops(v)),
                AggregateKind::Tuple => format!("({})", ops(v)),
                AggregateKind::Adt { def_id, variant_index, active_field, .. } => {
                    let adt = self.adt_def(*def_id);
                    let var = &adt.variants()[*variant_index];
                    let name = self.def_path_str(var.def_id);
                    match var.ctor {
                        Some((vsc_hir::def::CtorKind::Const, _)) => name,
                        Some((vsc_hir::def::CtorKind::Fn, _)) => format!("{name}({})", ops(v)),
                        None => {
                            let fields: Vec<String> = match active_field {
                                Some(f) => vec![format!("{}: {}", var.fields[*f].name, ops(v))],
                                None => var.fields.iter().zip(v).map(|(f, o)| format!("{}: {}", f.name, self.operand_str(o))).collect(),
                            };
                            format!("{name} {{ {} }}", fields.join(", "))
                        }
                    }
                }
                AggregateKind::Closure(d, _) => format!("{}({})", self.def_path_str(*d), ops(v)),
            },
        }
    }

    fn terminator_str(self, t: &TerminatorKind<'tcx>) -> String {
        match t {
            TerminatorKind::Goto { target } => format!("goto -> {target:?}"),
            TerminatorKind::SwitchInt { discr, targets } => {
                let mut arms: Vec<String> = targets.values.iter().map(|(v, b)| format!("{v}: {b:?}")).collect();
                arms.push(format!("otherwise: {:?}", targets.otherwise));
                format!("switchInt({}) -> [{}]", self.operand_str(discr), arms.join(", "))
            }
            TerminatorKind::Return => "return".to_string(),
            TerminatorKind::Unreachable => "unreachable".to_string(),
            TerminatorKind::Call { func, args, destination, target, .. } => {
                let args: Vec<String> = args.iter().map(|o| self.operand_str(o)).collect();
                let call = format!("{} = {}({})", self.place_str(destination), self.operand_str(func), args.join(", "));
                match target {
                    Some(t) => format!("{call} -> {t:?}"),
                    None => format!("{call} -> diverges"),
                }
            }
            TerminatorKind::Assert { cond, expected, msg, target } => {
                let m = match &**msg {
                    AssertKind::BoundsCheck { len, index } => {
                        format!("\"index out of bounds: the length is {{}} but the index is {{}}\", {}, {}", self.operand_str(len), self.operand_str(index))
                    }
                    AssertKind::Overflow(op, a, b) => format!("\"attempt to compute `{{}} {op:?} {{}}`, which would overflow\", {}, {}", self.operand_str(a), self.operand_str(b)),
                    AssertKind::DivisionByZero(a) => format!("\"attempt to divide `{{}}` by zero\", {}", self.operand_str(a)),
                    AssertKind::RemainderByZero(a) => format!("\"attempt to calculate the remainder of `{{}}` with a divisor of zero\", {}", self.operand_str(a)),
                };
                let neg = if *expected { "" } else { "!" };
                format!("assert({neg}{}, {m}) -> {target:?}", self.operand_str(cond))
            }
            TerminatorKind::InlineAsm { target, .. } => match target {
                Some(t) => format!("asm!(…) -> {t:?}"),
                None => "asm!(…) -> diverges".to_string(),
            },
        }
    }
}

fn int_width(tcx: TyCtxt<'_>, p: PrimTy) -> u32 {
    use vsc_hir::def::{IntTy, UintTy};
    match p {
        PrimTy::Int(IntTy::I8) | PrimTy::Uint(UintTy::U8) => 8,
        PrimTy::Int(IntTy::I16) | PrimTy::Uint(UintTy::U16) => 16,
        PrimTy::Int(IntTy::I32) | PrimTy::Uint(UintTy::U32) => 32,
        PrimTy::Int(IntTy::I64) | PrimTy::Uint(UintTy::U64) => 64,
        PrimTy::Int(IntTy::I128) | PrimTy::Uint(UintTy::U128) => 128,
        PrimTy::Int(IntTy::Isize) | PrimTy::Uint(UintTy::Usize) => tcx.target.pointer_width,
        _ => 128,
    }
}
