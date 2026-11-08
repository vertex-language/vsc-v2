//! Lowering the built-ins and their format strings (rustc's `format.rs`;
//! exception 12).
//!
//! `vsc_builtin_macros` has already parsed and checked each format string
//! into a `FormatArgs`. Here it becomes a value of the `formatArguments`
//! lang item, built the way rustc builds `fmt::Arguments`:
//!
//! ```text
//! println("{} is {:?}", a, b)
//! → print(FormatArguments.newV1(&["", " is ", "\n"],
//!       &[FormatArgument.newDisplay(&a), FormatArgument.newDebug(&b)]))
//! ```
//!
//! When an argument is used more than once or out of order, the arguments
//! are evaluated once, in order, by a `match` on a tuple of references.
//! When a placeholder has options (width, precision, flags, …), each one is
//! described by a `formatPlaceholder` and `newV1Formatted` is used.

use vsc_ast::{self as ast, FormatArgs, FormatArgsPiece, FormatCount, FormatTrait};
use vsc_data_structures::FxIndexMap;
use vsc_hir::{self as hir, LangItem};
use vsc_span::{Ident, Span, Symbol};

use crate::LoweringContext;

/// How an argument is passed: formatted with a trait, or as a count.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ArgumentType {
    Format(FormatTrait),
    Usize,
}

