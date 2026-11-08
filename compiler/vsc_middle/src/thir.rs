//! The typed high-level IR (rustc's `rustc_middle::thir`).
//!
//! THIR is a body after type checking, with everything implicit made
//! explicit: adjustments (autoderef, autoref, unsizing, `never` to any) are
//! nodes, method calls are calls of the method's function item, overloaded
//! operators would be calls (operators are primitive-only for now), and
//! patterns spell out the dereferences default binding modes did. MIR is
//! built from it, and the unsafety and exhaustiveness checks run on it.
//!
//! Vertex adds two nodes: `ClassNew`, the construction of a class value
//! (exception 3), and the `ToFunc` pointer coercion that makes a `func`
//! value (exception 14).

pub mod visit;

use vsc_ast::{BinOpKind, Mutability, UnOp};
use vsc_hir::HirId;
use vsc_index::IndexVec;
use vsc_span::def_id::{DefId, LocalDefId};
use vsc_span::{Span, Symbol};

use crate::ty::{AdtDef, Const, GenericArgsRef, Ty};

vsc_index::newtype_index! {
    pub struct ExprId {}
}
vsc_index::newtype_index! {
    pub struct StmtId {}
}
vsc_index::newtype_index! {
    pub struct BlockId {}
}
vsc_index::newtype_index! {
    pub struct ArmId {}
}

#[derive(Debug)]
pub struct Thir<'tcx> {
    pub owner: LocalDefId,
    pub exprs: IndexVec<ExprId, Expr<'tcx>>,
    pub stmts: IndexVec<StmtId, Stmt<'tcx>>,
    pub blocks: IndexVec<BlockId, Block>,
    pub arms: IndexVec<ArmId, Arm<'tcx>>,
    pub params: Vec<Param<'tcx>>,
    /// The body's value.
    pub value: Option<ExprId>,
}

impl<'tcx> Thir<'tcx> {
    pub fn new(owner: LocalDefId) -> Self {
        Thir {
            owner,
            exprs: IndexVec::new(),
            stmts: IndexVec::new(),
            blocks: IndexVec::new(),
            arms: IndexVec::new(),
            params: Vec::new(),
            value: None,
        }
    }
}

#[derive(Debug)]
pub struct Param<'tcx> {
    pub pat: Option<Box<Pat<'tcx>>>,
    pub ty: Ty<'tcx>,
    /// Is it `self` (or a flat receiver)?
    pub self_kind: bool,
}

#[derive(Debug)]
pub struct Expr<'tcx> {
    pub kind: ExprKind<'tcx>,
    pub ty: Ty<'tcx>,
    pub span: Span,
}

#[derive(Debug)]
pub enum BlockSafety {
    Safe,
    /// `unsafe { … }`
    ExplicitUnsafe(HirId),
}

#[derive(Debug)]
pub struct Block {
    /// A labeled block's id, which its `break`s name.
    pub target: Option<HirId>,
    pub stmts: Vec<StmtId>,
    pub expr: Option<ExprId>,
    pub safety_mode: BlockSafety,
    pub span: Span,
}

#[derive(Debug)]
pub struct Stmt<'tcx> {
    pub kind: StmtKind<'tcx>,
}

#[derive(Debug)]
pub enum StmtKind<'tcx> {
    /// An expression whose value is discarded.
    Expr { expr: ExprId },
    Let { pattern: Box<Pat<'tcx>>, initializer: Option<ExprId>, else_block: Option<BlockId>, span: Span },
}

#[derive(Debug)]
pub struct Arm<'tcx> {
    pub pattern: Box<Pat<'tcx>>,
    pub guard: Option<ExprId>,
    pub body: ExprId,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogicalOp {
    And,
    Or,
}

/// How a `match` came about, for its diagnostics.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatchSource {
    Normal,
    ForLoop,
    Try,
    Desugared,
}

/// A coercion between pointer-like types.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerCoercion {
    /// `&[T; N]` to `&[T]`.
    Unsize,
    /// A function item to a function pointer.
    ReifyFnPointer,
    /// A function item or literal to a `func` value (exception 14).
    ToFunc,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BorrowKind {
    Shared,
    Mut,
}

