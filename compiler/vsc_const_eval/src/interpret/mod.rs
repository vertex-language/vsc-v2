//! The MIR interpreter (rustc's `rustc_const_eval::interpret`, as Miri uses
//! it): runs a program's MIR directly, so programs can run before code
//! generation exists.
//!
//! Each call pushes a frame whose locals are allocations. Generic functions
//! run with their frame's arguments, which types are instantiated with on
//! demand; trait methods are resolved to the impl's method for the concrete
//! types, as monomorphization will. Library functions marked
//! `@intrinsic("name")` and a few lang items are carried out natively
//! (`intrinsics.rs`); formatting follows Rust's (`fmt.rs`).
//!
//! Undefined behavior the interpreter can see (reading uninitialized or
//! freed memory, dereferencing a non-pointer) stops the program with an
//! error; a panic stops it as Rust's does, with exit code 101.

mod fmt;
mod intrinsics;
mod operator;
pub mod value;

use std::fmt::Write as _;

use vsc_data_structures::FxHashMap;
use vsc_hir::def::DefKind;
use vsc_index::Idx;
use vsc_middle::mir::*;
use vsc_middle::ty::{self, GenericArgsRef, Ty, TyCtxt, TyKind};
use vsc_span::def_id::DefId;
use vsc_span::{Span, Symbol};

pub use value::*;

/// How a run ended other than by returning.
#[derive(Debug)]
pub enum InterpError {
    /// A panic: its message and where it happened.
    Panic(String, Span),
    /// Undefined behavior the interpreter caught.
    Ub(String, Span),
    /// Something the interpreter can't do yet.
    Unsupported(String, Span),
}

pub type InterpResult<T> = Result<T, InterpError>;

/// Where the program's output goes.
pub trait Output {
    fn stdout(&mut self, s: &str);
    fn stderr(&mut self, s: &str);
}

struct Frame<'tcx> {
    body: &'tcx Body<'tcx>,
    args: GenericArgsRef<'tcx>,
    locals: Vec<AllocId>,
    block: BasicBlock,
    stmt: usize,
    /// Where the return value goes in the caller, and where it continues.
    ret: Option<(Location, Option<BasicBlock>)>,
    /// Allocations to free on return besides the locals (a closure's
    /// receiver).
    extra: Vec<AllocId>,
}

pub struct Interpreter<'a, 'tcx> {
    pub tcx: TyCtxt<'tcx>,
    pub out: &'a mut dyn Output,
    memory: Vec<Option<Value<'tcx>>>,
    stack: Vec<Frame<'tcx>>,
    statics: FxHashMap<DefId, AllocId>,
    strings: FxHashMap<Symbol, AllocId>,
    intrinsic_names: FxHashMap<DefId, Option<Symbol>>,
    steps: u64,
    pub step_limit: u64,
}

