//! AST expressions, builtins, format arguments, inline assembly, blocks, and statements.

use vsc_span::{Ident, Span, Symbol};

use super::{AttrVec, FnDecl, Item, Mutability, P, Pat, Path, PathSegment, QSelf, Ty};
use crate::node_id::NodeId;
use crate::token;

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
    pub attrs: AttrVec,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    /// `[a, b]`
    Array(Vec<P<Expr>>),
    /// `[x; n]`
    Repeat(P<Expr>, AnonConst),
    /// `vec[…]`: Rust's `vec![…]` (exception 12). Holds an `Array` or `Repeat`.
    Vec(P<Expr>),
    /// `const { … }`
    ConstBlock(AnonConst),
    /// `f(a, b)`. A call whose callee is a path may turn out to be a method
    /// call once names are resolved (exception 7).
    Call(P<Expr>, Vec<P<Expr>>),
    /// `x.f(a)`, when the receiver isn't a plain path.
    MethodCall(P<MethodCall>),
    /// `(a, b)`
    Tup(Vec<P<Expr>>),
    Binary(BinOp, P<Expr>, P<Expr>),
    Unary(UnOp, P<Expr>),
    Lit(token::Lit),
    /// `x as T`
    Cast(P<Expr>, P<Ty>),
    /// `let pat = expr`, only in `if` and `while` conditions and guards.
    Let(P<Pat>, P<Expr>, Span),
    /// `if cond { … } else …`
    If(P<Expr>, P<Block>, Option<P<Expr>>),
    /// `'label: while cond { … }`
    While(P<Expr>, P<Block>, Option<Label>),
    /// `'label: for pat in iter { … }`
    ForLoop { pat: P<Pat>, iter: P<Expr>, body: P<Block>, label: Option<Label> },
    /// `'label: loop { … }`
    Loop(P<Block>, Option<Label>, Span),
    /// `match x { arms }`
    Match(P<Expr>, Vec<Arm>),
    /// `func(x) { … }` (exception 14).
    Closure(P<Closure>),
    /// `{ … }`, `'label: { … }`, `unsafe { … }` (see `Block::rules`).
    Block(P<Block>, Option<Label>),
    /// `async { … }`
    Async(P<Block>),
    /// `x.await`
    Await(P<Expr>, Span),
    /// `a = b`
    Assign(P<Expr>, P<Expr>, Span),
    /// `a += b`
    AssignOp(AssignOp, P<Expr>, P<Expr>),
    /// `x.field`, `x.0`, when the receiver isn't a plain path.
    Field(P<Expr>, Ident),
    /// `x[i]`
    Index(P<Expr>, P<Expr>, Span),
    /// `a..b` and friends.
    Range(Option<P<Expr>>, Option<P<Expr>>, RangeLimits),
    /// `_` on the left of an assignment.
    Underscore,
    /// `a.b.c`, `<T as Trait>.f`.
    Path(Option<P<QSelf>>, Path),
    /// `&x`, `&mut x`, `&raw const x`.
    AddrOf(BorrowKind, Mutability, P<Expr>),
    /// `break 'label value`
    Break(Option<Label>, Option<P<Expr>>),
    /// `continue 'label`
    Continue(Option<Label>),
    /// `return value`
    Ret(Option<P<Expr>>),
    /// `asm(…)`, `nakedAsm(…)` (exception 12).
    InlineAsm(P<InlineAsm>),
    /// A built-in call after expansion: `println(…)`, `format(…)`,
    /// `assert(…)` and the rest (exception 12). Before expansion these are
    /// ordinary `Call`s.
    Builtin(P<BuiltinCall>),
    /// `Point { x: 1, ..base }`
    Struct(P<StructExpr>),
    /// `(x)`
    Paren(P<Expr>),
    /// `x?`
    Try(P<Expr>),
    /// An expression the parser already reported an error for.
    Err,
}

impl Expr {
    /// Does the expression end with a block, so that it can stand as a
    /// statement without a terminator?
    pub fn is_block_like(&self) -> bool {
        matches!(
            self.kind,
            ExprKind::Block(..)
                | ExprKind::Async(..)
                | ExprKind::If(..)
                | ExprKind::While(..)
                | ExprKind::ForLoop { .. }
                | ExprKind::Loop(..)
                | ExprKind::Match(..)
                | ExprKind::ConstBlock(..)
        )
    }
}