#[derive(Debug)]
pub enum ExprKind<'tcx> {
    If { cond: ExprId, then: ExprId, else_opt: Option<ExprId> },
    Call { fun: ExprId, args: Vec<ExprId>, fn_span: Span },
    /// `*x`, written or from autoderef.
    Deref { arg: ExprId },
    Binary { op: BinOpKind, lhs: ExprId, rhs: ExprId },
    LogicalOp { op: LogicalOp, lhs: ExprId, rhs: ExprId },
    Unary { op: UnOp, arg: ExprId },
    Cast { source: ExprId },
    /// A value used as is (a desugaring's `DropTemps`).
    Use { source: ExprId },
    NeverToAny { source: ExprId },
    PointerCoercion { cast: PointerCoercion, source: ExprId },
    /// `loop`; `id` is what its `break`s and `continue`s name.
    Loop { body: ExprId, id: HirId },
    /// `let pat = expr` in a condition.
    Let { expr: ExprId, pat: Box<Pat<'tcx>> },
    Match { scrutinee: ExprId, arms: Vec<ArmId>, source: MatchSource },
    Block { block: BlockId },
    Assign { lhs: ExprId, rhs: ExprId },
    AssignOp { op: BinOpKind, lhs: ExprId, rhs: ExprId },
    Field { lhs: ExprId, variant_index: usize, field: usize },
    Index { lhs: ExprId, index: ExprId },
    /// A local variable of this body: its binding's id.
    VarRef { id: HirId },
    /// A variable of an enclosing body, used in a function literal.
    UpvarRef { closure_def_id: DefId, var_hir_id: HirId },
    Borrow { borrow_kind: BorrowKind, arg: ExprId },
    RawBorrow { mutability: Mutability, arg: ExprId },
    /// `break` out of the loop or labeled block with this id.
    Break { target: HirId, value: Option<ExprId> },
    Continue { target: HirId },
    Return { value: Option<ExprId> },
    Repeat { value: ExprId, count: Const<'tcx> },
    Array { fields: Vec<ExprId> },
    Tuple { fields: Vec<ExprId> },
    Adt(Box<AdtExpr<'tcx>>),
    /// A class value from its struct (exception 3): Rust's
    /// `Rc::new(RefCell::new(value))`.
    ClassNew { value: ExprId },
    Closure { closure_id: LocalDefId },
    Literal { lit: vsc_ast::token::Lit, neg: bool },
    /// A zero-sized value: a function item.
    ZstLiteral,
    /// A constant, by its definition.
    NamedConst { def_id: DefId, args: GenericArgsRef<'tcx> },
    ConstParam { def_id: DefId },
    /// A static, as a place: its address, dereferenced.
    StaticRef { def_id: DefId },
    InlineAsm { operands: Vec<ExprId> },
    /// An expression type checking rejected.
    Error,
}

#[derive(Debug)]
pub struct AdtExpr<'tcx> {
    pub adt_def: AdtDef<'tcx>,
    pub variant_index: usize,
    pub args: GenericArgsRef<'tcx>,
    /// By field index.
    pub fields: Vec<(usize, ExprId)>,
    /// `..base`
    pub base: Option<ExprId>,
}

// ---------------------------------------------------------------------------
// Patterns
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Pat<'tcx> {
    pub ty: Ty<'tcx>,
    pub span: Span,
    pub kind: PatKind<'tcx>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ByRef {
    No,
    Yes(Mutability),
}

#[derive(Clone, Debug)]
pub struct FieldPat<'tcx> {
    pub field: usize,
    pub pattern: Pat<'tcx>,
}

/// A constant in a pattern.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PatConst {
    /// An integer or `char`, as its bits (two's complement, sign extended
    /// to 128 bits for signed types).
    Int(u128),
    Bool(bool),
    /// A string or float: compared by value, never exhaustive.
    Str(Symbol),
    Float(Symbol),
    /// A named constant that isn't a literal: matched by equality, never
    /// exhaustive.
    Opaque(DefId),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RangeEnd {
    Included,
    Excluded,
}

#[derive(Clone, Debug)]
pub enum PatKind<'tcx> {
    Wild,
    Binding { name: Symbol, mutability: Mutability, mode: ByRef, var: HirId, ty: Ty<'tcx>, subpattern: Option<Box<Pat<'tcx>>> },
    /// An enum variant.
    Variant { adt_def: AdtDef<'tcx>, args: GenericArgsRef<'tcx>, variant_index: usize, subpatterns: Vec<FieldPat<'tcx>> },
    /// A struct, tuple or other single-constructor value.
    Leaf { subpatterns: Vec<FieldPat<'tcx>> },
    /// `&pat`, written or implied by default binding modes.
    Deref { subpattern: Box<Pat<'tcx>> },
    Constant { value: PatConst },
    Range { lo: Option<PatConst>, hi: Option<PatConst>, end: RangeEnd },
    /// `[a, rest @ .., z]` on a slice.
    Slice { prefix: Vec<Pat<'tcx>>, slice: Option<Box<Pat<'tcx>>>, suffix: Vec<Pat<'tcx>> },
    /// The same on an array, whose length is known.
    Array { prefix: Vec<Pat<'tcx>>, slice: Option<Box<Pat<'tcx>>>, suffix: Vec<Pat<'tcx>> },
    Or { pats: Vec<Pat<'tcx>> },
    Error,
}

/// The value of an integer literal's text: decimal, `0x`, `0o` or `0b`,
/// with `_` separators.
pub fn parse_int_lit(text: &str) -> Option<u128> {
    let t: String = text.chars().filter(|&c| c != '_').collect();
    if let Some(h) = t.strip_prefix("0x") {
        u128::from_str_radix(h, 16).ok()
    } else if let Some(o) = t.strip_prefix("0o") {
        u128::from_str_radix(o, 8).ok()
    } else if let Some(b) = t.strip_prefix("0b") {
        u128::from_str_radix(b, 2).ok()
    } else {
        t.parse().ok()
    }
}

/// The value of a `char` literal's text (escapes included).
pub fn parse_char_lit(text: &str) -> Option<char> {
    let mut chars = text.chars();
    match chars.next()? {
        '\\' => match chars.next()? {
            'n' => Some('\n'),
            't' => Some('\t'),
            'r' => Some('\r'),
            '0' => Some('\0'),
            '\\' => Some('\\'),
            '\'' => Some('\''),
            '"' => Some('"'),
            'x' => u32::from_str_radix(chars.as_str(), 16).ok().and_then(char::from_u32),
            'u' => {
                let s = chars.as_str().trim_start_matches('{').trim_end_matches('}');
                u32::from_str_radix(&s.replace('_', ""), 16).ok().and_then(char::from_u32)
            }
            _ => None,
        },
        c => Some(c),
    }
}