impl<'a, 'tcx> Interpreter<'a, 'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, out: &'a mut dyn Output) -> Self {
        Interpreter {
            tcx,
            out,
            memory: Vec::new(),
            stack: Vec::new(),
            statics: FxHashMap::default(),
            strings: FxHashMap::default(),
            intrinsic_names: FxHashMap::default(),
            steps: 0,
            step_limit: 200_000_000,
        }
    }

    // -----------------------------------------------------------------------
    // Memory
    // -----------------------------------------------------------------------

    pub fn allocate(&mut self, v: Value<'tcx>) -> AllocId {
        self.memory.push(Some(v));
        AllocId(self.memory.len() - 1)
    }

    fn free(&mut self, a: AllocId) {
        self.memory[a.0] = None;
    }

    fn span(&self) -> Span {
        match self.stack.last() {
            Some(f) => {
                let data = &f.body.basic_blocks[f.block];
                match data.statements.get(f.stmt) {
                    Some(s) => s.span,
                    None => data.terminator.as_ref().map_or(f.body.span, |t| t.span),
                }
            }
            None => vsc_span::DUMMY_SP,
        }
    }

    pub fn ub<T>(&self, msg: impl Into<String>) -> InterpResult<T> {
        Err(InterpError::Ub(msg.into(), self.span()))
    }

    pub fn unsupported<T>(&self, msg: impl Into<String>) -> InterpResult<T> {
        Err(InterpError::Unsupported(msg.into(), self.span()))
    }

    pub fn panic<T>(&self, msg: impl Into<String>) -> InterpResult<T> {
        Err(InterpError::Panic(msg.into(), self.span()))
    }

    fn navigate<'v>(v: &'v Value<'tcx>, path: &[usize]) -> Option<&'v Value<'tcx>> {
        let mut cur = v;
        for &i in path {
            cur = match cur {
                Value::Tuple(xs) | Value::Array(xs) | Value::Vec(xs) => xs.get(i)?,
                Value::Adt { fields, .. } => fields.get(i)?,
                Value::Closure { captures, .. } => captures.get(i)?,
                _ => return None,
            };
        }
        Some(cur)
    }

    fn navigate_mut<'v>(v: &'v mut Value<'tcx>, path: &[usize]) -> Option<&'v mut Value<'tcx>> {
        let mut cur = v;
        for &i in path {
            cur = match cur {
                Value::Tuple(xs) | Value::Array(xs) | Value::Vec(xs) => xs.get_mut(i)?,
                Value::Adt { fields, .. } => fields.get_mut(i)?,
                Value::Closure { captures, .. } => captures.get_mut(i)?,
                _ => return None,
            };
        }
        Some(cur)
    }

    pub fn read(&self, loc: &Location) -> InterpResult<Value<'tcx>> {
        if loc.slice.is_some() {
            return self.unsupported("reading a slice by value");
        }
        let Some(Some(root)) = self.memory.get(loc.alloc.0) else { return self.ub("using memory that was freed") };
        match Self::navigate(root, &loc.path) {
            Some(Value::Uninit) => self.ub("using uninitialized data"),
            Some(v) => Ok(v.clone()),
            None => self.ub("a place outside its value"),
        }
    }

    pub fn write(&mut self, loc: &Location, v: Value<'tcx>) -> InterpResult<()> {
        if loc.slice.is_some() {
            return self.unsupported("writing a slice by value");
        }
        let span = self.span();
        let Some(Some(root)) = self.memory.get_mut(loc.alloc.0) else { return Err(InterpError::Ub("writing memory that was freed".into(), span)) };
        match Self::navigate_mut(root, &loc.path) {
            Some(slot) => {
                *slot = v;
                Ok(())
            }
            None => Err(InterpError::Ub("writing into uninitialized data".into(), span)),
        }
    }

    /// The location a pointer value points to.
    pub fn deref_value(&self, v: &Value<'tcx>) -> InterpResult<Location> {
        match v {
            Value::Ptr(p) => Ok(Location { alloc: p.alloc, path: p.path.clone(), slice: None }),
            Value::SlicePtr { array, start, len } => Ok(Location { alloc: array.alloc, path: array.path.clone(), slice: Some((*start, *len)) }),
            Value::Class(a) => Ok(Location::of(*a)),
            Value::Uninit => self.ub("dereferencing an uninitialized pointer"),
            _ => self.ub("dereferencing a value that isn't a pointer"),
        }
    }

    /// A pointer to a location.
    pub fn pointer_to(&self, loc: Location) -> Value<'tcx> {
        let array = Pointer { alloc: loc.alloc, path: loc.path };
        match loc.slice {
            Some((start, len)) => Value::SlicePtr { array, start, len },
            None => Value::Ptr(array),
        }
    }

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    fn frame(&self) -> &Frame<'tcx> {
        self.stack.last().unwrap()
    }

    /// A type of the current frame, with its generic arguments filled in.
    pub fn monomorphize(&self, ty: Ty<'tcx>) -> Ty<'tcx> {
        let args = self.frame().args;
        self.tcx.erase_regions(self.tcx.instantiate(ty, args))
    }

    fn place_ty(&self, p: &Place<'tcx>) -> Ty<'tcx> {
        let tcx = self.tcx;
        let mut t = self.monomorphize(self.frame().body.local_decls[p.local].ty);
        for elem in &p.projection {
            t = match *elem {
                PlaceElem::Deref => match *t.kind() {
                    TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => inner,
                    TyKind::Adt(_, args) if !args.is_empty() => args[0].expect_ty(),
                    _ => tcx.types.error,
                },
                PlaceElem::Field(_, fty) => self.monomorphize(fty),
                PlaceElem::Index(_) | PlaceElem::ConstantIndex { .. } => match *t.kind() {
                    TyKind::Array(e, _) | TyKind::Slice(e) => e,
                    _ => tcx.types.error,
                },
                PlaceElem::Subslice { .. } | PlaceElem::Downcast(_) => t,
            };
        }
        t
    }

    pub fn int_size(&self, ty: Ty<'tcx>) -> Option<(u32, bool)> {
        use vsc_middle::ty::{IntTy, UintTy};
        let ptr = self.tcx.target.pointer_width;
        Some(match *ty.kind() {
            TyKind::Int(i) => (
                match i {
                    IntTy::I8 => 8,
                    IntTy::I16 => 16,
                    IntTy::I32 => 32,
                    IntTy::I64 => 64,
                    IntTy::I128 => 128,
                    IntTy::Isize => ptr,
                },
                true,
            ),
            TyKind::Uint(u) => (
                match u {
                    UintTy::U8 => 8,
                    UintTy::U16 => 16,
                    UintTy::U32 => 32,
                    UintTy::U64 => 64,
                    UintTy::U128 => 128,
                    UintTy::Usize => ptr,
                },
                false,
            ),
            _ => return None,
        })
    }

    pub fn usize(&self, n: usize) -> Value<'tcx> {
        Value::Int(Int::new(n as i128, self.tcx.target.pointer_width, false))
    }

    pub fn as_usize(&self, v: &Value<'tcx>) -> InterpResult<usize> {
        match v {
            Value::Int(i) => Ok(i.to_i128() as usize),
            _ => self.ub("expected an integer"),
        }
    }

    // -----------------------------------------------------------------------
    // Places, operands, constants
    // -----------------------------------------------------------------------

    pub fn eval_place(&mut self, p: &Place<'tcx>) -> InterpResult<Location> {
        let mut loc = Location::of(self.frame().locals[p.local.index()]);
        for elem in &p.projection {
            match *elem {
                PlaceElem::Deref => {
                    let ptr = self.read(&loc)?;
                    loc = self.deref_value(&ptr)?;
                }
                PlaceElem::Field(i, _) => loc.path.push(i),
                PlaceElem::Downcast(_) => {}
                PlaceElem::Index(l) => {
                    let idx = self.read(&Location::of(self.frame().locals[l.index()]))?;
                    let i = self.as_usize(&idx)?;
                    match loc.slice.take() {
                        Some((start, len)) => {
                            if i >= len {
                                return self.ub(format!("index {i} out of bounds of a slice of length {len}"));
                            }
                            loc.path.push(start + i);
                        }
                        None => loc.path.push(i),
                    }
                }
                PlaceElem::ConstantIndex { offset, from_end, .. } => {
                    let (start, len) = match loc.slice.take() {
                        Some(s) => s,
                        None => (0, self.array_len(&loc)?),
                    };
                    let i = if from_end { len - offset as usize } else { offset as usize };
                    loc.path.push(start + i);
                }
                PlaceElem::Subslice { from, to, from_end } => {
                    let (start, len) = match loc.slice {
                        Some(s) => s,
                        None => (0, self.array_len(&loc)?),
                    };
                    let (from, to) = (from as usize, to as usize);
                    let new_len = if from_end { len - from - to } else { to - from };
                    loc.slice = Some((start + from, new_len));
                }
            }
        }
        Ok(loc)
    }

    fn array_len(&self, loc: &Location) -> InterpResult<usize> {
        match self.read(loc)? {
            Value::Array(xs) => Ok(xs.len()),
            _ => self.ub("taking the length of a value that isn't an array"),
        }
    }

    pub fn eval_operand(&mut self, o: &Operand<'tcx>) -> InterpResult<Value<'tcx>> {
        match o {
            Operand::Copy(p) | Operand::Move(p) => {
                let loc = self.eval_place(p)?;
                self.read(&loc)
            }
            Operand::Constant(c) => self.eval_const(c),
        }
    }

    fn eval_const(&mut self, c: &ConstOperand<'tcx>) -> InterpResult<Value<'tcx>> {
        let ty = self.monomorphize(c.ty);
        match c.value {
            ConstValue::Scalar(bits) => Ok(self.scalar(bits, ty)),
            ConstValue::ZeroSized => match *ty.kind() {
                TyKind::FnDef(d, args) => Ok(Value::FnDef(d, args)),
                _ => Ok(Value::unit()),
            },
            ConstValue::Str(sym) => Ok(self.string_literal(sym)),
            ConstValue::Unevaluated(d, args) => {
                let args = self.tcx.erase_regions_args(self.tcx.instantiate_args(args, self.frame().args));
                self.eval_const_item(d, args)
            }
            ConstValue::StaticRef(d) => {
                let a = self.static_alloc(d)?;
                Ok(Value::Ptr(Pointer { alloc: a, path: Vec::new() }))
            }
            ConstValue::Ty(k) => self.eval_ty_const(k),
        }
    }

    pub fn scalar(&self, bits: u128, ty: Ty<'tcx>) -> Value<'tcx> {
        match *ty.kind() {
            TyKind::Bool => Value::Bool(bits != 0),
            TyKind::Char => Value::Char(char::from_u32(bits as u32).unwrap_or('\0')),
            TyKind::Float(ty::FloatTy::F32) => Value::F32(f32::from_bits(bits as u32)),
            TyKind::Float(ty::FloatTy::F64) => Value::F64(f64::from_bits(bits as u64)),
            _ => match self.int_size(ty) {
                Some((size, signed)) => Value::Int(Int::from_bits(bits, size, signed)),
                None => Value::Int(Int::from_bits(bits, 128, false)),
            },
        }
    }

    /// A string literal: a slice pointer into an array of its bytes.
    fn string_literal(&mut self, sym: Symbol) -> Value<'tcx> {
        let alloc = match self.strings.get(&sym) {
            Some(&a) => a,
            None => {
                let text = unescape(sym.as_str());
                let bytes = text.bytes().map(|b| Value::Int(Int::new(b as i128, 8, false))).collect();
                let a = self.allocate(Value::Array(bytes));
                self.strings.insert(sym, a);
                a
            }
        };
        let len = match &self.memory[alloc.0] {
            Some(Value::Array(xs)) => xs.len(),
            _ => 0,
        };
        Value::SlicePtr { array: Pointer { alloc, path: Vec::new() }, start: 0, len }
    }

    /// The text of a `&string` value.
    pub fn read_str(&self, v: &Value<'tcx>) -> InterpResult<String> {
        let Value::SlicePtr { array, start, len } = v else { return self.ub("expected a string slice") };
        let arr = self.read(&Location { alloc: array.alloc, path: array.path.clone(), slice: None })?;
        let Value::Array(xs) = arr else { return self.ub("a string slice that doesn't point into bytes") };
        let bytes: Vec<u8> = xs[*start..*start + *len]
            .iter()
            .map(|b| match b {
                Value::Int(i) => i.bits as u8,
                _ => 0,
            })
            .collect();
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn eval_ty_const(&mut self, k: ty::Const<'tcx>) -> InterpResult<Value<'tcx>> {
        let k = self.tcx.instantiate_const(k, self.frame().args);
        match *k.kind() {
            ty::ConstKind::Value(t, bits) => Ok(self.scalar(bits, t)),
            ty::ConstKind::Unevaluated(d, args) => self.eval_const_item(d, args),
            _ => self.unsupported("a constant whose value isn't known"),
        }
    }

    /// The value of a constant item, an associated constant or an
    /// anonymous constant.
    fn eval_const_item(&mut self, d: DefId, args: GenericArgsRef<'tcx>) -> InterpResult<Value<'tcx>> {
        let (d, args) = self.resolve(d, args);
        if !d.is_local() || self.tcx.body_owned_by(d.expect_local()).is_none() {
            return self.unsupported(format!("evaluating `{}`, which has no body", self.tcx.def_path_str(d)));
        }
        let body = self.tcx.mir_built(d);
        self.call_body(body, args, Vec::new(), Vec::new())
    }

    fn static_alloc(&mut self, d: DefId) -> InterpResult<AllocId> {
        if let Some(&a) = self.statics.get(&d) {
            return Ok(a);
        }
        let foreign = self.tcx.opt_parent(d).is_some_and(|p| self.tcx.def_kind(p) == DefKind::ForeignMod);
        if foreign {
            return self.unsupported(format!("using extern static `{}`", self.tcx.def_path_str(d)));
        }
        let body = self.tcx.mir_built(d);
        let v = self.call_body(body, &[], Vec::new(), Vec::new())?;
        let a = self.allocate(v);
        self.statics.insert(d, a);
        Ok(a)
    }

    // -----------------------------------------------------------------------
    // Rvalues
    // -----------------------------------------------------------------------

    fn eval_rvalue(&mut self, r: &Rvalue<'tcx>, dest_ty: Ty<'tcx>) -> InterpResult<Value<'tcx>> {
        let tcx = self.tcx;
        match r {
            Rvalue::Use(o) => self.eval_operand(o),
            Rvalue::Repeat(o, n) => {
                let v = self.eval_operand(o)?;
                let n = self.eval_ty_const(*n)?;
                let n = self.as_usize(&n)?;
                Ok(Value::Array(vec![v; n]))
            }
            Rvalue::Ref(_, p) | Rvalue::RawPtr(_, p) => {
                let loc = self.eval_place(p)?;
                Ok(self.pointer_to(loc))
            }
            Rvalue::Len(p) => {
                let loc = self.eval_place(p)?;
                let n = match loc.slice {
                    Some((_, len)) => len,
                    None => self.array_len(&loc)?,
                };
                Ok(self.usize(n))
            }
            Rvalue::Cast(kind, o, ty) => {
                let v = self.eval_operand(o)?;
                let ty = self.monomorphize(*ty);
                self.cast(*kind, v, ty)
            }
            Rvalue::BinaryOp(op, b) => {
                let l = self.eval_operand(&b.0)?;
                let r = self.eval_operand(&b.1)?;
                self.binary_op(*op, l, r)
            }
            Rvalue::CheckedBinaryOp(op, b) => {
                let l = self.eval_operand(&b.0)?;
                let r = self.eval_operand(&b.1)?;
                let (v, overflow) = self.checked_op(*op, l, r)?;
                Ok(Value::Tuple(vec![v, Value::Bool(overflow)]))
            }
            Rvalue::UnaryOp(op, o) => {
                let v = self.eval_operand(o)?;
                self.unary_op(*op, v)
            }
            Rvalue::Discriminant(p) => {
                let ty = self.place_ty(p);
                let loc = self.eval_place(p)?;
                let v = self.read(&loc)?;
                let (TyKind::Adt(adt, _), Value::Adt { variant, .. }) = (ty.kind(), v) else {
                    return self.ub("taking the discriminant of a value that isn't an enum");
                };
                let d = vsc_mir_build::builder::discriminant_value(tcx, *adt, variant);
                Ok(Value::Int(Int::new(d, tcx.target.pointer_width, true)))
            }
            Rvalue::Aggregate(kind, ops) => {
                let mut vals = Vec::with_capacity(ops.len());
                for o in ops {
                    vals.push(self.eval_operand(o)?);
                }
                Ok(match &**kind {
                    AggregateKind::Tuple => Value::Tuple(vals),
                    AggregateKind::Array(_) => Value::Array(vals),
                    AggregateKind::Adt { def_id, variant_index, active_field, .. } => match active_field {
                        // A union holds one field at a time.
                        Some(f) => {
                            let n = tcx.adt_def(*def_id).variants()[0].fields.len();
                            let mut fields = vec![Value::Uninit; n];
                            fields[*f] = vals.into_iter().next().unwrap_or(Value::Uninit);
                            Value::Adt { variant: 0, fields }
                        }
                        None => Value::Adt { variant: *variant_index, fields: vals },
                    },
                    AggregateKind::Closure(d, _) => Value::Closure { def: *d, args: self.frame().args, captures: vals },
                })
            }
            Rvalue::ClassNew(o) => {
                let v = self.eval_operand(o)?;
                let _ = dest_ty;
                Ok(Value::Class(self.allocate(v)))
            }
        }
    }

    // -----------------------------------------------------------------------
    // Calls
    // -----------------------------------------------------------------------

    /// A trait method's implementation for these (concrete) arguments, or
    /// the function itself.
    pub fn resolve(&self, def: DefId, args: GenericArgsRef<'tcx>) -> (DefId, GenericArgsRef<'tcx>) {
        let tcx = self.tcx;
        let Some(parent) = tcx.opt_parent(def) else { return (def, args) };
        if tcx.def_kind(parent) != DefKind::Trait || !matches!(tcx.def_kind(def), DefKind::AssocFn | DefKind::AssocConst) {
            return (def, args);
        }
        let n = tcx.generics_of(parent).count().min(args.len());
        let tr = ty::TraitRef { def_id: parent, args: tcx.mk_args(&args[..n]) };
        let infcx = vsc_infer::InferCtxt::new(tcx);
        let env = vsc_trait_selection::ParamEnv::default();
        let selcx = vsc_trait_selection::SelectionCtxt::new(&infcx, &env);
        if let vsc_trait_selection::Selection::Selected(vsc_trait_selection::Candidate::Impl(imp, impl_args), _) = selcx.select(tr, tcx.def_span(def)) {
            let impl_args = infcx.resolve_args(impl_args);
            let name = tcx.item_name(def);
            if let Some(item) = tcx.associated_items(imp).iter().find(|i| i.name == name) {
                let mut full = impl_args.to_vec();
                full.extend_from_slice(&args[n..]);
                return (item.def_id, tcx.erase_regions_args(tcx.mk_args(&full)));
            }
        }
        (def, args)
    }

    /// Runs a body to completion with these arguments.
    pub fn call_body(&mut self, body: &'tcx Body<'tcx>, args: GenericArgsRef<'tcx>, arg_values: Vec<Value<'tcx>>, extra: Vec<AllocId>) -> InterpResult<Value<'tcx>> {
        let depth = self.stack.len();
        self.push_frame(body, args, arg_values, None, extra);
        self.run_until(depth)
    }

    fn push_frame(
        &mut self,
        body: &'tcx Body<'tcx>,
        args: GenericArgsRef<'tcx>,
        arg_values: Vec<Value<'tcx>>,
        ret: Option<(Location, Option<BasicBlock>)>,
        extra: Vec<AllocId>,
    ) {
        let mut locals = Vec::with_capacity(body.local_decls.len());
        let mut arg_values = arg_values.into_iter();
        for (i, _) in body.local_decls.iter_enumerated() {
            let v = if i.index() >= 1 && i.index() <= body.arg_count { arg_values.next().unwrap_or(Value::Uninit) } else { Value::Uninit };
            locals.push(self.allocate(v));
        }
        self.stack.push(Frame { body, args, locals, block: START_BLOCK, stmt: 0, ret, extra });
    }

    /// Steps until the stack is back to `depth` frames; returns the last
    /// frame's return value.
    fn run_until(&mut self, depth: usize) -> InterpResult<Value<'tcx>> {
        loop {
            if let Some(v) = self.step(depth)? {
                return Ok(v);
            }
        }
    }

    /// One statement or terminator. Returns the value when the frame at
    /// `depth` returns.
    fn step(&mut self, depth: usize) -> InterpResult<Option<Value<'tcx>>> {
        self.steps += 1;
        if self.steps > self.step_limit {
            return self.unsupported(format!("the program ran for more than {} steps", self.step_limit));
        }
        let (body, block, stmt) = {
            let f = self.frame();
            (f.body, f.block, f.stmt)
        };
        let data = &body.basic_blocks[block];
        if let Some(s) = data.statements.get(stmt) {
            if let StatementKind::Assign(a) = &s.kind {
                let dest_ty = self.place_ty(&a.0);
                let v = self.eval_rvalue(&a.1, dest_ty)?;
                let loc = self.eval_place(&a.0)?;
                self.write(&loc, v)?;
            }
            self.stack.last_mut().unwrap().stmt += 1;
            return Ok(None);
        }
        let term = data.terminator();
        match &term.kind {
            TerminatorKind::Goto { target } => self.jump(*target),
            TerminatorKind::SwitchInt { discr, targets } => {
                let v = self.eval_operand(discr)?;
                let bits = match v {
                    Value::Bool(b) => b as u128,
                    Value::Int(i) => i.bits,
                    Value::Char(c) => c as u128,
                    _ => return self.ub("switching on a value that isn't an integer"),
                };
                let target = targets.values.iter().find(|(x, _)| *x == bits).map_or(targets.otherwise, |(_, b)| *b);
                self.jump(target);
            }
            TerminatorKind::Return => {
                let frame = self.stack.pop().unwrap();
                let v = self.read(&Location::of(frame.locals[0])).unwrap_or(Value::unit());
                for a in frame.locals.iter().chain(frame.extra.iter()) {
                    self.free(*a);
                }
                match frame.ret {
                    Some((dest, target)) => {
                        self.write(&dest, v)?;
                        match target {
                            Some(t) => self.jump(t),
                            None => return self.ub("returned from a function that never returns"),
                        }
                    }
                    None => {
                        if self.stack.len() == depth {
                            return Ok(Some(v));
                        }
                    }
                }
            }
            TerminatorKind::Unreachable => return self.ub("entered unreachable code"),
            TerminatorKind::Assert { cond, expected, msg, target } => {
                let c = match self.eval_operand(cond)? {
                    Value::Bool(b) => b,
                    _ => return self.ub("asserting a value that isn't a `bool`"),
                };
                if c != *expected {
                    let m = self.assert_message(msg)?;
                    return self.panic(m);
                }
                self.jump(*target);
            }
            TerminatorKind::Call { func, args, destination, target, .. } => {
                let f = self.eval_operand(func)?;
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval_operand(a)?);
                }
                let dest = self.eval_place(destination)?;
                self.call(f, vals, dest, *target)?;
            }
            TerminatorKind::InlineAsm { .. } => return self.unsupported("inline assembly"),
        }
        Ok(None)
    }

    fn jump(&mut self, target: BasicBlock) {
        let f = self.stack.last_mut().unwrap();
        f.block = target;
        f.stmt = 0;
    }

    fn call(&mut self, f: Value<'tcx>, vals: Vec<Value<'tcx>>, dest: Location, target: Option<BasicBlock>) -> InterpResult<()> {
        let tcx = self.tcx;
        match f {
            Value::FnDef(def, args) => {
                let (def, args) = self.resolve(def, args);
                // The function traits' methods for what they're built in
                // for: call the value with the tuple's elements.
                if tcx.opt_parent(def).is_some_and(|p| vsc_trait_selection::fn_trait_kind(tcx, p).is_some()) && vals.len() == 2 {
                    let mut vals = vals;
                    let Value::Tuple(untupled) = vals.pop().unwrap() else { return self.ub("function trait arguments that aren't a tuple") };
                    let callee = vals.pop().unwrap();
                    return self.call(callee, untupled, dest, target);
                }
                if let Some(v) = self.call_intrinsic(def, args, &vals)? {
                    self.write(&dest, v)?;
                    match target {
                        Some(t) => self.jump(t),
                        None => return self.ub("an intrinsic that never returns returned"),
                    }
                    return Ok(());
                }
                let Some(local) = def.as_local() else { return self.unsupported("calling a function from another crate") };
                if tcx.body_owned_by(local).is_none() && tcx.def_kind(def) != DefKind::Closure {
                    let what = match tcx.opt_parent(def).map(|p| tcx.def_kind(p)) {
                        Some(DefKind::ForeignMod) => "foreign function",
                        _ => "function without a body",
                    };
                    return self.unsupported(format!("calling {what} `{}`", tcx.def_path_str(def)));
                }
                let body = tcx.mir_built(def);
                self.push_frame(body, args, vals, Some((dest, target)), Vec::new());
                Ok(())
            }
            Value::Closure { def, args, captures } => {
                // The body takes the literal by reference.
                let env = self.allocate(Value::Closure { def, args, captures });
                let mut all = vec![Value::Ptr(Pointer { alloc: env, path: Vec::new() })];
                all.extend(vals);
                let body = tcx.mir_built(def);
                self.push_frame(body, args, all, Some((dest, target)), vec![env]);
                Ok(())
            }
            Value::Func(inner) => self.call(*inner, vals, dest, target),
            Value::Ptr(p) => {
                let v = self.read(&Location { alloc: p.alloc, path: p.path, slice: None })?;
                self.call(v, vals, dest, target)
            }
            _ => self.ub("calling a value that isn't a function"),
        }
    }

    fn assert_message(&mut self, msg: &AssertKind<'tcx>) -> InterpResult<String> {
        Ok(match msg {
            AssertKind::BoundsCheck { len, index } => {
                let len = self.eval_operand(len)?;
                let index = self.eval_operand(index)?;
                format!("index out of bounds: the len is {} but the index is {}", self.as_usize(&len)?, self.as_usize(&index)?)
            }
            AssertKind::Overflow(op, ..) => {
                let what = match op {
                    BinOp::Add => "add",
                    BinOp::Sub => "subtract",
                    BinOp::Mul => "multiply",
                    BinOp::Div => "divide",
                    BinOp::Rem => "calculate the remainder",
                    BinOp::Shl => "shift left",
                    BinOp::Shr => "shift right",
                    _ => "compute",
                };
                format!("attempt to {what} with overflow")
            }
            AssertKind::DivisionByZero(_) => "attempt to divide by zero".to_string(),
            AssertKind::RemainderByZero(_) => "attempt to calculate the remainder with a divisor of zero".to_string(),
        })
    }

    /// The name in an `@intrinsic("name")` attribute.
    pub fn intrinsic_name(&mut self, def: DefId) -> Option<Symbol> {
        if let Some(n) = self.intrinsic_names.get(&def) {
            return *n;
        }
        let name = def.as_local().and_then(|l| {
            self.tcx.owner_attrs(l).iter().find_map(|a| {
                let vsc_ast::AttrKind::Normal(n) = &a.kind else { return None };
                if a.name()?.as_str() != "intrinsic" {
                    return None;
                }
                match &n.args {
                    vsc_ast::AttrArgs::Delimited(d) => {
                        let text = vsc_ast_pretty::pprust::tts_to_string(&d.tokens);
                        Some(Symbol::intern(text.trim().trim_matches('"')))
                    }
                    _ => None,
                }
            })
        });
        self.intrinsic_names.insert(def, name);
        name
    }

    /// Where a span is, as Rust prints it in a panic: `file:line:column`.
    pub fn location_str(&self, span: Span) -> String {
        let sm = &self.tcx.sess.source_map;
        match (sm.lookup_source_file(span.lo()), sm.lookup_char_pos(span.lo())) {
            (Some(f), Some(loc)) => {
                let mut s = String::new();
                let _ = write!(s, "{}:{}:{}", f.name, loc.line, loc.col + 1);
                s
            }
            _ => "<unknown>".to_string(),
        }
    }
}

