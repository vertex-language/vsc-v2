//! The mid-level IR (rustc's `rustc_middle::mir`).
//!
//! A MIR body is a control-flow graph of basic blocks over locals: `_0` is
//! the return value, then the arguments, then user variables and
//! temporaries. Each block is a list of statements (assignments, mostly)
//! and a terminator (a jump, a switch, a call, a return). Places are a local
//! with projections (`*`, fields, indexing, downcasts); rvalues compute a
//! value from operands. Borrow checking, the interpreter and code generation
//! all work on MIR.
//!
//! What Vertex adds: `Rvalue::ClassNew`, the construction of a class value
//! (exception 3); a dereference of a `Class<C>` place reaches the class's
//! struct; and the `ToFunc` pointer coercion makes a `func` value
//! (exception 14).
//!
//! Not built yet: drops (no `Drop` lang item yet), storage markers, and the
//! fake reads and false edges borrow checking will want.

pub mod pretty;

use vsc_ast::Mutability;
use vsc_index::IndexVec;
use vsc_span::def_id::DefId;
use vsc_span::{Span, Symbol};

use crate::thir::PointerCoercion;
use crate::ty::{Const, GenericArgsRef, Ty};

vsc_index::newtype_index! {
    pub struct Local { DEBUG_FORMAT = "_{}" }
}
vsc_index::newtype_index! {
    pub struct BasicBlock { DEBUG_FORMAT = "bb{}" }
}

pub const RETURN_PLACE: Local = Local::ZERO;
pub const START_BLOCK: BasicBlock = BasicBlock::ZERO;

#[derive(Debug)]
pub struct Body<'tcx> {
    pub source: DefId,
    pub basic_blocks: IndexVec<BasicBlock, BasicBlockData<'tcx>>,
    pub local_decls: IndexVec<Local, LocalDecl<'tcx>>,
    pub arg_count: usize,
    /// User variables' names, for debuggers and the printer.
    pub var_debug_info: Vec<(Symbol, Local)>,
    pub span: Span,
}

impl<'tcx> Body<'tcx> {
    pub fn args_iter(&self) -> impl Iterator<Item = Local> {
        (1..=self.arg_count).map(Local::from_usize)
    }

    pub fn return_ty(&self) -> Ty<'tcx> {
        self.local_decls[RETURN_PLACE].ty
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocalKind {
    ReturnPointer,
    Arg,
    /// A variable the user declared.
    UserVar(Symbol),
    Temp,
}

#[derive(Debug)]
pub struct LocalDecl<'tcx> {
    pub mutability: Mutability,
    pub ty: Ty<'tcx>,
    pub span: Span,
    pub kind: LocalKind,
}

#[derive(Debug, Default)]
pub struct BasicBlockData<'tcx> {
    pub statements: Vec<Statement<'tcx>>,
    pub terminator: Option<Terminator<'tcx>>,
}

impl<'tcx> BasicBlockData<'tcx> {
    #[track_caller]
    pub fn terminator(&self) -> &Terminator<'tcx> {
        self.terminator.as_ref().expect("a block without a terminator")
    }
}

#[derive(Clone, Debug)]
pub struct Statement<'tcx> {
    pub span: Span,
    pub kind: StatementKind<'tcx>,
}

#[derive(Clone, Debug)]
pub enum StatementKind<'tcx> {
    Assign(Box<(Place<'tcx>, Rvalue<'tcx>)>),
    /// Makes the place hold the variant (its fields were written already).
    SetDiscriminant { place: Box<Place<'tcx>>, variant_index: usize },
    Nop,
}

#[derive(Clone, Debug)]
pub struct Terminator<'tcx> {
    pub span: Span,
    pub kind: TerminatorKind<'tcx>,
}

#[derive(Clone, Debug)]
pub struct SwitchTargets {
    /// Value, then the block to go to.
    pub values: Vec<(u128, BasicBlock)>,
    pub otherwise: BasicBlock,
}

impl SwitchTargets {
    pub fn all_targets(&self) -> impl Iterator<Item = BasicBlock> + '_ {
        self.values.iter().map(|(_, b)| *b).chain(std::iter::once(self.otherwise))
    }
}

