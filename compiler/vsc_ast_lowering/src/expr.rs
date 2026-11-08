//! Lowering expressions (rustc's `expr.rs`), and exception 7's second half.
//!
//! A path expression whose trailing segments name no item is a field access
//! (`t.items` → `Field(t, items)`), and a call of one is a method call
//! (`self.items.Push(x)` → `MethodCall(Push, Field(self, items), [x])`).
//!
//! The desugarings are rustc's:
//!
//! - `while cond { body }` → `loop { if cond { body } else { break } }`
//! - `for pat in head { body }` →
//!   `match IntoIterator::into_iter(head) { mut iter => loop {
//!       match Iterator::next(&mut iter) { None => break, Some(pat) => body } } }`
//! - `x?` → `match Try::branch(x) { Continue(v) => v,
//!       Break(r) => return FromResidual::from_residual(r) }`
//! - `a..b` and friends → the range lang items
//! - `vec[…]` → the `vecNew`, `vecFromArray` and `vecFromElem` lang items
//! - `(a, b) = e` → `{ let (lhs0, lhs1) = e; a = lhs0; b = lhs1 }`

use vsc_ast::token::{Lit, LitKind};
use vsc_ast::{self as ast, P};
use vsc_errors::ErrCode;
use vsc_hir::def::Res;
use vsc_hir::{self as hir, HirId, LangItem};
use vsc_span::{Ident, Span, Spanned, Symbol};

use crate::LoweringContext;
use crate::path::PathMode;