/// A string literal's text, with its escapes processed.
fn unescape(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    vsc_lexer::unescape::unescape_unicode(src, vsc_lexer::unescape::Mode::Str, &mut |_, r| {
        if let Ok(c) = r {
            out.push(c);
        }
    });
    out
}

/// How a run of `main` ended.
pub enum RunResult {
    Exited,
    Panicked,
    /// Undefined behavior or something unsupported, reported as an error.
    Failed,
}

/// Runs the crate's `main`.
pub fn run_main(tcx: TyCtxt<'_>, out: &mut dyn Output) -> RunResult {
    run_main_with_limit(tcx, out, 200_000_000)
}

/// Runs `main`, stopping after `step_limit` steps.
pub fn run_main_with_limit(tcx: TyCtxt<'_>, out: &mut dyn Output, step_limit: u64) -> RunResult {
    let main = tcx.defs.iter_enumerated().find(|(_, d)| d.kind == DefKind::Fn && d.name.as_str() == "main" && d.parent == Some(vsc_span::def_id::CRATE_DEF_ID));
    let Some((main, _)) = main else {
        tcx.dcx().struct_err("`main` function not found in the crate").with_code(vsc_errors::ErrCode(601)).emit();
        return RunResult::Failed;
    };
    let body = tcx.mir_built(main.to_def_id());
    let mut interp = Interpreter::new(tcx, out);
    interp.step_limit = step_limit;
    match interp.call_body(body, &[], Vec::new(), Vec::new()) {
        Ok(_) => RunResult::Exited,
        Err(InterpError::Panic(msg, span)) => {
            let loc = interp.location_str(span);
            interp.out.stderr(&format!("thread 'main' panicked at {loc}:\n{msg}\nnote: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n"));
            RunResult::Panicked
        }
        Err(InterpError::Ub(msg, span)) => {
            tcx.dcx().struct_span_err(span, format!("undefined behavior: {msg}")).with_note("the interpreter stopped the program here").emit();
            RunResult::Failed
        }
        Err(InterpError::Unsupported(msg, span)) => {
            tcx.dcx().struct_span_err(span, format!("unsupported: {msg}")).with_note("the MIR interpreter can't run this yet").emit();
            RunResult::Failed
        }
    }
}
