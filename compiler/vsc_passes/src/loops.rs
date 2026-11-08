//! Checking `break` and `continue` (rustc's `loops.rs`).
//!
//! Lowering found each one's target: the loop or block its label names, or
//! the innermost loop. What's left to check is where there was none, and
//! whether the jump makes sense for its target.

use vsc_data_structures::FxHashMap;
use vsc_errors::ErrCode;
use vsc_hir::intravisit::{self, Visitor};
use vsc_hir::*;
use vsc_middle::ty::TyCtxt;
use vsc_span::def_id::CRATE_DEF_ID;

pub fn check_crate(tcx: TyCtxt<'_>) {
    let mut v = CheckLoops { tcx, cx: Vec::new(), targets: FxHashMap::default() };
    if let OwnerNode::Crate(m) = &tcx.hir_crate.owner(CRATE_DEF_ID).node {
        v.visit_mod(m);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Context {
    Loop(LoopSource),
    /// A function literal: `break` can't leave it.
    Closure,
    AsyncBlock,
    /// A constant's value, or an item's body.
    Normal,
}

#[derive(Clone, Copy)]
enum Target {
    Loop(LoopSource),
    Block,
}

struct CheckLoops<'tcx> {
    tcx: TyCtxt<'tcx>,
    cx: Vec<Context>,
    targets: FxHashMap<HirId, Target>,
}

impl<'tcx> CheckLoops<'tcx> {
    fn with(&mut self, cx: Context, f: impl FnOnce(&mut Self)) {
        self.cx.push(cx);
        f(self);
        self.cx.pop();
    }

    fn report_no_target(&self, e: &Expr, kw: &str) {
        let span = e.span;
        match self.cx.last() {
            Some(Context::Closure) => {
                self.tcx
                    .dcx()
                    .struct_span_err(span, format!("`{kw}` inside of a function literal"))
                    .with_code(ErrCode(267))
                    .with_span_label(span, format!("cannot `{kw}` inside of a function literal"))
                    .emit();
            }
            Some(Context::AsyncBlock) => {
                self.tcx
                    .dcx()
                    .struct_span_err(span, format!("`{kw}` inside `async` block"))
                    .with_code(ErrCode(267))
                    .with_span_label(span, format!("cannot `{kw}` inside `async` block"))
                    .emit();
            }
            _ => {
                let what = if kw == "break" { "a loop or labeled block" } else { "a loop" };
                self.tcx
                    .dcx()
                    .struct_span_err(span, format!("`{kw}` outside of {what}"))
                    .with_code(ErrCode(268))
                    .with_span_label(span, format!("cannot `{kw}` outside of {what}"))
                    .emit();
            }
        }
    }
}

impl<'tcx> Visitor<'tcx> for CheckLoops<'tcx> {
    fn krate(&self) -> Option<&'tcx Crate> {
        Some(self.tcx.hir_crate)
    }

    fn visit_body(&mut self, b: &'tcx Body) {
        // Bodies reached directly (items, constants) start fresh; closures
        // push their own context first.
        let cx = match self.cx.last() {
            Some(Context::Closure) | Some(Context::AsyncBlock) => *self.cx.last().unwrap(),
            _ => Context::Normal,
        };
        self.with(cx, |this| intravisit::walk_body(this, b));
    }

    fn visit_anon_const(&mut self, c: &'tcx AnonConst) {
        self.with(Context::Normal, |this| intravisit::walk_anon_const(this, c));
    }

    fn visit_expr(&mut self, e: &'tcx Expr) {
        match &e.kind {
            ExprKind::Loop(b, _, source, _) => {
                self.targets.insert(e.hir_id, Target::Loop(*source));
                self.with(Context::Loop(*source), |this| this.visit_block(b));
            }
            ExprKind::Block(b, Some(_)) => {
                self.targets.insert(e.hir_id, Target::Block);
                intravisit::walk_block(self, b);
            }
            ExprKind::Closure(c) => {
                let cx = if c.kind == ClosureKind::AsyncBlock { Context::AsyncBlock } else { Context::Closure };
                self.with(cx, |this| intravisit::walk_expr(this, e));
            }
            ExprKind::Break(dest, value) => {
                match dest.target_id.and_then(|t| self.targets.get(&t).copied()) {
                    None if dest.label.is_none() => self.report_no_target(e, "break"),
                    // A label that didn't resolve was reported already.
                    None => {}
                    Some(Target::Loop(source @ (LoopSource::While | LoopSource::ForLoop))) if value.is_some() => {
                        let kind = if source == LoopSource::While { "while" } else { "for" };
                        self.tcx
                            .dcx()
                            .struct_span_err(e.span, format!("`break` with value from a `{kind}` loop"))
                            .with_code(ErrCode(571))
                            .with_span_label(e.span, format!("can only break with a value inside `loop` or breakable block"))
                            .emit();
                    }
                    Some(_) => {}
                }
                intravisit::walk_expr(self, e);
            }
            ExprKind::Continue(dest) => {
                match dest.target_id.and_then(|t| self.targets.get(&t).copied()) {
                    None if dest.label.is_none() => self.report_no_target(e, "continue"),
                    None => {}
                    Some(Target::Block) => {
                        self.tcx
                            .dcx()
                            .struct_span_err(e.span, "`continue` pointing to a labeled block")
                            .with_code(ErrCode(696))
                            .with_span_label(e.span, "labeled blocks cannot be `continue`d")
                            .emit();
                    }
                    Some(Target::Loop(_)) => {}
                }
            }
            _ => intravisit::walk_expr(self, e),
        }
    }
}