impl LoweringContext<'_> {
    pub(crate) fn lower_exprs(&mut self, es: &[P<ast::Expr>]) -> Vec<hir::Expr> {
        es.iter().map(|e| self.lower_expr(e)).collect()
    }

    pub(crate) fn lower_expr(&mut self, e: &ast::Expr) -> hir::Expr {
        use ast::ExprKind as K;
        let kind = match &e.kind {
            K::Paren(inner) => {
                let mut ex = self.lower_expr(inner);
                self.lower_attrs(ex.hir_id, &e.attrs);
                ex.span = e.span;
                return ex;
            }
            K::ForLoop { pat, iter, body, label } => return self.lower_expr_for(e, pat, iter, body, *label),
            K::Path(qself, path) => return self.lower_expr_path(e, qself, path),
            K::Call(callee, args) => match &callee.kind {
                K::Path(qself, path) => return self.lower_expr_call_path(e, callee, qself, path, args),
                _ => hir::ExprKind::Call(Box::new(self.lower_expr(callee)), self.lower_exprs(args)),
            },

            K::Array(es) => hir::ExprKind::Array(self.lower_exprs(es)),
            K::Repeat(x, n) => hir::ExprKind::Repeat(Box::new(self.lower_expr(x)), self.lower_anon_const(n)),
            K::Vec(inner) => self.lower_expr_vec(inner, e.span),
            K::ConstBlock(c) => {
                let def_id = self.local_def_id(c.id);
                let hir_id = self.lower_node_id(c.id);
                let body = self.with_new_scopes(|this| {
                    let value = this.lower_expr(&c.value);
                    this.record_body(Vec::new(), value)
                });
                hir::ExprKind::ConstBlock(hir::ConstBlock { hir_id, def_id, body })
            }
            K::MethodCall(m) => {
                let seg = self.lower_path_segment(&m.seg, Res::Err);
                let receiver = self.lower_expr(&m.receiver);
                hir::ExprKind::MethodCall(Box::new(seg), Box::new(receiver), self.lower_exprs(&m.args), m.span)
            }
            K::Tup(es) => hir::ExprKind::Tup(self.lower_exprs(es)),
            K::Binary(op, a, b) => hir::ExprKind::Binary(*op, Box::new(self.lower_expr(a)), Box::new(self.lower_expr(b))),
            K::Unary(op, x) => hir::ExprKind::Unary(*op, Box::new(self.lower_expr(x))),
            K::Lit(lit) => hir::ExprKind::Lit(*lit),
            K::Cast(x, t) => hir::ExprKind::Cast(Box::new(self.lower_expr(x)), self.lower_ty(t)),
            K::Let(pat, scrutinee, span) => {
                let init = self.lower_expr(scrutinee);
                let pat = self.lower_pat(pat);
                hir::ExprKind::Let(Box::new(hir::LetExpr { span: *span, pat, ty: None, init }))
            }
            K::If(cond, then, els) => {
                let cond = self.lower_cond(cond);
                let then = self.lower_block_expr(then);
                let els = els.as_ref().map(|e| Box::new(self.lower_expr(e)));
                hir::ExprKind::If(Box::new(cond), Box::new(then), els)
            }
            K::While(cond, body, label) => return self.lower_expr_while(e, cond, body, *label),
            K::Loop(body, label, span) => {
                let loop_id = self.lower_node_id(e.id);
                let block = self.with_loop_scope(loop_id, |this| this.lower_block(body));
                let kind = hir::ExprKind::Loop(Box::new(block), label.map(lower_label), hir::LoopSource::Loop, *span);
                return self.finish_expr(e, loop_id, kind);
            }
            K::Match(scrutinee, arms) => {
                let scrutinee = self.lower_expr(scrutinee);
                let arms = arms.iter().map(|a| self.lower_arm(a)).collect();
                hir::ExprKind::Match(Box::new(scrutinee), arms, hir::MatchSource::Normal)
            }
            K::Closure(c) => {
                let kind = if c.asyncness.is_some() { hir::ClosureKind::AsyncClosure } else { hir::ClosureKind::Closure };
                let fn_decl = self.lower_fn_decl(&c.fn_decl);
                let body = self.lower_fn_body(&c.fn_decl, Some(&c.body), e.span);
                hir::ExprKind::Closure(Box::new(hir::Closure {
                    def_id: self.local_def_id(e.id),
                    capture_clause: c.capture_clause,
                    fn_decl,
                    body,
                    fn_decl_span: c.fn_decl_span,
                    kind,
                }))
            }
            K::Async(b) => {
                let body = self.with_new_scopes(|this| {
                    let value = this.lower_block_expr(b);
                    this.record_body(Vec::new(), value)
                });
                let fn_decl = hir::FnDecl {
                    inputs: Vec::new(),
                    output: hir::FnRetTy::DefaultReturn(b.span.shrink_to_lo()),
                    c_variadic: false,
                    implicit_self: hir::ImplicitSelfKind::None,
                };
                hir::ExprKind::Closure(Box::new(hir::Closure {
                    def_id: self.local_def_id(e.id),
                    capture_clause: ast::CaptureBy::Ref,
                    fn_decl,
                    body,
                    fn_decl_span: e.span.until(b.span),
                    kind: hir::ClosureKind::AsyncBlock,
                }))
            }
            K::Block(b, label) => hir::ExprKind::Block(Box::new(self.lower_block(b)), label.map(lower_label)),
            K::Await(x, _) => hir::ExprKind::Await(Box::new(self.lower_expr(x))),
            K::Assign(lhs, rhs, eq_span) => return self.lower_expr_assign(e, lhs, rhs, *eq_span),
            K::AssignOp(op, lhs, rhs) => {
                hir::ExprKind::AssignOp(*op, Box::new(self.lower_expr(lhs)), Box::new(self.lower_expr(rhs)))
            }
            K::Field(x, ident) => hir::ExprKind::Field(Box::new(self.lower_expr(x)), *ident),
            K::Index(x, i, span) => hir::ExprKind::Index(Box::new(self.lower_expr(x)), Box::new(self.lower_expr(i)), *span),
            K::Range(lo, hi, limits) => self.lower_expr_range(e.span, lo.as_deref(), hi.as_deref(), *limits),
            K::Underscore => {
                self.dcx()
                    .struct_span_err(e.span, "in expressions, `_` can only be used on the left-hand side of an assignment")
                    .with_span_label(e.span, "`_` not allowed here")
                    .emit();
                hir::ExprKind::Err
            }
            K::AddrOf(k, m, x) => hir::ExprKind::AddrOf(*k, *m, Box::new(self.lower_expr(x))),
            K::Break(label, value) => {
                let dest = self.lower_destination(e.id, *label);
                hir::ExprKind::Break(dest, value.as_ref().map(|v| Box::new(self.lower_expr(v))))
            }
            K::Continue(label) => hir::ExprKind::Continue(self.lower_destination(e.id, *label)),
            K::Ret(v) => hir::ExprKind::Ret(v.as_ref().map(|v| Box::new(self.lower_expr(v)))),
            K::InlineAsm(asm) => hir::ExprKind::InlineAsm(Box::new(self.lower_inline_asm(asm))),
            K::Builtin(b) => self.lower_builtin(b, e.span),
            K::Struct(s) => {
                let qpath = self.lower_qpath_ty(e.id, &s.qself, &s.path, "struct, variant or union type");
                let fields = s
                    .fields
                    .iter()
                    .map(|f| {
                        let hir_id = self.lower_node_id(f.id);
                        self.lower_attrs(hir_id, &f.attrs);
                        hir::ExprField { hir_id, ident: f.ident, expr: self.lower_expr(&f.expr), span: f.span, is_shorthand: f.is_shorthand }
                    })
                    .collect();
                let tail = match &s.rest {
                    ast::StructRest::Base(b) => hir::StructTailExpr::Base(Box::new(self.lower_expr(b))),
                    ast::StructRest::None => hir::StructTailExpr::None,
                };
                hir::ExprKind::Struct(qpath, fields, tail)
            }
            K::Try(x) => return self.lower_expr_try(e, x),
            K::Err => hir::ExprKind::Err,
        };
        let hir_id = self.lower_node_id(e.id);
        self.finish_expr(e, hir_id, kind)
    }

    fn finish_expr(&mut self, e: &ast::Expr, hir_id: HirId, kind: hir::ExprKind) -> hir::Expr {
        self.lower_attrs(hir_id, &e.attrs);
        hir::Expr { hir_id, kind, span: e.span }
    }

    fn lower_arm(&mut self, a: &ast::Arm) -> hir::Arm {
        let hir_id = self.lower_node_id(a.id);
        self.lower_attrs(hir_id, &a.attrs);
        let pat = self.lower_pat(&a.pat);
        let guard = a.guard.as_ref().map(|g| self.lower_cond(g));
        hir::Arm { hir_id, span: a.span, pat, guard, body: self.lower_expr(&a.body) }
    }

    /// An `if` or `while` condition. Its temporaries are dropped before the
    /// branch runs, except in a `let` chain, as in rustc.
    fn lower_cond(&mut self, cond: &ast::Expr) -> hir::Expr {
        fn has_let(e: &ast::Expr) -> bool {
            match &e.kind {
                ast::ExprKind::Binary(op, a, b) if op.node == ast::BinOpKind::And => has_let(a) || has_let(b),
                ast::ExprKind::Let(..) => true,
                ast::ExprKind::Paren(inner) => has_let(inner),
                _ => false,
            }
        }
        let lowered = self.lower_expr(cond);
        if has_let(cond) {
            lowered
        } else {
            let span = lowered.span;
            self.expr(span, hir::ExprKind::DropTemps(Box::new(lowered)))
        }
    }

    fn lower_destination(&mut self, id: ast::NodeId, label: Option<ast::Label>) -> hir::Destination {
        let target_id = match label {
            Some(_) => self.resolver.label_res_map.get(&id).map(|&target| self.lower_node_id(target)),
            None => self.loop_scope,
        };
        hir::Destination { label: label.map(lower_label), target_id }
    }

    // -----------------------------------------------------------------------
    // Paths (exception 7)
    // -----------------------------------------------------------------------

    fn lower_expr_path(&mut self, e: &ast::Expr, qself: &Option<P<ast::QSelf>>, path: &ast::Path) -> hir::Expr {
        let (qpath, consumed) = self.lower_qpath(e.id, qself, path, PathMode::Expr);
        let n = path.segments.len();
        if consumed == n {
            let hir_id = self.lower_node_id(e.id);
            return self.finish_expr(e, hir_id, hir::ExprKind::Path(qpath));
        }
        // The prefix, then a field for each remaining segment; the outermost
        // field access is the expression itself.
        let mut base = self.expr(qpath.span(), hir::ExprKind::Path(qpath));
        for seg in &path.segments[consumed..n - 1] {
            self.report_segment_args(seg, "a field");
            let span = base.span.to(seg.ident.span);
            base = self.expr(span, hir::ExprKind::Field(Box::new(base), seg.ident));
        }
        let last = &path.segments[n - 1];
        self.report_segment_args(last, "a field");
        let hir_id = self.lower_node_id(e.id);
        self.finish_expr(e, hir_id, hir::ExprKind::Field(Box::new(base), last.ident))
    }

    /// `callee(args)` with a path callee: a call, or a method call when the
    /// path's last segment names a method (exception 7).
    fn lower_expr_call_path(
        &mut self,
        e: &ast::Expr,
        callee: &ast::Expr,
        qself: &Option<P<ast::QSelf>>,
        path: &ast::Path,
        args: &[P<ast::Expr>],
    ) -> hir::Expr {
        let (qpath, consumed) = self.lower_qpath(callee.id, qself, path, PathMode::Expr);
        let n = path.segments.len();
        let kind = if consumed == n {
            let callee_id = self.lower_node_id(callee.id);
            self.lower_attrs(callee_id, &callee.attrs);
            let callee = hir::Expr { hir_id: callee_id, kind: hir::ExprKind::Path(qpath), span: callee.span };
            hir::ExprKind::Call(Box::new(callee), self.lower_exprs(args))
        } else {
            let mut receiver = self.expr(qpath.span(), hir::ExprKind::Path(qpath));
            for seg in &path.segments[consumed..n - 1] {
                self.report_segment_args(seg, "a field");
                let span = receiver.span.to(seg.ident.span);
                receiver = self.expr(span, hir::ExprKind::Field(Box::new(receiver), seg.ident));
            }
            let method = &path.segments[n - 1];
            let seg = self.lower_path_segment(method, Res::Err);
            let span = method.ident.span.to(e.span);
            hir::ExprKind::MethodCall(Box::new(seg), Box::new(receiver), self.lower_exprs(args), span)
        };
        let hir_id = self.lower_node_id(e.id);
        self.finish_expr(e, hir_id, kind)
    }

    fn report_segment_args(&mut self, seg: &ast::PathSegment, what: &str) {
        if let Some(args) = &seg.args {
            self.dcx()
                .struct_span_err(args.span(), format!("`{}` is {what}, which takes no generic arguments", seg.ident))
                .with_code(ErrCode(109))
                .with_span_label(args.span(), "remove these arguments")
                .emit();
        }
    }

    // -----------------------------------------------------------------------
    // Loops
    // -----------------------------------------------------------------------

    /// `while cond { body }` → `loop { if cond { body } else { break } }`.
    fn lower_expr_while(&mut self, e: &ast::Expr, cond: &ast::Expr, body: &ast::Block, label: Option<ast::Label>) -> hir::Expr {
        let loop_id = self.lower_node_id(e.id);
        let block = self.with_loop_scope(loop_id, |this| {
            let cond = this.lower_cond(cond);
            let then = this.lower_block_expr(body);
            let brk = this.expr_break(cond.span.shrink_to_hi(), loop_id);
            let if_expr = this.expr(cond.span.to(body.span), hir::ExprKind::If(Box::new(cond), Box::new(then), Some(Box::new(brk))));
            this.block_expr(if_expr)
        });
        let head = e.span.with_hi(cond.span.hi());
        let kind = hir::ExprKind::Loop(Box::new(block), label.map(lower_label), hir::LoopSource::While, head);
        self.finish_expr(e, loop_id, kind)
    }

    /// ```text
    /// match IntoIterator::into_iter(head) {
    ///     mut iter => loop {
    ///         match Iterator::next(&mut iter) {
    ///             None => break,
    ///             Some(pat) => body,
    ///         }
    ///     }
    /// }
    /// ```
    /// The loop takes the `for`'s id, so `break` and `continue` find it.
    fn lower_expr_for(
        &mut self,
        e: &ast::Expr,
        pat: &ast::Pat,
        head: &ast::Expr,
        body: &ast::Block,
        label: Option<ast::Label>,
    ) -> hir::Expr {
        let head_span = head.span;
        let head = self.lower_expr(head);
        let loop_id = self.lower_node_id(e.id);

        let iter_ident = Ident::new(Symbol::intern("iter"), head_span);
        let (iter_pat, iter_id) = self.pat_ident_binding(head_span, iter_ident, ast::Mutability::Mut);

        let loop_block = self.with_loop_scope(loop_id, |this| {
            // `None => break`
            let none_pat = this.pat_lang_item_variant(head_span, LangItem::OptionNone, Vec::new());
            let brk = this.expr_break(head_span, loop_id);
            let none_arm = this.arm(none_pat, brk);
            // `Some(pat) => body`
            let pat = this.lower_pat(pat);
            let some_pat = this.pat_lang_item_variant(pat.span, LangItem::OptionSome, vec![pat]);
            let body_expr = this.lower_block_expr(body);
            let some_arm = this.arm(some_pat, body_expr);
            // `Iterator::next(&mut iter)`
            let iter = this.expr_local(head_span, iter_ident, iter_id);
            let iter_ref = this.expr(head_span, hir::ExprKind::AddrOf(ast::BorrowKind::Ref, ast::Mutability::Mut, Box::new(iter)));
            let next = this.expr_call_lang_item_fn(head_span, LangItem::IteratorNext, vec![iter_ref]);
            let m = this.expr(head_span, hir::ExprKind::Match(Box::new(next), vec![none_arm, some_arm], hir::MatchSource::ForLoopDesugar));
            this.block_expr(m)
        });
        let loop_expr = hir::Expr {
            hir_id: loop_id,
            kind: hir::ExprKind::Loop(Box::new(loop_block), label.map(lower_label), hir::LoopSource::ForLoop, e.span.with_hi(head_span.hi())),
            span: e.span,
        };
        self.lower_attrs(loop_id, &e.attrs);

        let into_iter = self.expr_call_lang_item_fn(head_span, LangItem::IntoIterIntoIter, vec![head]);
        let iter_arm = self.arm(iter_pat, loop_expr);
        let m = self.expr(e.span, hir::ExprKind::Match(Box::new(into_iter), vec![iter_arm], hir::MatchSource::ForLoopDesugar));
        // Drop the iterator's temporaries when the loop ends, as rustc does.
        self.expr(e.span, hir::ExprKind::DropTemps(Box::new(m)))
    }

    // -----------------------------------------------------------------------
    // `?`
    // -----------------------------------------------------------------------

    /// ```text
    /// match Try::branch(x) {
    ///     ControlFlow::Continue(val) => val,
    ///     ControlFlow::Break(residual) => return FromResidual::from_residual(residual),
    /// }
    /// ```
    fn lower_expr_try(&mut self, e: &ast::Expr, x: &ast::Expr) -> hir::Expr {
        let span = e.span;
        let q_span = x.span.shrink_to_hi().to(span.shrink_to_hi());
        let x = self.lower_expr(x);
        let scrutinee = self.expr_call_lang_item_fn(q_span, LangItem::TryTraitBranch, vec![x]);

        let val_ident = Ident::new(Symbol::intern("val"), span);
        let (val_pat, val_id) = self.pat_ident_binding(span, val_ident, ast::Mutability::Not);
        let val_expr = self.expr_local(span, val_ident, val_id);
        let continue_pat = self.pat_lang_item_variant(span, LangItem::ControlFlowContinue, vec![val_pat]);
        let continue_arm = self.arm(continue_pat, val_expr);

        let residual_ident = Ident::new(Symbol::intern("residual"), q_span);
        let (residual_pat, residual_id) = self.pat_ident_binding(q_span, residual_ident, ast::Mutability::Not);
        let residual = self.expr_local(q_span, residual_ident, residual_id);
        let from_residual = self.expr_call_lang_item_fn(q_span, LangItem::TryTraitFromResidual, vec![residual]);
        let ret = self.expr(q_span, hir::ExprKind::Ret(Some(Box::new(from_residual))));
        let break_pat = self.pat_lang_item_variant(q_span, LangItem::ControlFlowBreak, vec![residual_pat]);
        let break_arm = self.arm(break_pat, ret);

        let kind = hir::ExprKind::Match(Box::new(scrutinee), vec![continue_arm, break_arm], hir::MatchSource::TryDesugar);
        let hir_id = self.lower_node_id(e.id);
        self.finish_expr(e, hir_id, kind)
    }

    // -----------------------------------------------------------------------
    // Ranges and `vec[…]`
    // -----------------------------------------------------------------------

    fn lower_expr_range(
        &mut self,
        span: Span,
        lo: Option<&ast::Expr>,
        hi: Option<&ast::Expr>,
        limits: ast::RangeLimits,
    ) -> hir::ExprKind {
        use ast::RangeLimits::*;
        let (item, fields): (LangItem, &[(&str, Option<&ast::Expr>)]) = match (lo, hi, limits) {
            (Some(a), Some(b), Closed) => {
                let args = vec![self.lower_expr(a), self.lower_expr(b)];
                let f = self.expr_lang_item_path(span, LangItem::RangeInclusiveNew);
                return hir::ExprKind::Call(Box::new(f), args);
            }
            (None, None, HalfOpen) => return hir::ExprKind::Path(hir::QPath::LangItem(LangItem::RangeFull, span)),
            (Some(_), Some(_), HalfOpen) => (LangItem::Range, &[("start", lo), ("end", hi)]),
            (Some(_), None, HalfOpen) => (LangItem::RangeFrom, &[("start", lo)]),
            (None, Some(_), HalfOpen) => (LangItem::RangeTo, &[("end", hi)]),
            (None, Some(_), Closed) => (LangItem::RangeToInclusive, &[("end", hi)]),
            (_, None, Closed) => {
                self.dcx()
                    .struct_span_err(span, "inclusive range with no end")
                    .with_code(ErrCode(586))
                    .with_help("use `..` instead of `..=` to make the range open")
                    .emit();
                return hir::ExprKind::Err;
            }
        };
        let fields = fields
            .iter()
            .map(|&(name, e)| {
                let expr = self.lower_expr(e.unwrap());
                let fspan = expr.span;
                hir::ExprField { hir_id: self.next_id(), ident: Ident::new(Symbol::intern(name), fspan), expr, span: fspan, is_shorthand: false }
            })
            .collect();
        hir::ExprKind::Struct(hir::QPath::LangItem(item, span), fields, hir::StructTailExpr::None)
    }

    /// `vec[]`, `vec[a, b]`, `vec[x; n]` (exception 12; Rust's `vec!`).
    fn lower_expr_vec(&mut self, inner: &ast::Expr, span: Span) -> hir::ExprKind {
        let (item, args) = match &inner.kind {
            ast::ExprKind::Array(es) if es.is_empty() => (LangItem::VecNew, Vec::new()),
            ast::ExprKind::Array(_) => (LangItem::VecFromArray, vec![self.lower_expr(inner)]),
            // The count is evaluated at run time, unlike an array's length.
            ast::ExprKind::Repeat(x, n) => (LangItem::VecFromElem, vec![self.lower_expr(x), self.lower_expr(&n.value)]),
            _ => (LangItem::VecFromArray, vec![self.lower_expr(inner)]),
        };
        let f = self.expr_lang_item_path(span, item);
        hir::ExprKind::Call(Box::new(f), args)
    }

    // -----------------------------------------------------------------------
    // Assignment
    // -----------------------------------------------------------------------

    /// `a = b`, or a destructuring assignment such as `(a, b) = (b, a)` or
    /// `_ = f()`, which becomes `{ let (lhs0, lhs1) = rhs; a = lhs0; b = lhs1 }`.
    fn lower_expr_assign(&mut self, e: &ast::Expr, lhs: &ast::Expr, rhs: &ast::Expr, eq_span: Span) -> hir::Expr {
        if !is_destructuring(lhs) {
            let kind = hir::ExprKind::Assign(Box::new(self.lower_expr(lhs)), Box::new(self.lower_expr(rhs)), eq_span);
            let hir_id = self.lower_node_id(e.id);
            return self.finish_expr(e, hir_id, kind);
        }
        let rhs = self.lower_expr(rhs);
        let mut assignments = Vec::new();
        let pat = self.destructure_assign(lhs, eq_span, &mut assignments);
        let let_stmt = hir::LetStmt { hir_id: self.next_id(), pat, ty: None, init: Some(rhs), els: None, span: e.span };
        let mut stmts = vec![hir::Stmt { hir_id: self.next_id(), kind: hir::StmtKind::Let(Box::new(let_stmt)), span: e.span }];
        stmts.extend(assignments);
        let block = hir::Block { hir_id: self.next_id(), stmts, expr: None, rules: hir::BlockCheckMode::Default, span: e.span };
        let hir_id = self.lower_node_id(e.id);
        self.finish_expr(e, hir_id, hir::ExprKind::Block(Box::new(block), None))
    }

    /// The pattern for one side of a destructuring assignment. Each place
    /// assigned gets a fresh binding and an assignment from it.
    fn destructure_assign(&mut self, lhs: &ast::Expr, eq_span: Span, out: &mut Vec<hir::Stmt>) -> hir::Pat {
        let kind = match &lhs.kind {
            ast::ExprKind::Paren(inner) => return self.destructure_assign(inner, eq_span, out),
            ast::ExprKind::Underscore => hir::PatKind::Wild,
            ast::ExprKind::Tup(es) => {
                let (pats, rest) = self.destructure_elements(es, eq_span, out);
                hir::PatKind::Tuple(pats, rest)
            }
            ast::ExprKind::Array(es) => {
                let (mut before, rest) = self.destructure_elements(es, eq_span, out);
                match rest {
                    Some(i) => {
                        let after = before.split_off(i);
                        let wild = hir::Pat { hir_id: self.next_id(), kind: hir::PatKind::Wild, span: lhs.span };
                        hir::PatKind::Slice(before, Some(Box::new(wild)), after)
                    }
                    None => hir::PatKind::Slice(before, None, Vec::new()),
                }
            }
            _ => {
                // A place: `lhs = <fresh binding>`.
                let ident = Ident::new(Symbol::intern(&format!("lhs{}", out.len())), lhs.span);
                let (pat, id) = self.pat_ident_binding(lhs.span, ident, ast::Mutability::Not);
                let place = self.lower_expr(lhs);
                let value = self.expr_local(lhs.span, ident, id);
                let assign = self.expr(lhs.span, hir::ExprKind::Assign(Box::new(place), Box::new(value), eq_span));
                out.push(hir::Stmt { hir_id: self.next_id(), kind: hir::StmtKind::Semi(assign), span: lhs.span });
                return pat;
            }
        };
        hir::Pat { hir_id: self.next_id(), kind, span: lhs.span }
    }

    fn destructure_elements(&mut self, es: &[P<ast::Expr>], eq_span: Span, out: &mut Vec<hir::Stmt>) -> (Vec<hir::Pat>, Option<usize>) {
        let mut pats = Vec::new();
        let mut rest = None;
        for e in es {
            if matches!(e.kind, ast::ExprKind::Range(None, None, ast::RangeLimits::HalfOpen)) {
                if rest.is_some() {
                    self.dcx().struct_span_err(e.span, "`..` can only be used once per tuple pattern").emit();
                }
                rest = Some(pats.len());
                continue;
            }
            pats.push(self.destructure_assign(e, eq_span, out));
        }
        (pats, rest)
    }

    // -----------------------------------------------------------------------
    // Helpers for building HIR
    // -----------------------------------------------------------------------

    pub(crate) fn expr(&mut self, span: Span, kind: hir::ExprKind) -> hir::Expr {
        hir::Expr { hir_id: self.next_id(), kind, span }
    }

    pub(crate) fn expr_err(&mut self, span: Span) -> hir::Expr {
        self.expr(span, hir::ExprKind::Err)
    }

    pub(crate) fn expr_block(&mut self, block: hir::Block) -> hir::Expr {
        let span = block.span;
        self.expr(span, hir::ExprKind::Block(Box::new(block), None))
    }

    /// A block holding just `expr` as its value.
    pub(crate) fn block_expr(&mut self, expr: hir::Expr) -> hir::Block {
        let span = expr.span;
        hir::Block { hir_id: self.next_id(), stmts: Vec::new(), expr: Some(Box::new(expr)), rules: hir::BlockCheckMode::Default, span }
    }

    fn expr_break(&mut self, span: Span, target: HirId) -> hir::Expr {
        self.expr(span, hir::ExprKind::Break(hir::Destination { label: None, target_id: Some(target) }, None))
    }

    pub(crate) fn expr_lang_item_path(&mut self, span: Span, item: LangItem) -> hir::Expr {
        self.expr(span, hir::ExprKind::Path(hir::QPath::LangItem(item, span)))
    }

    pub(crate) fn expr_call_lang_item_fn(&mut self, span: Span, item: LangItem, args: Vec<hir::Expr>) -> hir::Expr {
        let f = self.expr_lang_item_path(span, item);
        self.expr(span, hir::ExprKind::Call(Box::new(f), args))
    }

    pub(crate) fn expr_call(&mut self, span: Span, f: hir::Expr, args: Vec<hir::Expr>) -> hir::Expr {
        self.expr(span, hir::ExprKind::Call(Box::new(f), args))
    }

    /// `Ty.name`, where `Ty` is a lang item type: `FormatArgument.newDisplay`.
    pub(crate) fn expr_lang_item_type_relative(&mut self, span: Span, item: LangItem, name: &str) -> hir::Expr {
        let ty = hir::Ty { hir_id: self.next_id(), kind: hir::TyKind::Path(hir::QPath::LangItem(item, span)), span };
        let seg = hir::PathSegment { ident: Ident::new(Symbol::intern(name), span), hir_id: self.next_id(), res: Res::Err, args: None };
        self.expr(span, hir::ExprKind::Path(hir::QPath::TypeRelative(Box::new(ty), seg)))
    }

    /// A use of a binding made by a desugaring.
    pub(crate) fn expr_local(&mut self, span: Span, ident: Ident, binding: HirId) -> hir::Expr {
        let res = Res::Local(binding);
        let seg = hir::PathSegment { ident, hir_id: self.next_id(), res, args: None };
        let path = hir::Path { span, res, segments: vec![seg] };
        self.expr(span, hir::ExprKind::Path(hir::QPath::Resolved(None, path)))
    }

    pub(crate) fn expr_addr_of(&mut self, span: Span, e: hir::Expr) -> hir::Expr {
        self.expr(span, hir::ExprKind::AddrOf(ast::BorrowKind::Ref, ast::Mutability::Not, Box::new(e)))
    }

    pub(crate) fn expr_str(&mut self, span: Span, s: &str) -> hir::Expr {
        let lit = Lit::new(LitKind::Str, Symbol::intern(&escape_str(s)), None);
        self.expr(span, hir::ExprKind::Lit(lit))
    }

    pub(crate) fn expr_uint(&mut self, span: Span, n: usize) -> hir::Expr {
        let lit = Lit::new(LitKind::Integer, Symbol::intern(&n.to_string()), None);
        self.expr(span, hir::ExprKind::Lit(lit))
    }

    pub(crate) fn expr_char(&mut self, span: Span, c: char) -> hir::Expr {
        let text: String = match c {
            '\'' => "\\'".to_string(),
            _ => c.escape_debug().collect(),
        };
        self.expr(span, hir::ExprKind::Lit(Lit::new(LitKind::Char, Symbol::intern(&text), None)))
    }

    /// A new binding made by a desugaring: its pattern and its id.
    pub(crate) fn pat_ident_binding(&mut self, span: Span, ident: Ident, mutbl: ast::Mutability) -> (hir::Pat, HirId) {
        let hir_id = self.next_id();
        let kind = hir::PatKind::Binding(hir::BindingMode(ast::ByRef::No, mutbl), hir_id, ident, None);
        (hir::Pat { hir_id, kind, span }, hir_id)
    }

    /// `Some(p)`, `None`, `Continue(p)`: a lang item variant pattern.
    pub(crate) fn pat_lang_item_variant(&mut self, span: Span, item: LangItem, subpats: Vec<hir::Pat>) -> hir::Pat {
        let qpath = hir::QPath::LangItem(item, span);
        let kind = if subpats.is_empty() { hir::PatKind::Path(qpath) } else { hir::PatKind::TupleStruct(qpath, subpats, None) };
        hir::Pat { hir_id: self.next_id(), kind, span }
    }

    pub(crate) fn arm(&mut self, pat: hir::Pat, body: hir::Expr) -> hir::Arm {
        let span = pat.span.to(body.span);
        hir::Arm { hir_id: self.next_id(), span, pat, guard: None, body }
    }

    pub(crate) fn binop(span: Span, op: ast::BinOpKind) -> hir::BinOp {
        Spanned { node: op, span }
    }
}

fn lower_label(l: ast::Label) -> hir::Label {
    hir::Label { ident: l.ident }
}

/// Is the left side of `=` a pattern to destructure, rather than a place?
fn is_destructuring(lhs: &ast::Expr) -> bool {
    match &lhs.kind {
        ast::ExprKind::Underscore | ast::ExprKind::Tup(_) | ast::ExprKind::Array(_) => true,
        ast::ExprKind::Paren(inner) => is_destructuring(inner),
        _ => false,
    }
}

/// The text of a string literal holding `s`.
pub(crate) fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out
}
