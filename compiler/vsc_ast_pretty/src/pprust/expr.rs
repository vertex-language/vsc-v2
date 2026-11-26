//! Pretty-printing blocks, statements, expressions, builtins, and inline assembly.

use vsc_ast::*;
use vsc_span::Ident;

use super::Printer;

impl Printer {
    // -----------------------------------------------------------------------
    // Statements and blocks
    // -----------------------------------------------------------------------

    /// A block on its own lines, or `{ e }` on one line when `inline` is
    /// allowed and the block is a single short expression.
    pub(super) fn block(&mut self, b: &Block, inline: bool) {
        if b.rules == BlockCheckMode::Unsafe {
            self.w("unsafe ");
        }
        if b.stmts.is_empty() {
            self.w("{}");
            return;
        }
        if inline && b.stmts.len() == 1 {
            if let StmtKind::Expr(e) = &b.stmts[0].kind {
                if is_simple(e) {
                    self.w("{ ");
                    self.expr(e);
                    self.w(" }");
                    return;
                }
            }
        }
        self.w("{");
        self.indent += 1;
        for s in &b.stmts {
            self.nl();
            self.stmt(s);
        }
        self.indent -= 1;
        self.nl();
        self.w("}");
    }

    pub(super) fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let(l) => {
                self.attrs(&l.attrs);
                self.w(if l.is_var { "var " } else { "let " });
                self.pat(&l.pat);
                if let Some(t) = &l.ty {
                    self.w(": ");
                    self.ty(t);
                }
                match &l.kind {
                    LocalKind::Decl => {}
                    LocalKind::Init(e) => {
                        self.w(" = ");
                        self.expr(e);
                    }
                    LocalKind::InitElse(e, b) => {
                        self.w(" = ");
                        self.expr(e);
                        self.w(" else ");
                        self.block(b, false);
                    }
                }
            }
            StmtKind::Item(i) => self.item(i),
            StmtKind::Expr(e) => self.expr(e),
        }
    }

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------

    pub(super) fn label(&mut self, label: &Option<Label>) {
        if let Some(l) = label {
            self.w(l.ident.name.as_str());
            self.w(": ");
        }
    }

    pub(super) fn expr(&mut self, e: &Expr) {
        for a in &e.attrs {
            self.attr(a);
            self.w(" ");
        }
        match &e.kind {
            ExprKind::Array(es) => {
                self.w("[");
                self.comma_sep(es, |p, e| p.expr(e));
                self.w("]");
            }
            ExprKind::Repeat(e, n) => {
                self.w("[");
                self.expr(e);
                self.w("; ");
                self.expr(&n.value);
                self.w("]");
            }
            ExprKind::Vec(inner) => {
                self.w("vec");
                self.expr(inner);
            }
            ExprKind::ConstBlock(c) => {
                self.w("const ");
                self.expr(&c.value);
            }
            ExprKind::Call(f, args) => {
                self.expr(f);
                self.w("(");
                self.comma_sep(args, |p, a| p.expr(a));
                self.w(")");
            }
            ExprKind::MethodCall(m) => {
                self.expr(&m.receiver);
                self.w(".");
                self.ident(m.seg.ident);
                if let Some(a) = &m.seg.args {
                    self.generic_args(a);
                }
                self.w("(");
                self.comma_sep(&m.args, |p, a| p.expr(a));
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
            ExprKind::Binary(op, l, r) => {
                self.expr(l);
                self.w(" ");
                self.w(op.node.as_str());
                self.w(" ");
                self.expr(r);
            }
            ExprKind::Unary(op, e) => {
                self.w(op.as_str());
                self.expr(e);
            }
            ExprKind::Lit(lit) => self.w(&lit.to_string()),
            ExprKind::Cast(e, t) => {
                self.expr(e);
                self.w(" as ");
                self.ty(t);
            }
            ExprKind::Let(pat, e, _) => {
                self.w("let ");
                self.pat(pat);
                self.w(" = ");
                self.expr(e);
            }
            ExprKind::If(cond, then, els) => {
                self.w("if ");
                self.expr(cond);
                self.w(" ");
                self.block(then, false);
                if let Some(els) = els {
                    self.w(" else ");
                    match &els.kind {
                        ExprKind::Block(b, None) => self.block(b, false),
                        _ => self.expr(els),
                    }
                }
            }
            ExprKind::While(cond, body, label) => {
                self.label(label);
                self.w("while ");
                self.expr(cond);
                self.w(" ");
                self.block(body, false);
            }
            ExprKind::ForLoop { pat, iter, body, label } => {
                self.label(label);
                self.w("for ");
                self.pat(pat);
                self.w(" in ");
                self.expr(iter);
                self.w(" ");
                self.block(body, false);
            }
            ExprKind::Loop(body, label, _) => {
                self.label(label);
                self.w("loop ");
                self.block(body, false);
            }
            ExprKind::Match(scrutinee, arms) => {
                self.w("match ");
                self.expr(scrutinee);
                if arms.is_empty() {
                    self.w(" {}");
                    return;
                }
                self.w(" {");
                self.indent += 1;
                for arm in arms {
                    self.nl();
                    self.attrs(&arm.attrs);
                    self.pat(&arm.pat);
                    if let Some(g) = &arm.guard {
                        self.w(" if ");
                        self.expr(g);
                    }
                    self.w(" => ");
                    match &arm.body.kind {
                        ExprKind::Block(b, None) if arm.body.attrs.is_empty() => self.block(b, true),
                        _ => self.expr(&arm.body),
                    }
                }
                self.indent -= 1;
                self.nl();
                self.w("}");
            }
            ExprKind::Closure(c) => {
                if c.asyncness.is_some() {
                    self.w("async ");
                }
                self.w("func(");
                self.comma_sep(&c.fn_decl.inputs, |p, param| p.param(param));
                self.w(")");
                self.ret_ty(&c.fn_decl.output);
                self.w(" ");
                self.block(&c.body, true);
            }
            ExprKind::Block(b, label) => {
                self.label(label);
                self.block(b, false);
            }
            ExprKind::Async(b) => {
                self.w("async ");
                self.block(b, true);
            }
            ExprKind::Await(e, _) => {
                self.expr(e);
                self.w(".await");
            }
            ExprKind::Assign(l, r, _) => {
                self.expr(l);
                self.w(" = ");
                self.expr(r);
            }
            ExprKind::AssignOp(op, l, r) => {
                self.expr(l);
                self.w(" ");
                self.w(op.node.as_str());
                self.w("= ");
                self.expr(r);
            }
            ExprKind::Field(e, ident) => {
                self.expr(e);
                self.w(".");
                self.w(ident.name.as_str());
            }
            ExprKind::Index(e, idx, _) => {
                self.expr(e);
                self.w("[");
                self.expr(idx);
                self.w("]");
            }
            ExprKind::Range(lo, hi, limits) => {
                if let Some(lo) = lo {
                    self.expr(lo);
                }
                self.w(match limits {
                    RangeLimits::HalfOpen => "..",
                    RangeLimits::Closed => "..=",
                });
                if let Some(hi) = hi {
                    self.expr(hi);
                }
            }
            ExprKind::Underscore => self.w("_"),
            ExprKind::Path(None, path) => self.path(path, true),
            ExprKind::Path(Some(q), path) => self.qpath(q, path),
            ExprKind::AddrOf(kind, m, e) => {
                self.w("&");
                if *kind == BorrowKind::Raw {
                    self.w(if m.is_mut() { "raw mut " } else { "raw const " });
                } else if m.is_mut() {
                    self.w("mut ");
                }
                self.expr(e);
            }
            ExprKind::Break(label, value) => {
                self.w("break");
                if let Some(l) = label {
                    self.w(" ");
                    self.w(l.ident.name.as_str());
                }
                if let Some(v) = value {
                    self.w(" ");
                    self.expr(v);
                }
            }
            ExprKind::Continue(label) => {
                self.w("continue");
                if let Some(l) = label {
                    self.w(" ");
                    self.w(l.ident.name.as_str());
                }
            }
            ExprKind::Ret(value) => {
                self.w("return");
                if let Some(v) = value {
                    self.w(" ");
                    self.expr(v);
                }
            }
            ExprKind::InlineAsm(asm) => self.inline_asm(asm),
            ExprKind::Builtin(b) => self.builtin(b),
            ExprKind::Struct(s) => {
                self.maybe_qpath(s.qself.as_deref(), &s.path);
                if s.fields.is_empty() && matches!(s.rest, StructRest::None) {
                    self.w(" {}");
                    return;
                }
                self.w(" { ");
                self.comma_sep(&s.fields, |p, f| {
                    if f.is_shorthand {
                        p.ident(f.ident);
                    } else {
                        p.w(f.ident.name.as_str());
                        p.w(": ");
                        p.expr(&f.expr);
                    }
                });
                if let StructRest::Base(base) = &s.rest {
                    if !s.fields.is_empty() {
                        self.w(", ");
                    }
                    self.w("..");
                    self.expr(base);
                }
                self.w(" }");
            }
            ExprKind::Paren(e) => {
                self.w("(");
                self.expr(e);
                self.w(")");
            }
            ExprKind::Try(e) => {
                self.expr(e);
                self.w("?");
            }
            ExprKind::Err => self.w("/*error*/"),
        }
    }

    /// A built-in call, printed as it was written.
    pub(super) fn builtin(&mut self, b: &BuiltinCall) {
        self.w(b.kind.name());
        self.w("(");
        let mut first = true;
        let mut sep = |p: &mut Self| {
            if !std::mem::replace(&mut first, false) {
                p.w(", ");
            }
        };
        let fmt = |p: &mut Self, f: &FormatArgs, sep: &mut dyn FnMut(&mut Self)| {
            sep(p);
            p.w(&f.lit.to_string());
            for arg in &f.arguments {
                match arg.kind {
                    FormatArgumentKind::Normal => {
                        sep(p);
                        p.expr(&arg.expr);
                    }
                    FormatArgumentKind::Named(name) => {
                        sep(p);
                        p.ident(name);
                        p.w(" = ");
                        p.expr(&arg.expr);
                    }
                    FormatArgumentKind::Captured(_) => {}
                }
            }
        };
        match &b.kind {
            BuiltinKind::Print { args, .. } | BuiltinKind::Format(args) => {
                if !(args.lit.symbol.is_empty() && matches!(b.kind, BuiltinKind::Print { newline: true, .. }) && args.lit_span.is_empty()) {
                    fmt(self, args, &mut sep);
                }
            }
            BuiltinKind::Panic(f) | BuiltinKind::Todo(f) | BuiltinKind::Unreachable(f) => {
                if let Some(f) = f {
                    fmt(self, f, &mut sep);
                }
            }
            BuiltinKind::Assert { cond, msg } => {
                sep(self);
                self.expr(cond);
                if let Some(f) = msg {
                    fmt(self, f, &mut sep);
                }
            }
            BuiltinKind::AssertEq { left, right, msg } => {
                sep(self);
                self.expr(left);
                sep(self);
                self.expr(right);
                if let Some(f) = msg {
                    fmt(self, f, &mut sep);
                }
            }
        }
        self.w(")");
    }

    pub(super) fn inline_asm(&mut self, asm: &InlineAsm) {
        self.w(match asm.asm_macro {
            AsmMacro::Asm => "asm(",
            AsmMacro::NakedAsm => "nakedAsm(",
            AsmMacro::GlobalAsm => "globalAsm(",
        });
        let mut first = true;
        let mut sep = |p: &mut Self| {
            if !std::mem::replace(&mut first, false) {
                p.w(", ");
            }
        };
        for (lit, _) in &asm.templates {
            sep(self);
            self.w(&lit.to_string());
        }
        for (op, _) in &asm.operands {
            sep(self);
            let reg = |r: &InlineAsmRegOrRegClass| match r {
                InlineAsmRegOrRegClass::Reg(s) => format!("(\"{s}\")"),
                InlineAsmRegOrRegClass::RegClass(s) => format!("({s})"),
            };
            let name = |p: &mut Self, n: &Option<Ident>| {
                if let Some(n) = n {
                    p.ident(*n);
                    p.w(" = ");
                }
            };
            match op {
                InlineAsmOperand::In { name: n, reg: r, expr } => {
                    name(self, n);
                    self.w(&format!("in{} ", reg(r)));
                    self.expr(expr);
                }
                InlineAsmOperand::Out { name: n, reg: r, late, expr } => {
                    name(self, n);
                    self.w(&format!("{}{} ", if *late { "lateout" } else { "out" }, reg(r)));
                    match expr {
                        Some(e) => self.expr(e),
                        None => self.w("_"),
                    }
                }
                InlineAsmOperand::InOut { name: n, reg: r, late, expr } => {
                    name(self, n);
                    self.w(&format!("{}{} ", if *late { "inlateout" } else { "inout" }, reg(r)));
                    self.expr(expr);
                }
                InlineAsmOperand::SplitInOut { name: n, reg: r, late, in_expr, out_expr } => {
                    name(self, n);
                    self.w(&format!("{}{} ", if *late { "inlateout" } else { "inout" }, reg(r)));
                    self.expr(in_expr);
                    self.w(" => ");
                    match out_expr {
                        Some(e) => self.expr(e),
                        None => self.w("_"),
                    }
                }
                InlineAsmOperand::Const { name: n, anon_const } => {
                    name(self, n);
                    self.w("const ");
                    self.expr(&anon_const.value);
                }
                InlineAsmOperand::Sym { name: n, qself, path, .. } => {
                    name(self, n);
                    self.w("sym ");
                    self.maybe_qpath(qself.as_deref(), path);
                }
                InlineAsmOperand::Label { name: n, block } => {
                    name(self, n);
                    self.w("label ");
                    self.block(block, true);
                }
            }
        }
        if !asm.clobber_abis.is_empty() {
            sep(self);
            self.w("clobberAbi(");
            let abis: Vec<String> = asm.clobber_abis.iter().map(|(s, _)| format!("\"{s}\"")).collect();
            self.w(&abis.join(", "));
            self.w(")");
        }
        if !asm.options.is_empty() {
            sep(self);
            self.w("options(");
            let opts: Vec<&str> = asm.options.iter().map(|(s, _)| s.as_str()).collect();
            self.w(&opts.join(", "));
            self.w(")");
        }
        self.w(")");
    }
}

/// Short enough for `{ e }` on one line: no nested blocks.
fn is_simple(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::If(..)
        | ExprKind::Match(..)
        | ExprKind::Loop(..)
        | ExprKind::While(..)
        | ExprKind::ForLoop { .. }
        | ExprKind::Block(..)
        | ExprKind::Async(..) => false,
        ExprKind::Closure(c) => c.body.stmts.len() <= 1,
        ExprKind::Call(f, args) => is_simple(f) && args.iter().all(|a| is_simple(a)),
        ExprKind::MethodCall(m) => is_simple(&m.receiver) && m.args.iter().all(|a| is_simple(a)),
        ExprKind::Binary(_, l, r) | ExprKind::Assign(l, r, _) | ExprKind::AssignOp(_, l, r) => {
            is_simple(l) && is_simple(r)
        }
        ExprKind::Unary(_, e) | ExprKind::Paren(e) | ExprKind::Try(e) | ExprKind::Field(e, _) | ExprKind::Await(e, _) => {
            is_simple(e)
        }
        _ => true,
    }
}