#[derive(Clone, Debug)]
pub struct MethodCall {
    pub seg: PathSegment,
    pub receiver: P<Expr>,
    pub args: Vec<P<Expr>>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StructExpr {
    pub qself: Option<P<QSelf>>,
    pub path: Path,
    pub fields: Vec<ExprField>,
    pub rest: StructRest,
}

#[derive(Clone, Debug)]
pub struct ExprField {
    pub attrs: AttrVec,
    pub id: NodeId,
    pub span: Span,
    pub ident: Ident,
    pub expr: P<Expr>,
    /// Written as just the name: `Task { id }`.
    pub is_shorthand: bool,
}

#[derive(Clone, Debug)]
pub enum StructRest {
    /// `..base`
    Base(P<Expr>),
    None,
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub attrs: AttrVec,
    pub pat: P<Pat>,
    pub guard: Option<P<Expr>>,
    pub body: P<Expr>,
    pub span: Span,
    pub id: NodeId,
}

#[derive(Clone, Copy, Debug)]
pub struct Label {
    pub ident: Ident,
}

/// A function literal (exception 14). There is no `move`: whether captures
/// are taken by value is inferred from whether the literal escapes.
#[derive(Clone, Debug)]
pub struct Closure {
    pub capture_clause: CaptureBy,
    pub asyncness: Option<Span>,
    pub fn_decl: P<FnDecl>,
    pub body: P<Block>,
    /// The `func(…) -> T` part.
    pub fn_decl_span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureBy {
    /// Captures by reference unless inference decides otherwise. Every
    /// function literal starts out like this.
    Ref,
    /// Captures by value.
    Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeLimits {
    /// `..`
    HalfOpen,
    /// `..=`
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorrowKind {
    /// `&x`
    Ref,
    /// `&raw const x`
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinOpKind {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    BitXor,
    BitAnd,
    BitOr,
    Shl,
    Shr,
    Eq,
    Lt,
    Le,
    Ne,
    Ge,
    Gt,
}

impl BinOpKind {
    pub fn as_str(self) -> &'static str {
        use BinOpKind::*;
        match self {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Rem => "%",
            And => "&&",
            Or => "||",
            BitXor => "^",
            BitAnd => "&",
            BitOr => "|",
            Shl => "<<",
            Shr => ">>",
            Eq => "==",
            Lt => "<",
            Le => "<=",
            Ne => "!=",
            Ge => ">=",
            Gt => ">",
        }
    }

    pub fn is_comparison(self) -> bool {
        matches!(self, BinOpKind::Eq | BinOpKind::Lt | BinOpKind::Le | BinOpKind::Ne | BinOpKind::Ge | BinOpKind::Gt)
    }
}

pub type BinOp = vsc_span::Spanned<BinOpKind>;
pub type AssignOp = vsc_span::Spanned<BinOpKind>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    /// `*`
    Deref,
    /// `!`
    Not,
    /// `-`
    Neg,
}

impl UnOp {
    pub fn as_str(self) -> &'static str {
        match self {
            UnOp::Deref => "*",
            UnOp::Not => "!",
            UnOp::Neg => "-",
        }
    }
}

/// An expression evaluated at compile time: array lengths, discriminants,
/// const generic arguments.
#[derive(Clone, Debug)]
pub struct AnonConst {
    pub id: NodeId,
    pub value: P<Expr>,
}

// ---------------------------------------------------------------------------
// Built-ins and format strings (exception 12)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct BuiltinCall {
    pub kind: BuiltinKind,
    /// The built-in's name, as written.
    pub name_span: Span,
}

#[derive(Clone, Debug)]
pub enum BuiltinKind {
    /// `print`, `println`, `eprint`, `eprintln`.
    Print { newline: bool, stderr: bool, args: P<FormatArgs> },
    /// `format(…)`: a `string`.
    Format(P<FormatArgs>),
    Panic(Option<P<FormatArgs>>),
    Todo(Option<P<FormatArgs>>),
    Unreachable(Option<P<FormatArgs>>),
    Assert { cond: P<Expr>, msg: Option<P<FormatArgs>> },
    AssertEq { left: P<Expr>, right: P<Expr>, msg: Option<P<FormatArgs>> },
}

impl BuiltinKind {
    pub fn name(&self) -> &'static str {
        match self {
            BuiltinKind::Print { newline: false, stderr: false, .. } => "print",
            BuiltinKind::Print { newline: true, stderr: false, .. } => "println",
            BuiltinKind::Print { newline: false, stderr: true, .. } => "eprint",
            BuiltinKind::Print { newline: true, stderr: true, .. } => "eprintln",
            BuiltinKind::Format(_) => "format",
            BuiltinKind::Panic(_) => "panic",
            BuiltinKind::Todo(_) => "todo",
            BuiltinKind::Unreachable(_) => "unreachable",
            BuiltinKind::Assert { .. } => "assert",
            BuiltinKind::AssertEq { .. } => "assertEq",
        }
    }
}

/// A checked format string and its arguments (rustc's `FormatArgs`).
#[derive(Clone, Debug)]
pub struct FormatArgs {
    pub span: Span,
    /// The string literal as written, for printing.
    pub lit: token::Lit,
    pub lit_span: Span,
    pub template: Vec<FormatArgsPiece>,
    /// Explicit arguments first (positional, then named), then the names
    /// captured from the string (`{name}`).
    pub arguments: Vec<FormatArgument>,
}

#[derive(Clone, Debug)]
pub enum FormatArgsPiece {
    Literal(Symbol),
    Placeholder(FormatPlaceholder),
}

