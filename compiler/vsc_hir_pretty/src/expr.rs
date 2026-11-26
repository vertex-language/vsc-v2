//! Pretty-printing HIR expressions, blocks, and inline assembly.

use std::fmt::Write;

use vsc_ast::{CaptureBy, UnOp};
use vsc_hir::*;

use super::{PREC_LOWEST, PREC_POSTFIX, PREC_PREFIX, Printer, expr_prec, prec};

impl<'a> Printer<'a> {
    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------

    /// An operand that binds at least as tightly as `min`.
    pub(super) fn expr_prec(&mut self, e: &Expr, min: u8) {
        if expr_prec(e) < min {
            self.w("(");
            self.expr(e);
            self.w(")");
        } else {
            self.expr(e);
        }
    }

    pub(super) fn label(&mut self, l: &Option<Label>) {
        if let Some(l) = l {
            let _ = write!(self.out, "'{}: ", l.ident.as_str().trim_start_matches('\''));
        }
    }

    pub(super) fn expr(&mut self, e: &Expr) {
        self.attrs(e.hir_id);
        match &e.kind {
            ExprKind::ConstBlock(c) => {
                self.w("const ");
                self.body_value(c.body);
            }
            ExprKind::Array(es) => {
                self.w("[");
                self.comma_sep(es, |p, e| p.expr(e));
                self.w("]");
            }
            ExprKind::Call(f, args) => {
                self.expr_prec(f, PREC_POSTFIX);
                self.w("(");
                self.comma_sep(args, |p, e| p.expr(e));
                self.w(")");
            }
            ExprKind::MethodCall(seg, recv, args, _) => {
                self.expr_prec(recv, PREC_POSTFIX);
                self.w(".");
                self.segment(seg, true);
                self.w("(");
                self.comma_sep(args, |p, e| p.expr(e));
                self.w(")");
            }
            ExprKind::Tup(es) => {
                self.w("(");
                self.comma_sep(es, |p, e| p.expr(e));
                if es.len() == 1 {
                    self.w(",");
                }
                self.w(")");
            }
            ExprKind::Binary(op, a, b) => {
                let p = prec(op.node);
                self.expr_prec(a, p);
                let _ = write!(self.out, " {} ", op.node.as_str());
                self.expr_prec(b, p + 1);
            }
            ExprKind::Unary(op, x) => {
                self.w(match op {
                    UnOp::Deref => "*",
                    UnOp::Not => "!",
                    UnOp::Neg => "-",
                });
                self.expr_prec(x, PREC_PREFIX);
            }
            ExprKind::Lit(lit) => {
                let _ = write!(self.out, "{lit}");
            }
            ExprKind::Cast(x, t) => {
                self.expr_prec(x, 11);
                self.w(" as ");
                self.ty(t);
            }
            ExprKind::DropTemps(x) => {
                self.w("drop_temps!(");
                self.expr(x);
                self.w(")");
            }
            ExprKind::Let(l) => {
                self.w("let ");
                self.pat(&l.pat);
                if let Some(t) = &l.ty {
                    self.w(": ");
                    self.ty(t);
                }
                self.w(" = ");
                self.expr_prec(&l.init, 3);
            }
            ExprKind::If(cond, then, els) => {
                self.w("if ");
                self.expr(cond);
                self.w(" ");
                self.expr(then);
                if let Some(els) = els {
                    self.w(" else ");
                    match els.kind {
                        ExprKind::Block(..) | ExprKind::If(..) => self.expr(els),
                        // A desugaring's `else break`.
                        _ => {
                            self.w("{ ");
                            self.expr(els);
                            self.w(" }");
                        }
                    }
                }
            }
            ExprKind::Loop(body, label, _, _) => {
                self.label(label);
                self.w("loop ");
                self.block(body);
            }
            ExprKind::Match(scrutinee, arms, _) => {
                self.w("match ");
                self.expr(scrutinee);
                self.w(" {");
                self.indent += 1;
                for arm in arms {
                    self.nl();
                    self.attrs(arm.hir_id);
                    self.pat(&arm.pat);
                    if let Some(g) = &arm.guard {
                        self.w(" if ");
                        self.expr(g);
                    }
                    self.w(" => ");
                    self.expr(&arm.body);
                    self.w(",");
                }
                self.indent -= 1;
                self.nl();
                self.w("}");
            }
            ExprKind::Closure(c) => {
                let body = self.krate.body(c.body);
                match c.kind {
                    ClosureKind::AsyncBlock | ClosureKind::AsyncFn => {
                        self.w("async ");
                        if c.capture_clause == CaptureBy::Value {
                            self.w("move ");
                        }
                        self.expr(&body.value);
                        return;
                    }
                    ClosureKind::AsyncClosure => self.w("async "),
                    ClosureKind::Closure => {}
                }
                if c.capture_clause == CaptureBy::Value {
                    self.w("move ");
                }
                self.w("|");
                for (i, param) in body.params.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    self.pat(&param.pat);
                    if let Some(t) = c.fn_decl.inputs.get(i) {
                        if !matches!(t.kind, TyKind::Infer) {
                            self.w(": ");
                            self.ty(t);
                        }
                    }
                }
                self.w("|");
                self.fn_ret(&c.fn_decl.output);
                self.w(" ");
                self.expr(&body.value);
            }
            ExprKind::Block(b, label) => {
                self.label(label);
                self.block(b);
            }
            ExprKind::Assign(l, r, _) => {
                self.expr_prec(l, PREC_LOWEST + 1);
                self.w(" = ");
                self.expr(r);
            }
            ExprKind::AssignOp(op, l, r) => {
                self.expr_prec(l, PREC_LOWEST + 1);
                let _ = write!(self.out, " {}= ", op.node.as_str());
                self.expr(r);
            }
            ExprKind::Field(x, ident) => {
                self.expr_prec(x, PREC_POSTFIX);
                let _ = write!(self.out, ".{ident}");
            }
            ExprKind::Index(x, i, _) => {
                self.expr_prec(x, PREC_POSTFIX);
                self.w("[");
                self.expr(i);
                self.w("]");
            }
            ExprKind::Path(q) => self.qpath(q, true),
            ExprKind::AddrOf(k, m, x) => {
                self.w("&");
                if *k == vsc_ast::BorrowKind::Raw {
                    self.w(if m.is_mut() { "raw mut " } else { "raw const " });
                } else if m.is_mut() {
                    self.w("mut ");
                }
                self.expr_prec(x, PREC_PREFIX);
            }
            ExprKind::Break(dest, value) => {
                self.w("break");
                if let Some(l) = dest.label {
                    let _ = write!(self.out, " '{}", l.ident.as_str().trim_start_matches('\''));
                }
                if let Some(v) = value {
                    self.w(" ");
                    self.expr(v);
                }
            }
            ExprKind::Continue(dest) => {
                self.w("continue");
                if let Some(l) = dest.label {
                    let _ = write!(self.out, " '{}", l.ident.as_str().trim_start_matches('\''));
                }
            }
            ExprKind::Ret(v) => {
                self.w("return");
                if let Some(v) = v {
                    self.w(" ");
                    self.expr(v);
                }
            }
            ExprKind::InlineAsm(asm) => self.inline_asm(asm),
            ExprKind::Struct(q, fields, tail) => {
                self.qpath(q, false);
                self.w(" { ");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    if f.is_shorthand {
                        self.expr(&f.expr);
                    } else {
                        let _ = write!(self.out, "{}: ", f.ident);
                        self.expr(&f.expr);
                    }
                }
                if let StructTailExpr::Base(b) = tail {
                    if !fields.is_empty() {
                        self.w(", ");
                    }
                    self.w("..");
                    self.expr(b);
                }
                self.w(" }");
            }
            ExprKind::Repeat(x, n) => {
                self.w("[");
                self.expr(x);
                self.w("; ");
                self.anon_const(n);
                self.w("]");
            }
            ExprKind::Await(x) => {
                self.expr_prec(x, PREC_POSTFIX);
                self.w(".await");
            }
            ExprKind::Err => self.w("/*error*/"),
        }
    }

    pub(super) fn block(&mut self, b: &Block) {
        if b.rules == BlockCheckMode::Unsafe {
            self.w("unsafe ");
        }
        if b.stmts.is_empty() && b.expr.is_none() {
            self.w("{ }");
            return;
        }
        self.w("{");
        self.indent += 1;
        for s in &b.stmts {
            self.nl();
            self.attrs(s.hir_id);
            match &s.kind {
                StmtKind::Let(l) => {
                    self.attrs(l.hir_id);
                    self.w("let ");
                    self.pat(&l.pat);
                    if let Some(t) = &l.ty {
                        self.w(": ");
                        self.ty(t);
                    }
                    if let Some(init) = &l.init {
                        self.w(" = ");
                        self.expr(init);
                    }
                    if let Some(els) = &l.els {
                        self.w(" else ");
                        self.block(els);
                    }
                    self.w(";");
                }
                StmtKind::Item(id) => self.item(*id),
                StmtKind::Semi(e) => {
                    self.expr(e);
                    self.w(";");
                }
            }
        }
        if let Some(e) = &b.expr {
            self.nl();
            self.expr(e);
        }
        self.indent -= 1;
        self.nl();
        self.w("}");
    }

    pub(super) fn inline_asm(&mut self, asm: &InlineAsm) {
        self.w(match asm.asm_macro {
            vsc_ast::AsmMacro::Asm => "asm!(",
            vsc_ast::AsmMacro::NakedAsm => "naked_asm!(",
            vsc_ast::AsmMacro::GlobalAsm => "global_asm!(",
        });
        let mut first = true;
        let mut sep = |p: &mut Self| {
            if !first {
                p.w(", ");
            }
            first = false;
        };
        for (lit, _) in &asm.templates {
            sep(self);
            let _ = write!(self.out, "{lit}");
        }
        for (op, _) in &asm.operands {
            sep(self);
            let reg = |p: &mut Self, reg: vsc_span::Symbol, explicit: bool| {
                if explicit {
                    let _ = write!(p.out, "(\"{reg}\") ");
                } else {
                    let _ = write!(p.out, "({reg}) ");
                }
            };
            match op {
                InlineAsmOperand::In { reg: r, explicit, expr } => {
                    self.w("in");
                    reg(self, *r, *explicit);
                    self.expr(expr);
                }
                InlineAsmOperand::Out { reg: r, explicit, late, expr } => {
                    self.w(if *late { "lateout" } else { "out" });
                    reg(self, *r, *explicit);
                    match expr {
                        Some(e) => self.expr(e),
                        None => self.w("_"),
                    }
                }
                InlineAsmOperand::InOut { reg: r, explicit, late, expr } => {
                    self.w(if *late { "inlateout" } else { "inout" });
                    reg(self, *r, *explicit);
                    self.expr(expr);
                }
                InlineAsmOperand::SplitInOut { reg: r, explicit, late, in_expr, out_expr } => {
                    self.w(if *late { "inlateout" } else { "inout" });
                    reg(self, *r, *explicit);
                    self.expr(in_expr);
                    self.w(" => ");
                    match out_expr {
                        Some(e) => self.expr(e),
                        None => self.w("_"),
                    }
                }
                InlineAsmOperand::Const { anon_const } => {
                    self.w("const ");
                    self.anon_const(anon_const);
                }
                InlineAsmOperand::SymFn { expr } => {
                    self.w("sym ");
                    self.expr(expr);
                }
                InlineAsmOperand::Label { block } => {
                    self.w("label ");
                    self.block(block);
                }
            }
        }
        for (abi, _) in &asm.clobber_abis {
            sep(self);
            let _ = write!(self.out, "clobber_abi(\"{abi}\")");
        }
        if !asm.options.is_empty() {
            sep(self);
            self.w("options(");
            let opts: Vec<&str> = asm.options.iter().map(|(o, _)| o.as_str()).collect();
            self.w(&opts.join(", "));
            self.w(")");
        }
        self.w(")");
    }
}

