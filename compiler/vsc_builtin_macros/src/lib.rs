//! The built-ins (rustc's `rustc_builtin_macros`; exception 12).
//!
//! In Vertex the built-ins are calls without `!`: `println("{x}")`. Their
//! names are reserved, so a call to one is recognized by its name. Expansion
//! turns each into an `ExprKind::Builtin` with its format string checked, as
//! `format_args!` does in Rust.
//!
//! `@derive` and `@test` come here too once the standard library exists to
//! name their traits and harness.

mod format;

pub use format::parse_format_args;

use vsc_ast::mut_visit::{self, MutVisitor};
use vsc_ast::*;
use vsc_session::ParseSess;
use vsc_span::{Span, Symbol, sym};

/// Expands every built-in call in the crate.
pub fn expand_builtins(psess: &ParseSess, krate: &mut Crate) {
    BuiltinExpander { psess }.visit_crate(krate);
}

struct BuiltinExpander<'a> {
    psess: &'a ParseSess,
}

impl MutVisitor for BuiltinExpander<'_> {
    fn visit_expr(&mut self, e: &mut Expr) {
        mut_visit::walk_expr(self, e);
        let ExprKind::Call(callee, _) = &e.kind else { return };
        let ExprKind::Path(None, path) = &callee.kind else { return };
        let Some(name) = path.as_ident() else { return };
        if !is_format_builtin(name.name) {
            return;
        }
        let ExprKind::Call(_, args) = std::mem::replace(&mut e.kind, ExprKind::Err) else { unreachable!() };
        e.kind = match expand(self.psess, name.name, name.span, e.span, args) {
            Some(kind) => ExprKind::Builtin(P::new(BuiltinCall { kind, name_span: name.span })),
            None => ExprKind::Err,
        };
    }
}

fn is_format_builtin(name: Symbol) -> bool {
    [
        sym::print,
        sym::println,
        sym::eprint,
        sym::eprintln,
        sym::format,
        sym::panic,
        sym::todo,
        sym::unreachable,
        sym::assert,
        sym::assertEq,
    ]
    .contains(&name)
}

/// The built-in call `name(args)`, checked. `None` after an error.
fn expand(psess: &ParseSess, name: Symbol, name_span: Span, span: Span, mut args: Vec<P<Expr>>) -> Option<BuiltinKind> {
    let dcx = &psess.dcx;
    let needs_args = |n: usize, what: &str| {
        dcx.struct_span_err(span, format!("`{name}` requires {what}"))
            .with_span_label(name_span, format!("expects at least {n} argument{}", if n == 1 { "" } else { "s" }))
            .emit();
        None
    };
    // An optional message: nothing, or a format string and its arguments.
    let optional_msg = |args: Vec<P<Expr>>| -> Option<Option<P<FormatArgs>>> {
        if args.is_empty() { Some(None) } else { parse_format_args(psess, span, args).map(Some) }
    };
    Some(if name == sym::print || name == sym::println || name == sym::eprint || name == sym::eprintln {
        let newline = name == sym::println || name == sym::eprintln;
        let stderr = name == sym::eprint || name == sym::eprintln;
        let args = if args.is_empty() {
            if !newline {
                return needs_args(1, "at least a format string argument");
            }
            P::new(empty_format_args(span))
        } else {
            parse_format_args(psess, span, args)?
        };
        BuiltinKind::Print { newline, stderr, args }
    } else if name == sym::format {
        if args.is_empty() {
            return needs_args(1, "at least a format string argument");
        }
        BuiltinKind::Format(parse_format_args(psess, span, args)?)
    } else if name == sym::panic {
        BuiltinKind::Panic(optional_msg(args)?)
    } else if name == sym::todo {
        BuiltinKind::Todo(optional_msg(args)?)
    } else if name == sym::unreachable {
        BuiltinKind::Unreachable(optional_msg(args)?)
    } else if name == sym::assert {
        if args.is_empty() {
            return needs_args(1, "a boolean expression");
        }
        let cond = args.remove(0);
        BuiltinKind::Assert { cond, msg: optional_msg(args)? }
    } else if name == sym::assertEq {
        if args.len() < 2 {
            return needs_args(2, "two values to compare");
        }
        let left = args.remove(0);
        let right = args.remove(0);
        BuiltinKind::AssertEq { left, right, msg: optional_msg(args)? }
    } else {
        unreachable!("not a format built-in: {name}")
    })
}

/// `println()`: just a line feed.
fn empty_format_args(span: Span) -> FormatArgs {
    FormatArgs {
        span,
        lit: token::Lit::new(token::LitKind::Str, vsc_span::kw::Empty, None),
        lit_span: span.shrink_to_hi(),
        template: Vec::new(),
        arguments: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
