//! MIR terminator codegen (branches, switches, calls, drops, panics, returns).

use obsidian::ir;
use vsc_codegen_ssa::common::IntPredicate;
use vsc_codegen_ssa::mir::{OperandValue, PlaceRef};
use vsc_middle::mir::{Body, TerminatorKind};
use vsc_middle::ty::{Instance, Ty};

use super::Builder;
use crate::value::{ObsBlock, ObsVal};

impl<'a, 'tcx> Builder<'a, 'tcx> {
    /// Calls the runtime panic helper.
    pub fn call_panic_abort(&self, msg: &str) {
        let panic_fn = self.cx.panic_abort_fn();
        let ptr = self.cx.type_ptr();
        let fn_ty = self
            .cx
            .type_func(&[ptr, self.cx.type_i64()], self.cx.type_void(), false);
        let (msg_ptr, msg_len) = self.cx.const_bytes_global(msg.as_bytes());
        self.call_raw(
            fn_ty,
            panic_fn,
            &mut [msg_ptr, self.cx.const_u64(msg_len as u64)],
        );
        self.unreachable();
    }

    /// Lowers a basic block's terminator given the pre-created `block_bbs` slice.
    pub fn codegen_terminator(
        &mut self,
        body: &Body<'tcx>,
        locals: &[PlaceRef<'tcx, ObsVal>],
        instance: Instance<'tcx>,
        fn_abi: &vsc_target::callconv::FnAbi<'tcx, Ty<'tcx>>,
        block_bbs: &[ObsBlock],
        term: &TerminatorKind<'tcx>,
    ) {
        match term {
            TerminatorKind::Goto { target } => {
                self.br(block_bbs[target.as_usize()]);
            }
            TerminatorKind::SwitchInt { discr, targets } => {
                let discr_op = self.codegen_operand(body, locals, instance, discr);
                let OperandValue::Immediate(val) = discr_op.val else {
                    self.br(block_bbs[targets.otherwise.as_usize()]);
                    return;
                };
                if targets.values.len() == 1
                    && (targets.values[0].0 == 0 || targets.values[0].0 == 1)
                    && self.cx.int_width(val.ty()) == 1
                {
                    let (v, bb) = targets.values[0];
                    if v == 1 {
                        self.cond_br(
                            val,
                            block_bbs[bb.as_usize()],
                            block_bbs[targets.otherwise.as_usize()],
                        );
                    } else {
                        self.cond_br(
                            val,
                            block_bbs[targets.otherwise.as_usize()],
                            block_bbs[bb.as_usize()],
                        );
                    }
                    return;
                }
                if matches!(val, ObsVal::Pair(..)) {
                    let otherwise_bb = block_bbs[targets.otherwise.as_usize()];
                    for (idx, &(v, bb)) in targets.values.iter().enumerate() {
                        let case_const = self.cx.const_uint_big(self.cx.type_i128(), v);
                        let is_eq = self.icmp(IntPredicate::IntEQ, val, case_const);
                        if idx + 1 == targets.values.len() {
                            self.cond_br(is_eq, block_bbs[bb.as_usize()], otherwise_bb);
                        } else {
                            let next_bb = Self::append_block(
                                self.cx,
                                self.llfn(),
                                &format!("sw128_next_{idx}"),
                            );
                            self.cond_br(is_eq, block_bbs[bb.as_usize()], next_bb);
                            self.position_at_end(next_bb);
                        }
                    }
                    if targets.values.is_empty() {
                        self.br(otherwise_bb);
                    }
                    return;
                }
                let val_64 = self.intcast(val, self.cx.type_i64(), false);
                let cases: Vec<(u64, ObsBlock)> = targets
                    .values
                    .iter()
                    .map(|&(v, bb)| (v as u64, block_bbs[bb.as_usize()]))
                    .collect();
                self.switch_int(val_64, block_bbs[targets.otherwise.as_usize()], &cases);
            }
            TerminatorKind::Return => {
                crate::callee::codegen_return(self, locals[0], fn_abi);
            }
            TerminatorKind::Unreachable => {
                self.unreachable();
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
                ..
            } => {
                let func_op = self.codegen_operand(body, locals, instance, func);
                let arg_ops: Vec<_> = args
                    .iter()
                    .map(|a| self.codegen_operand(body, locals, instance, a))
                    .collect();
                let dst = self.codegen_place(body, locals, instance, destination);
                crate::callee::codegen_call(self, func_op, &arg_ops, dst);
                match target {
                    Some(t) => {
                        if !self.is_cur_block_terminated() {
                            self.br(block_bbs[t.as_usize()]);
                        }
                    }
                    None => {
                        if !self.is_cur_block_terminated() {
                            self.unreachable();
                        }
                    }
                }
            }
            TerminatorKind::Assert {
                cond,
                expected,
                msg,
                target,
            } => {
                let cond_op = self.codegen_operand(body, locals, instance, cond);
                let OperandValue::Immediate(mut cval) = cond_op.val else {
                    self.br(block_bbs[target.as_usize()]);
                    return;
                };
                if cval.as_scalar().ty() != ir::Type::I1 {
                    cval = self.trunc(cval, self.cx.type_i1());
                }
                let ok = if *expected { cval } else { self.not(cval) };
                let fail_bb = Self::append_block(self.cx, self.llfn(), "assert_fail");
                self.cond_br(ok, block_bbs[target.as_usize()], fail_bb);
                self.position_at_end(fail_bb);
                let text = match &**msg {
                    vsc_middle::mir::AssertKind::BoundsCheck { .. } => "index out of bounds",
                    vsc_middle::mir::AssertKind::Overflow(..) => "attempt to compute with overflow",
                    vsc_middle::mir::AssertKind::DivisionByZero(..) => "attempt to divide by zero",
                    vsc_middle::mir::AssertKind::RemainderByZero(..) => {
                        "attempt to calculate the remainder with a zero divisor"
                    }
                };
                self.call_panic_abort(text);
            }
            TerminatorKind::Drop { place, target, .. } => {
                let p = self.codegen_place(body, locals, instance, place);
                if self.cx.tcx.needs_drop_raw(p.layout.ty) {
                    let drop_inst = Instance::resolve_drop_in_place(self.cx.tcx, p.layout.ty);
                    let drop_fn = self.cx.get_fn(drop_inst);
                    let drop_abi = self.cx.fn_abi_of_drop_glue(p.layout.ty);
                    let drop_ty = crate::abi::fn_abi_obs_type(self.cx, drop_abi);
                    self.call_raw(drop_ty, drop_fn, &mut [p.val.llval]);
                }
                self.br(block_bbs[target.as_usize()]);
            }
            TerminatorKind::InlineAsm { target, .. } => {
                if let Some(t) = target {
                    self.br(block_bbs[t.as_usize()]);
                } else {
                    self.unreachable();
                }
            }
            TerminatorKind::Yield { .. } => {
                panic!("untransformed TerminatorKind::Yield reached codegen");
            }
        }
    }
}
