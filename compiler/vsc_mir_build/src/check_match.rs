//! Checking patterns (rustc's `thir/pattern/check_match.rs`): a `match`
//! must be exhaustive (E0004) and its arms reachable, and `let`, function
//! parameters and `for` bindings must be irrefutable (E0005).

use vsc_errors::ErrCode;
use vsc_middle::thir::visit::{self, Visitor};
use vsc_middle::thir::*;
use vsc_middle::ty::TyCtxt;
use vsc_pattern_analysis::{DPat, MatchCx, Witness};
use vsc_span::def_id::DefId;

pub fn check_match(tcx: TyCtxt<'_>, def: DefId) {
    check_body(tcx, def, "function argument");
}

fn check_body(tcx: TyCtxt<'_>, def: DefId, param_what: &str) {
    let thir = tcx.thir_body(def);
    let mut v = MatchVisitor { tcx, thir, cx: MatchCx::new(tcx) };
    for p in &thir.params {
        if let Some(pat) = &p.pat {
            v.check_irrefutable(pat, param_what, false);
        }
    }
    if let Some(value) = thir.value {
        v.visit_expr(&thir.exprs[value]);
    }
}

struct MatchVisitor<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    thir: &'a Thir<'tcx>,
    cx: MatchCx<'tcx>,
}

/// "`a`", "`a` and `b`", "`a`, `b` and `c`", "`a`, `b`, `c` and more".
fn joined(ws: &[String]) -> String {
    let q: Vec<String> = ws.iter().map(|w| format!("`{w}`")).collect();
    match q.len() {
        0 => String::new(),
        1 => q[0].clone(),
        2 | 3 => format!("{} and {}", q[..q.len() - 1].join(", "), q[q.len() - 1]),
        _ => format!("{} and more", q[..3].join(", ")),
    }
}

impl<'a, 'tcx> MatchVisitor<'a, 'tcx> {
    fn witness_strings(&self, ws: &[Witness<'tcx>]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for w in ws {
            let s = self.cx.witness_to_string(w);
            if !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }

    /// The values `pat` doesn't match, if any.
    fn refutable_witnesses(&self, pat: &Pat<'tcx>) -> Vec<String> {
        let d = self.cx.deconstruct(pat);
        let report = self.cx.analyze(&[(d, false)], pat.ty);
        self.witness_strings(&report.missing)
    }

    fn check_irrefutable(&self, pat: &Pat<'tcx>, what: &str, is_let: bool) {
        let missing = self.refutable_witnesses(pat);
        if missing.is_empty() {
            return;
        }
        let what_pattern = if missing.len() == 1 { "pattern" } else { "patterns" };
        let mut err = self
            .tcx
            .dcx()
            .struct_span_err(pat.span, format!("refutable pattern in {what}"))
            .with_code(ErrCode(5))
            .with_span_label(pat.span, format!("{what_pattern} {} not covered", joined(&missing)));
        if is_let {
            err = err
                .with_note("`let` bindings require an \"irrefutable pattern\", like a `struct` or an `enum` with only one variant")
                .with_help("you might want to use `let else` to handle the values that aren't matched");
        }
        err.with_note(format!("the matched value is of type `{}`", self.tcx.ty_to_string(pat.ty))).emit();
    }

    fn check_match(&self, scrutinee: ExprId, arms: &[ArmId], source: MatchSource, span: vsc_span::Span) {
        let tcx = self.tcx;
        match source {
            MatchSource::Normal => {}
            // `match next(&mut iter) { None => break, Some(pat) => … }`:
            // the binding must be irrefutable.
            MatchSource::ForLoop => {
                if arms.len() == 2 {
                    if let PatKind::Variant { subpatterns, .. } = &self.thir.arms[arms[1]].pattern.kind {
                        if let Some(sub) = subpatterns.first() {
                            self.check_irrefutable(&sub.pattern, "`for` loop binding", false);
                        }
                    }
                }
                return;
            }
            MatchSource::Try | MatchSource::Desugared => return,
        }
        let scrut = &self.thir.exprs[scrutinee];
        let rows: Vec<(DPat<'tcx>, bool)> =
            arms.iter().map(|&a| (self.cx.deconstruct(&self.thir.arms[a].pattern), self.thir.arms[a].guard.is_some())).collect();
        let report = self.cx.analyze(&rows, scrut.ty);
        for i in report.unreachable {
            let pat_span = self.thir.arms[arms[i]].pattern.span;
            tcx.dcx().struct_span_warn(pat_span, "unreachable pattern").with_span_label(pat_span, "no value can reach this").emit();
        }
        let missing = self.witness_strings(&report.missing);
        if !missing.is_empty() {
            let what_pattern = if missing.len() == 1 { "pattern" } else { "patterns" };
            let _ = span;
            tcx.dcx()
                .struct_span_err(scrut.span, format!("non-exhaustive patterns: {} not covered", joined(&missing)))
                .with_code(ErrCode(4))
                .with_span_label(scrut.span, format!("{what_pattern} {} not covered", joined(&missing)))
                .with_note(format!("the matched value is of type `{}`", tcx.ty_to_string(scrut.ty)))
                .with_help("ensure that all possible cases are being handled by adding a match arm for each value shown, or a wildcard arm `_ => …`")
                .emit();
        }
    }
}

impl<'a, 'tcx> Visitor<'a, 'tcx> for MatchVisitor<'a, 'tcx> {
    fn thir(&self) -> &'a Thir<'tcx> {
        self.thir
    }

    fn visit_expr(&mut self, e: &'a Expr<'tcx>) {
        match &e.kind {
            ExprKind::Match { scrutinee, arms, source } => self.check_match(*scrutinee, arms, *source, e.span),
            ExprKind::Closure { closure_id } => check_body(self.tcx, closure_id.to_def_id(), "function argument"),
            _ => {}
        }
        visit::walk_expr(self, e);
    }

    fn visit_stmt(&mut self, s: &'a Stmt<'tcx>) {
        if let StmtKind::Let { pattern, else_block: None, .. } = &s.kind {
            self.check_irrefutable(pattern, "local binding", true);
        }
        visit::walk_stmt(self, s);
    }
}

