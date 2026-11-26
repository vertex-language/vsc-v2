//! Checking unsafe operations (rustc's `check_unsafety.rs`): calling an
//! unsafe function, dereferencing a raw pointer, using a `static var` or an
//! extern static, reading a union field, inline assembly. Each needs an
//! `unsafe` block. In an `unsafe func`, they're warned about, as in Rust
//! 2024 (`unsafe_op_in_unsafe_fn`): an unsafe function restricts its
//! callers, but its body is safe code.

use vsc_errors::ErrCode;
use vsc_hir::def::DefKind;
use vsc_middle::thir::visit::{self, Visitor};
use vsc_middle::thir::*;
use vsc_middle::ty::{AdtKind, Safety, TyCtxt, TyKind};
use vsc_span::Span;
use vsc_span::def_id::DefId;

pub fn check_unsafety(tcx: TyCtxt<'_>, def: DefId) {
    let in_unsafe_fn = matches!(tcx.def_kind(def), DefKind::Fn | DefKind::AssocFn) && tcx.fn_sig(def).skip_binder().safety == Safety::Unsafe;
    let thir = tcx.thir_body(def);
    let mut v = UnsafetyVisitor { tcx, thir, in_unsafe_block: false, in_unsafe_fn, assign_lhs: None };
    v.check_body();
}

struct UnsafetyVisitor<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    thir: &'a Thir<'tcx>,
    in_unsafe_block: bool,
    in_unsafe_fn: bool,
    /// The left side of the assignment being checked: writing a union
    /// field is safe.
    assign_lhs: Option<&'a Expr<'tcx>>,
}

impl<'a, 'tcx> UnsafetyVisitor<'a, 'tcx> {
    fn check_body(&mut self) {
        if let Some(v) = self.thir.value {
            self.visit_expr(&self.thir.exprs[v]);
        }
    }

    fn report(&self, span: Span, op: &str, note: &str) {
        if self.in_unsafe_block {
            return;
        }
        let msg = format!("{op} is unsafe and requires unsafe block");
        if self.in_unsafe_fn {
            self.tcx
                .dcx()
                .struct_span_warn(span, msg)
                .with_code(ErrCode(133))
                .with_span_label(span, op.to_string())
                .with_note(note.to_string())
                .with_note("an unsafe function restricts its callers, but its body is safe by default")
                .emit();
        } else {
            self.tcx.dcx().struct_span_err(span, msg).with_code(ErrCode(133)).with_span_label(span, op.to_string()).with_note(note.to_string()).emit();
        }
    }
}

impl<'a, 'tcx> Visitor<'a, 'tcx> for UnsafetyVisitor<'a, 'tcx> {
    fn thir(&self) -> &'a Thir<'tcx> {
        self.thir
    }

    fn visit_block(&mut self, b: &'a Block) {
        let saved = self.in_unsafe_block;
        if let BlockSafety::ExplicitUnsafe(_) = b.safety_mode {
            self.in_unsafe_block = true;
        }
        visit::walk_block(self, b);
        self.in_unsafe_block = saved;
    }

    fn visit_expr(&mut self, e: &'a Expr<'tcx>) {
        let tcx = self.tcx;
        match &e.kind {
            ExprKind::Call { fun, .. } => match *self.thir.exprs[*fun].ty.kind() {
                TyKind::FnDef(d, _) if tcx.fn_sig(d).skip_binder().safety == Safety::Unsafe => self.report(
                    e.span,
                    &format!("call to unsafe function `{}`", tcx.def_path_str(d)),
                    "consult the function's documentation for information on how to avoid undefined behavior",
                ),
                TyKind::FnPtr(sig) if sig.value.safety == Safety::Unsafe => self.report(
                    e.span,
                    "call to unsafe function",
                    "consult the function's documentation for information on how to avoid undefined behavior",
                ),
                _ => {}
            },
            ExprKind::Deref { arg } if matches!(self.thir.exprs[*arg].ty.kind(), TyKind::RawPtr(..)) => self.report(
                e.span,
                "dereference of raw pointer",
                "raw pointers may be null, dangling or unaligned; they can violate aliasing rules and cause data races: all of these are undefined behavior",
            ),
            ExprKind::StaticRef { def_id } => {
                let foreign = tcx.opt_parent(*def_id).is_some_and(|p| tcx.def_kind(p) == DefKind::ForeignMod);
                if foreign {
                    self.report(
                        e.span,
                        "use of extern static",
                        "extern statics are not controlled by the type system: invalid data, aliasing violations or data races will cause undefined behavior",
                    );
                } else if matches!(tcx.def_kind(*def_id), DefKind::Static { mutability } if mutability.is_mut()) {
                    self.report(
                        e.span,
                        "use of mutable static",
                        "mutable statics can be mutated by multiple threads: aliasing violations or data races will cause undefined behavior",
                    );
                }
            }
            ExprKind::Field { lhs, .. } if !self.assign_lhs.is_some_and(|l| std::ptr::eq(l, e)) => {
                if matches!(self.thir.exprs[*lhs].ty.kind(), TyKind::Adt(d, _) if d.kind() == AdtKind::Union) {
                    self.report(
                        e.span,
                        "access to union field",
                        "the field may not be properly initialized: using uninitialized data will cause undefined behavior",
                    );
                }
            }
            ExprKind::InlineAsm { .. } => self.report(e.span, "use of inline assembly", "inline assembly is entirely unchecked and can cause undefined behavior"),
            ExprKind::Assign { lhs, rhs } => {
                let saved = self.assign_lhs.replace(&self.thir.exprs[*lhs]);
                self.visit_expr(&self.thir.exprs[*lhs]);
                self.assign_lhs = saved;
                self.visit_expr(&self.thir.exprs[*rhs]);
                return;
            }
            ExprKind::Closure { closure_id } => {
                // A function literal is checked in the context it's written in.
                let inner = tcx.thir_body(closure_id.to_def_id());
                let mut v = UnsafetyVisitor { tcx, thir: inner, in_unsafe_block: self.in_unsafe_block, in_unsafe_fn: self.in_unsafe_fn, assign_lhs: None };
                v.check_body();
            }
            _ => {}
        }
        visit::walk_expr(self, e);
    }
}
