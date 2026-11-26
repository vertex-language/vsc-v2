//! Lowering inline assembly (rustc's `asm.rs`; exception 12's `asm(…)`,
//! `nakedAsm(…)` and `globalAsm(…)`).
//!
//! Operand names were for the template only; HIR keeps operands in order.

use vsc_ast::{self as ast};
use vsc_hir::{self as hir};

use crate::LoweringContext;
use crate::path::PathMode;

impl LoweringContext<'_> {
    pub(crate) fn lower_inline_asm(&mut self, asm: &ast::InlineAsm) -> hir::InlineAsm {
        let operands = asm
            .operands
            .iter()
            .map(|(op, span)| {
                let reg = |r: &ast::InlineAsmRegOrRegClass| match r {
                    ast::InlineAsmRegOrRegClass::Reg(s) => (*s, true),
                    ast::InlineAsmRegOrRegClass::RegClass(s) => (*s, false),
                };
                let op = match op {
                    ast::InlineAsmOperand::In { reg: r, expr, .. } => {
                        let (reg, explicit) = reg(r);
                        hir::InlineAsmOperand::In { reg, explicit, expr: self.lower_expr(expr) }
                    }
                    ast::InlineAsmOperand::Out { reg: r, late, expr, .. } => {
                        let (reg, explicit) = reg(r);
                        hir::InlineAsmOperand::Out { reg, explicit, late: *late, expr: expr.as_ref().map(|e| self.lower_expr(e)) }
                    }
                    ast::InlineAsmOperand::InOut { reg: r, late, expr, .. } => {
                        let (reg, explicit) = reg(r);
                        hir::InlineAsmOperand::InOut { reg, explicit, late: *late, expr: self.lower_expr(expr) }
                    }
                    ast::InlineAsmOperand::SplitInOut { reg: r, late, in_expr, out_expr, .. } => {
                        let (reg, explicit) = reg(r);
                        hir::InlineAsmOperand::SplitInOut {
                            reg,
                            explicit,
                            late: *late,
                            in_expr: self.lower_expr(in_expr),
                            out_expr: out_expr.as_ref().map(|e| self.lower_expr(e)),
                        }
                    }
                    ast::InlineAsmOperand::Const { anon_const, .. } => {
                        hir::InlineAsmOperand::Const { anon_const: self.lower_anon_const(anon_const) }
                    }
                    ast::InlineAsmOperand::Sym { id, qself, path, .. } => {
                        let (qpath, _) = self.lower_qpath(*id, qself, path, PathMode::Type);
                        let hir_id = self.lower_node_id(*id);
                        hir::InlineAsmOperand::SymFn { expr: hir::Expr { hir_id, kind: hir::ExprKind::Path(qpath), span: path.span } }
                    }
                    ast::InlineAsmOperand::Label { block, .. } => {
                        hir::InlineAsmOperand::Label { block: self.lower_block(block) }
                    }
                };
                (op, *span)
            })
            .collect();
        hir::InlineAsm {
            asm_macro: asm.asm_macro,
            templates: asm.templates.clone(),
            operands,
            clobber_abis: asm.clobber_abis.clone(),
            options: asm.options.clone(),
            span: asm.span,
        }
    }
}