#[derive(Clone, Debug)]
pub struct FormatPlaceholder {
    /// Index into `FormatArgs::arguments`.
    pub argument: usize,
    pub span: Span,
    pub format_trait: FormatTrait,
    pub format_options: FormatOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormatTrait {
    /// `{}`
    Display,
    /// `{:?}`
    Debug,
    /// `{:e}`
    LowerExp,
    /// `{:E}`
    UpperExp,
    /// `{:o}`
    Octal,
    /// `{:p}`
    Pointer,
    /// `{:b}`
    Binary,
    /// `{:x}`
    LowerHex,
    /// `{:X}`
    UpperHex,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormatOptions {
    pub width: Option<FormatCount>,
    pub precision: Option<FormatCount>,
    pub alignment: Option<FormatAlignment>,
    pub fill: Option<char>,
    pub sign: Option<FormatSign>,
    pub alternate: bool,
    pub zero_pad: bool,
    pub debug_hex: Option<FormatDebugHex>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatCount {
    Literal(usize),
    /// Index into `FormatArgs::arguments`.
    Argument(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatAlignment {
    Left,
    Right,
    Center,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatSign {
    Plus,
    Minus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatDebugHex {
    Lower,
    Upper,
}

#[derive(Clone, Debug)]
pub struct FormatArgument {
    pub kind: FormatArgumentKind,
    pub expr: P<Expr>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FormatArgumentKind {
    /// `format("{}", x)`
    Normal,
    /// `format("{n}", n = 5)`
    Named(Ident),
    /// `format("{n}")`: `n` taken from the scope.
    Captured(Ident),
}

// ---------------------------------------------------------------------------
// Inline assembly (exception 12)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsmMacro {
    /// `asm(…)`
    Asm,
    /// `nakedAsm(…)`
    NakedAsm,
    /// `globalAsm(…)`
    GlobalAsm,
}

#[derive(Clone, Debug)]
pub struct InlineAsm {
    pub asm_macro: AsmMacro,
    /// The template strings, unparsed.
    pub templates: Vec<(token::Lit, Span)>,
    pub operands: Vec<(InlineAsmOperand, Span)>,
    /// `clobberAbi("C")`
    pub clobber_abis: Vec<(Symbol, Span)>,
    /// `options(pure, nomem)`
    pub options: Vec<(Symbol, Span)>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum InlineAsmRegOrRegClass {
    /// `"eax"`
    Reg(Symbol),
    /// `reg`
    RegClass(Symbol),
}

#[derive(Clone, Debug)]
pub enum InlineAsmOperand {
    In { name: Option<Ident>, reg: InlineAsmRegOrRegClass, expr: P<Expr> },
    Out { name: Option<Ident>, reg: InlineAsmRegOrRegClass, late: bool, expr: Option<P<Expr>> },
    InOut { name: Option<Ident>, reg: InlineAsmRegOrRegClass, late: bool, expr: P<Expr> },
    SplitInOut {
        name: Option<Ident>,
        reg: InlineAsmRegOrRegClass,
        late: bool,
        in_expr: P<Expr>,
        out_expr: Option<P<Expr>>,
    },
    Const { name: Option<Ident>, anon_const: AnonConst },
    Sym { name: Option<Ident>, id: NodeId, qself: Option<P<QSelf>>, path: Path },
    Label { name: Option<Ident>, block: P<Block> },
}

// ---------------------------------------------------------------------------
// Statements and blocks (exception 10)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub id: NodeId,
    pub rules: BlockCheckMode,
    pub span: Span,
}

impl Block {
    /// The final expression, which is the block's value (Scala's rule): the
    /// last statement, when it is an expression statement.
    pub fn tail_expr(&self) -> Option<&Expr> {
        match self.stmts.last().map(|s| &s.kind) {
            Some(StmtKind::Expr(e)) => Some(e),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockCheckMode {
    Default,
    /// `unsafe { … }`
    Unsafe,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub id: NodeId,
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    /// `let x = e`, `var x = e`.
    Let(P<Local>),
    Item(P<Item>),
    /// An expression statement. Vertex has no `;` to tell a tail expression
    /// from a discarded one, so every expression statement is this kind; the
    /// type checker discards the value of a non-final one, and of a final one
    /// in a block whose type is `()`.
    Expr(P<Expr>),
}

#[derive(Clone, Debug)]
pub struct Local {
    pub id: NodeId,
    /// `var` instead of `let`: every binding in the pattern is mutable.
    pub is_var: bool,
    pub pat: P<Pat>,
    pub ty: Option<P<Ty>>,
    pub kind: LocalKind,
    pub span: Span,
    pub attrs: AttrVec,
}

#[derive(Clone, Debug)]
pub enum LocalKind {
    /// `let x`
    Decl,
    /// `let x = e`
    Init(P<Expr>),
    /// `let Some(x) = e else { … }`
    InitElse(P<Expr>, P<Block>),
}