impl LoweringContext<'_> {
    pub(crate) fn lower_builtin(&mut self, b: &ast::BuiltinCall, span: Span) -> hir::ExprKind {
        use ast::BuiltinKind as B;
        let call = match &b.kind {
            B::Print { newline, stderr, args } => {
                let args = self.lower_format_args(args, *newline);
                let item = if *stderr { LangItem::Eprint } else { LangItem::Print };
                return self.call_kind(span, item, vec![args]);
            }
            B::Format(args) => {
                let args = self.lower_format_args(args, false);
                return self.call_kind(span, LangItem::Format, vec![args]);
            }
            B::Panic(msg) => match msg {
                Some(fa) => self.lower_format_args(fa, false),
                None => self.format_args_const(span, "explicit panic"),
            },
            B::Todo(msg) => self.panic_message(span, "not yet implemented", msg.as_deref()),
            B::Unreachable(msg) => self.panic_message(span, "internal error: entered unreachable code", msg.as_deref()),
            B::Assert { cond, msg } => return self.lower_assert(span, cond, msg.as_deref()),
            B::AssertEq { left, right, msg } => return self.lower_assert_eq(span, left, right, msg.as_deref()),
        };
        self.call_kind(span, LangItem::PanicFmt, vec![call])
    }

    fn call_kind(&mut self, span: Span, item: LangItem, args: Vec<hir::Expr>) -> hir::ExprKind {
        let f = self.expr_lang_item_path(span, item);
        hir::ExprKind::Call(Box::new(f), args)
    }

    /// `prefix`, or `prefix: {msg}` when there's a message.
    fn panic_message(&mut self, span: Span, prefix: &str, msg: Option<&FormatArgs>) -> hir::Expr {
        let Some(fa) = msg else { return self.format_args_const(span, prefix) };
        let inner = self.lower_format_args(fa, false);
        let inner_ref = self.expr_addr_of(fa.span, inner);
        let arg = self.make_argument(fa.span, ArgumentType::Format(FormatTrait::Display), inner_ref);
        let pieces = self.pieces_array(span, &[format!("{prefix}: ")]);
        let args = self.expr(span, hir::ExprKind::Array(vec![arg]));
        let args = self.expr_addr_of(span, args);
        let f = self.expr_lang_item_type_relative(span, LangItem::FormatArguments, "newV1");
        self.expr_call(span, f, vec![pieces, args])
    }

    /// `FormatArguments.newConst(&["text"])`.
    fn format_args_const(&mut self, span: Span, text: &str) -> hir::Expr {
        let pieces = self.pieces_array(span, &[text.to_string()]);
        let f = self.expr_lang_item_type_relative(span, LangItem::FormatArguments, "newConst");
        self.expr_call(span, f, vec![pieces])
    }

    /// `&["a", "b"]`
    fn pieces_array(&mut self, span: Span, pieces: &[String]) -> hir::Expr {
        let elems = pieces.iter().map(|p| self.expr_str(span, p)).collect();
        let array = self.expr(span, hir::ExprKind::Array(elems));
        self.expr_addr_of(span, array)
    }

    /// `FormatArgument.newDisplay(arg)` and the rest. The library's methods
    /// are `camelCase` (exception 11).
    fn make_argument(&mut self, span: Span, ty: ArgumentType, arg: hir::Expr) -> hir::Expr {
        let name = match ty {
            ArgumentType::Format(t) => match t {
                FormatTrait::Display => "newDisplay",
                FormatTrait::Debug => "newDebug",
                FormatTrait::LowerExp => "newLowerExp",
                FormatTrait::UpperExp => "newUpperExp",
                FormatTrait::Octal => "newOctal",
                FormatTrait::Pointer => "newPointer",
                FormatTrait::Binary => "newBinary",
                FormatTrait::LowerHex => "newLowerHex",
                FormatTrait::UpperHex => "newUpperHex",
            },
            ArgumentType::Usize => "fromUsize",
        };
        let f = self.expr_lang_item_type_relative(span, LangItem::FormatArgument, name);
        self.expr_call(span, f, vec![arg])
    }

    pub(crate) fn lower_format_args(&mut self, fa: &FormatArgs, newline: bool) -> hir::Expr {
        let span = fa.span;
        let lit_span = fa.lit_span;

        // The literal text before each placeholder, and after the last one.
        let mut pieces: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut placeholders = Vec::new();
        for piece in &fa.template {
            match piece {
                FormatArgsPiece::Literal(s) => current.push_str(s.as_str()),
                FormatArgsPiece::Placeholder(p) => {
                    pieces.push(std::mem::take(&mut current));
                    placeholders.push(p);
                }
            }
        }
        if newline {
            current.push('\n');
        }
        if !current.is_empty() {
            pieces.push(current);
        }

        // Each (argument, how it's used) once, in order of first use.
        let mut argmap: FxIndexMap<(usize, ArgumentType), Span> = FxIndexMap::default();
        let mut use_format_options = false;
        for (i, p) in placeholders.iter().enumerate() {
            let (index, _) = argmap.insert_full((p.argument, ArgumentType::Format(p.format_trait)), p.span);
            if index != i || p.format_options != ast::FormatOptions::default() {
                use_format_options = true;
            }
        }
        if use_format_options {
            for p in &placeholders {
                for count in [p.format_options.precision, p.format_options.width].into_iter().flatten() {
                    if let FormatCount::Argument(a) = count {
                        argmap.insert_full((a, ArgumentType::Usize), p.span);
                    }
                }
            }
        }

        let pieces_expr = self.pieces_array(lit_span, &pieces);
        if argmap.is_empty() && !use_format_options {
            let f = self.expr_lang_item_type_relative(span, LangItem::FormatArguments, "newConst");
            return self.expr_call(span, f, vec![pieces_expr]);
        }

        // The arguments, evaluated once each.
        let use_simple_array = argmap.len() == fa.arguments.len() && argmap.keys().enumerate().all(|(i, &(j, _))| i == j);
        let args_expr = if use_simple_array {
            // `&[FormatArgument.newDisplay(&a), …]`
            let elems = argmap
                .keys()
                .copied()
                .collect::<Vec<_>>()
                .into_iter()
                .map(|(index, ty)| {
                    let a = &fa.arguments[index].expr;
                    let arg_span = a.span;
                    let value = self.lower_expr(a);
                    let value = self.expr_addr_of(arg_span, value);
                    self.make_argument(arg_span, ty, value)
                })
                .collect();
            let array = self.expr(span, hir::ExprKind::Array(elems));
            self.expr_addr_of(span, array)
        } else {
            // `&match (&a, &b) { args => [FormatArgument.newDisplay(args.0), …] }`
            let refs = fa
                .arguments
                .iter()
                .map(|arg| {
                    let arg_span = arg.expr.span;
                    let value = self.lower_expr(&arg.expr);
                    self.expr_addr_of(arg_span, value)
                })
                .collect();
            let tuple = self.expr(span, hir::ExprKind::Tup(refs));
            let args_ident = Ident::new(Symbol::intern("args"), span);
            let (args_pat, args_id) = self.pat_ident_binding(span, args_ident, ast::Mutability::Not);
            let entries: Vec<_> = argmap.iter().map(|(&k, &s)| (k, s)).collect();
            let elems = entries
                .into_iter()
                .map(|((index, ty), arg_span)| {
                    let args = self.expr_local(arg_span, args_ident, args_id);
                    let field = Ident::new(Symbol::intern(&index.to_string()), arg_span);
                    let value = self.expr(arg_span, hir::ExprKind::Field(Box::new(args), field));
                    self.make_argument(arg_span, ty, value)
                })
                .collect();
            let array = self.expr(span, hir::ExprKind::Array(elems));
            let arm = self.arm(args_pat, array);
            let m = self.expr(span, hir::ExprKind::Match(Box::new(tuple), vec![arm], hir::MatchSource::FormatArgs));
            self.expr_addr_of(span, m)
        };

        if !use_format_options {
            let f = self.expr_lang_item_type_relative(span, LangItem::FormatArguments, "newV1");
            return self.expr_call(span, f, vec![pieces_expr, args_expr]);
        }

        // `&[FormatPlaceholder.new(position, fill, align, flags, precision, width), …]`
        let specs = placeholders
            .iter()
            .map(|p| {
                let position = argmap.get_index_of(&(p.argument, ArgumentType::Format(p.format_trait))).unwrap();
                self.make_placeholder(p, position, &argmap)
            })
            .collect();
        let specs = self.expr(span, hir::ExprKind::Array(specs));
        let specs = self.expr_addr_of(span, specs);
        let f = self.expr_lang_item_type_relative(span, LangItem::FormatArguments, "newV1Formatted");
        self.expr_call(span, f, vec![pieces_expr, args_expr, specs])
    }

    fn make_placeholder(
        &mut self,
        p: &ast::FormatPlaceholder,
        position: usize,
        argmap: &FxIndexMap<(usize, ArgumentType), Span>,
    ) -> hir::Expr {
        let span = p.span;
        let o = &p.format_options;
        let position = self.expr_uint(span, position);
        let fill = self.expr_char(span, o.fill.unwrap_or(' '));
        let align_name = match o.alignment {
            Some(ast::FormatAlignment::Left) => "Left",
            Some(ast::FormatAlignment::Right) => "Right",
            Some(ast::FormatAlignment::Center) => "Center",
            None => "Unknown",
        };
        let align = self.expr_lang_item_type_relative(span, LangItem::FormatAlignment, align_name);
        // The flag bits, as in rustc's `fmt::rt::Flag`.
        let flags = (o.sign == Some(ast::FormatSign::Plus)) as usize
            | ((o.sign == Some(ast::FormatSign::Minus)) as usize) << 1
            | (o.alternate as usize) << 2
            | (o.zero_pad as usize) << 3
            | ((o.debug_hex == Some(ast::FormatDebugHex::Lower)) as usize) << 4
            | ((o.debug_hex == Some(ast::FormatDebugHex::Upper)) as usize) << 5;
        let flags = self.expr_uint(span, flags);
        let precision = self.make_count(span, o.precision, argmap);
        let width = self.make_count(span, o.width, argmap);
        let f = self.expr_lang_item_type_relative(span, LangItem::FormatPlaceholder, "new");
        self.expr_call(span, f, vec![position, fill, align, flags, precision, width])
    }

    /// `FormatCount.Is(n)`, `FormatCount.Param(i)` or `FormatCount.Implied`.
    fn make_count(&mut self, span: Span, count: Option<FormatCount>, argmap: &FxIndexMap<(usize, ArgumentType), Span>) -> hir::Expr {
        match count {
            Some(FormatCount::Literal(n)) => {
                let f = self.expr_lang_item_type_relative(span, LangItem::FormatCount, "Is");
                let n = self.expr_uint(span, n);
                self.expr_call(span, f, vec![n])
            }
            Some(FormatCount::Argument(a)) => {
                let i = argmap.get_index_of(&(a, ArgumentType::Usize)).unwrap();
                let f = self.expr_lang_item_type_relative(span, LangItem::FormatCount, "Param");
                let i = self.expr_uint(span, i);
                self.expr_call(span, f, vec![i])
            }
            None => self.expr_lang_item_type_relative(span, LangItem::FormatCount, "Implied"),
        }
    }

    // -----------------------------------------------------------------------
    // Assertions
    // -----------------------------------------------------------------------

    /// `assert(cond, msg…)` → `if !cond { panicFmt(msg) }`, where the
    /// default message is `assertion failed: cond`.
    fn lower_assert(&mut self, span: Span, cond: &ast::Expr, msg: Option<&FormatArgs>) -> hir::ExprKind {
        let cond_span = cond.span;
        let lowered = self.lower_expr(cond);
        let not = self.expr(cond_span, hir::ExprKind::Unary(ast::UnOp::Not, Box::new(lowered)));
        let not = self.expr(cond_span, hir::ExprKind::DropTemps(Box::new(not)));
        let msg = match msg {
            Some(fa) => self.lower_format_args(fa, false),
            None => {
                let text = self.psess.source_map.span_to_snippet(cond_span).unwrap_or_default();
                self.format_args_const(span, &format!("assertion failed: {text}"))
            }
        };
        let panic = self.expr_call_lang_item_fn(span, LangItem::PanicFmt, vec![msg]);
        let block = self.block_expr(panic);
        let then = self.expr_block(block);
        hir::ExprKind::If(Box::new(not), Box::new(then), None)
    }

    /// ```text
    /// match (&left, &right) {
    ///     (leftVal, rightVal) => if !(*leftVal == *rightVal) {
    ///         assertFailed(AssertKind.Eq, &*leftVal, &*rightVal, None | Some(msg))
    ///     }
    /// }
    /// ```
    fn lower_assert_eq(&mut self, span: Span, left: &ast::Expr, right: &ast::Expr, msg: Option<&FormatArgs>) -> hir::ExprKind {
        let (ls, rs) = (left.span, right.span);
        let l = self.lower_expr(left);
        let l = self.expr_addr_of(ls, l);
        let r = self.lower_expr(right);
        let r = self.expr_addr_of(rs, r);
        let scrutinee = self.expr(span, hir::ExprKind::Tup(vec![l, r]));

        let left_ident = Ident::new(Symbol::intern("leftVal"), ls);
        let right_ident = Ident::new(Symbol::intern("rightVal"), rs);
        let (lp, lid) = self.pat_ident_binding(ls, left_ident, ast::Mutability::Not);
        let (rp, rid) = self.pat_ident_binding(rs, right_ident, ast::Mutability::Not);
        let pat = hir::Pat { hir_id: self.next_id(), kind: hir::PatKind::Tuple(vec![lp, rp], None), span };

        let deref = |this: &mut Self, s: Span, ident: Ident, id| {
            let v = this.expr_local(s, ident, id);
            this.expr(s, hir::ExprKind::Unary(ast::UnOp::Deref, Box::new(v)))
        };
        let lv = deref(self, ls, left_ident, lid);
        let rv = deref(self, rs, right_ident, rid);
        let eq = self.expr(span, hir::ExprKind::Binary(Self::binop(span, ast::BinOpKind::Eq), Box::new(lv), Box::new(rv)));
        let not = self.expr(span, hir::ExprKind::Unary(ast::UnOp::Not, Box::new(eq)));
        let not = self.expr(span, hir::ExprKind::DropTemps(Box::new(not)));

        let kind = self.expr_lang_item_type_relative(span, LangItem::AssertKind, "Eq");
        let lv = deref(self, ls, left_ident, lid);
        let lv = self.expr_addr_of(ls, lv);
        let rv = deref(self, rs, right_ident, rid);
        let rv = self.expr_addr_of(rs, rv);
        let msg = match msg {
            Some(fa) => {
                let m = self.lower_format_args(fa, false);
                self.expr_call_lang_item_fn(fa.span, LangItem::OptionSome, vec![m])
            }
            None => self.expr_lang_item_path(span, LangItem::OptionNone),
        };
        let failed = self.expr_call_lang_item_fn(span, LangItem::AssertFailed, vec![kind, lv, rv, msg]);
        let block = self.block_expr(failed);
        let then = self.expr_block(block);
        let if_expr = self.expr(span, hir::ExprKind::If(Box::new(not), Box::new(then), None));
        let arm = self.arm(pat, if_expr);
        hir::ExprKind::Match(Box::new(scrutinee), vec![arm], hir::MatchSource::Normal)
    }
}