/// Why an `Assert` panics.
#[derive(Clone, Debug)]
pub enum AssertKind<'tcx> {
    BoundsCheck { len: Operand<'tcx>, index: Operand<'tcx> },
    Overflow(BinOp, Operand<'tcx>, Operand<'tcx>),
    DivisionByZero(Operand<'tcx>),
    RemainderByZero(Operand<'tcx>),
}

#[derive(Clone, Debug)]
pub enum TerminatorKind<'tcx> {
    Goto { target: BasicBlock },
    SwitchInt { discr: Operand<'tcx>, targets: SwitchTargets },
    Return,
    /// Control never gets here.
    Unreachable,
    /// `destination = func(args)`, then `target`; no target if the function
    /// never returns.
    Call { func: Operand<'tcx>, args: Vec<Operand<'tcx>>, destination: Place<'tcx>, target: Option<BasicBlock>, fn_span: Span },
    /// Panics with `msg` unless `cond` is `expected`.
    Assert { cond: Operand<'tcx>, expected: bool, msg: Box<AssertKind<'tcx>>, target: BasicBlock },
    InlineAsm { operands: Vec<Operand<'tcx>>, target: Option<BasicBlock> },
}

impl<'tcx> TerminatorKind<'tcx> {
    pub fn successors(&self) -> Vec<BasicBlock> {
        match self {
            TerminatorKind::Goto { target } | TerminatorKind::Assert { target, .. } => vec![*target],
            TerminatorKind::SwitchInt { targets, .. } => targets.all_targets().collect(),
            TerminatorKind::Call { target, .. } | TerminatorKind::InlineAsm { target, .. } => target.iter().copied().collect(),
            TerminatorKind::Return | TerminatorKind::Unreachable => Vec::new(),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Place<'tcx> {
    pub local: Local,
    pub projection: Vec<PlaceElem<'tcx>>,
}

impl<'tcx> Place<'tcx> {
    pub fn from_local(local: Local) -> Self {
        Place { local, projection: Vec::new() }
    }

    pub fn project(&self, elem: PlaceElem<'tcx>) -> Self {
        let mut p = self.clone();
        p.projection.push(elem);
        p
    }

    pub fn deref(&self) -> Self {
        self.project(PlaceElem::Deref)
    }

    pub fn field(&self, index: usize, ty: Ty<'tcx>) -> Self {
        self.project(PlaceElem::Field(index, ty))
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PlaceElem<'tcx> {
    Deref,
    Field(usize, Ty<'tcx>),
    Index(Local),
    /// `[offset]`, counting from the end if `from_end`; the slice has at
    /// least `min_length` elements.
    ConstantIndex { offset: u64, min_length: u64, from_end: bool },
    /// `[from..len - to]` if `from_end`, else `[from..to]`.
    Subslice { from: u64, to: u64, from_end: bool },
    /// The value as the given variant, to reach its fields.
    Downcast(usize),
}

#[derive(Clone, PartialEq, Debug)]
pub enum Operand<'tcx> {
    Copy(Place<'tcx>),
    Move(Place<'tcx>),
    Constant(Box<ConstOperand<'tcx>>),
}

impl<'tcx> Operand<'tcx> {
    pub fn place(&self) -> Option<&Place<'tcx>> {
        match self {
            Operand::Copy(p) | Operand::Move(p) => Some(p),
            Operand::Constant(_) => None,
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ConstOperand<'tcx> {
    pub span: Span,
    pub ty: Ty<'tcx>,
    pub value: ConstValue<'tcx>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ConstValue<'tcx> {
    /// An integer, `bool`, `char` or float, as its bits.
    Scalar(u128),
    /// A value of a zero-sized type: a function item, `()`.
    ZeroSized,
    /// A string literal's contents (escaped, as written).
    Str(Symbol),
    /// A named constant, evaluated later.
    Unevaluated(DefId, GenericArgsRef<'tcx>),
    /// The address of a static.
    StaticRef(DefId),
    /// A constant whose value isn't known yet (a const parameter, an array
    /// length expression).
    Ty(Const<'tcx>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
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
    /// A pointer plus an element count.
    Offset,
}

impl BinOp {
    pub fn from_ast(op: vsc_ast::BinOpKind) -> BinOp {
        use vsc_ast::BinOpKind as B;
        match op {
            B::Add => BinOp::Add,
            B::Sub => BinOp::Sub,
            B::Mul => BinOp::Mul,
            B::Div => BinOp::Div,
            B::Rem => BinOp::Rem,
            B::BitXor => BinOp::BitXor,
            B::BitAnd => BinOp::BitAnd,
            B::BitOr => BinOp::BitOr,
            B::Shl => BinOp::Shl,
            B::Shr => BinOp::Shr,
            B::Eq => BinOp::Eq,
            B::Lt => BinOp::Lt,
            B::Le => BinOp::Le,
            B::Ne => BinOp::Ne,
            B::Ge => BinOp::Ge,
            B::Gt => BinOp::Gt,
            B::And | B::Or => unreachable!("`&&` and `||` are control flow in MIR"),
        }
    }

    pub fn is_comparison(self) -> bool {
        matches!(self, BinOp::Eq | BinOp::Lt | BinOp::Le | BinOp::Ne | BinOp::Ge | BinOp::Gt)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CastKind {
    IntToInt,
    IntToFloat,
    FloatToInt,
    FloatToFloat,
    PtrToPtr,
    /// A pointer to an integer.
    PointerExposeProvenance,
    /// An integer to a pointer.
    PointerWithExposedProvenance,
    PointerCoercion(PointerCoercion),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BorrowKind {
    Shared,
    Mut,
}

#[derive(Clone, Debug)]
pub enum AggregateKind<'tcx> {
    Array(Ty<'tcx>),
    Tuple,
    /// A struct, enum variant or union (with the field set).
    Adt { def_id: DefId, variant_index: usize, args: GenericArgsRef<'tcx>, active_field: Option<usize> },
    /// A function literal and its captured values.
    Closure(DefId, GenericArgsRef<'tcx>),
}

#[derive(Clone, Debug)]
pub enum Rvalue<'tcx> {
    Use(Operand<'tcx>),
    Repeat(Operand<'tcx>, Const<'tcx>),
    Ref(BorrowKind, Place<'tcx>),
    RawPtr(Mutability, Place<'tcx>),
    /// The length of an array or slice.
    Len(Place<'tcx>),
    Cast(CastKind, Operand<'tcx>, Ty<'tcx>),
    BinaryOp(BinOp, Box<(Operand<'tcx>, Operand<'tcx>)>),
    /// The result and whether it overflowed: `(T, bool)`.
    CheckedBinaryOp(BinOp, Box<(Operand<'tcx>, Operand<'tcx>)>),
    UnaryOp(UnOp, Operand<'tcx>),
    /// An enum value's discriminant.
    Discriminant(Place<'tcx>),
    Aggregate(Box<AggregateKind<'tcx>>, Vec<Operand<'tcx>>),
    /// A class value holding the operand (exception 3): Rust's
    /// `Rc::new(RefCell::new(value))`.
    ClassNew(Operand<'tcx>),
}
