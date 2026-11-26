//! Printing HIR (rustc's `rustc_hir_pretty`), for `-Z unpretty=hir`.
//!
//! HIR has no Vertex spellings left, so it prints in Rust's syntax, as
//! rustc's printer does: `fn`, `::`, `'a` for lifetimes, `#[lang = "…"]` for
//! lang item paths. Desugarings show as what they became. The few things
//! Rust has no syntax for keep Vertex's: `class` and `func(T) -> U`
//! function value types.

mod expr;
mod item;
mod ty_pat;

use vsc_ast::BinOpKind;
use vsc_hir::*;

/// The whole crate, from its root package.
pub fn crate_to_string(krate: &Crate) -> String {
    let mut p = Printer { krate, out: String::new(), indent: 0 };
    let root = krate.owner(vsc_span::def_id::CRATE_DEF_ID);
    p.owner_attrs(root);
    if let OwnerNode::Crate(m) = &root.node {
        p.mod_items(m);
    }
    while p.out.ends_with("\n\n") {
        p.out.pop();
    }
    p.out
}

struct Printer<'a> {
    krate: &'a Crate,
    out: String,
    indent: usize,
}

/// Binary operator precedence, as in Rust: higher binds tighter.
fn prec(op: BinOpKind) -> u8 {
    use BinOpKind::*;
    match op {
        Mul | Div | Rem => 10,
        Add | Sub => 9,
        Shl | Shr => 8,
        BitAnd => 7,
        BitXor => 6,
        BitOr => 5,
        Eq | Lt | Le | Ne | Ge | Gt => 4,
        And => 3,
        Or => 2,
    }
}

const PREC_PREFIX: u8 = 12;
const PREC_POSTFIX: u8 = 13;
/// Assignments, closures, `return`, `break`, ranges: anything that must be
/// parenthesized as an operand.
const PREC_LOWEST: u8 = 0;

fn expr_prec(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Binary(op, ..) => prec(op.node),
        ExprKind::Cast(..) => 11,
        ExprKind::Unary(..) | ExprKind::AddrOf(..) => PREC_PREFIX,
        ExprKind::Assign(..)
        | ExprKind::AssignOp(..)
        | ExprKind::Closure(..)
        | ExprKind::Ret(..)
        | ExprKind::Break(..)
        | ExprKind::Let(..) => PREC_LOWEST,
        _ => PREC_POSTFIX,
    }
}

impl<'a> Printer<'a> {
    fn w(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn nl(&mut self) {
        self.out.push('\n');
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
    }

    fn comma_sep<T>(&mut self, items: &[T], mut f: impl FnMut(&mut Self, &T)) {
        for (i, x) in items.iter().enumerate() {
            if i > 0 {
                self.w(", ");
            }
            f(self, x);
        }
    }

    fn owner_attrs(&mut self, owner: &OwnerInfo) {
        if let Some(attrs) = owner.attrs.get(&ItemLocalId::ZERO) {
            for a in attrs {
                self.w(&vsc_ast_pretty::attribute_to_string(a));
                self.nl();
            }
        }
    }

    /// Attributes on a node inside an owner.
    fn attrs(&mut self, id: HirId) {
        let owner = self.krate.owner(id.owner);
        if id.local_id == ItemLocalId::ZERO {
            return;
        }
        if let Some(attrs) = owner.attrs.get(&id.local_id) {
            for a in attrs {
                self.w(&vsc_ast_pretty::attribute_to_string(a));
                self.nl();
            }
        }
    }

}
