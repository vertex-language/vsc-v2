//! Library functions the interpreter carries out itself: those marked
//! `@intrinsic("name")`, and the lang items for printing, formatting,
//! panicking and building vectors. Until `library/` can be built, these
//! stand in for code that will call the operating system or manage memory.

use std::rc::Rc;

use vsc_hir::LangItem;
use vsc_middle::ty::{GenericArg, GenericArgsRef, Ty};
use vsc_span::def_id::DefId;

use super::*;

impl<'a, 'tcx> Interpreter<'a, 'tcx> {
    /// Runs `def` natively if it's an intrinsic; `None` if it isn't.
    pub fn call_intrinsic(&mut self, def: DefId, args: GenericArgsRef<'tcx>, vals: &[Value<'tcx>]) -> InterpResult<Option<Value<'tcx>>> {
        if let Some(item) = self.tcx.lang_items().from_def_id(def) {
            if let Some(v) = self.lang_item(item, args, vals)? {
                return Ok(Some(v));
            }
        }
        let Some(name) = self.intrinsic_name(def) else { return Ok(None) };
        let ty_arg = |i: usize| -> Option<Ty<'tcx>> {
            args.iter().filter_map(|a| if let GenericArg::Type(t) = a { Some(*t) } else { None }).nth(i)
        };
        let last_ty = args.iter().rev().find_map(|a| if let GenericArg::Type(t) = a { Some(*t) } else { None });
        let v = match name.as_str() {
            "format_new_const" => {
                let pieces = self.read_str_slice(&vals[0])?;
                Value::FmtArgs(Rc::new(FmtArgs { pieces, args: Vec::new(), placeholders: None }))
            }
            "format_new_v1" | "format_new_v1_formatted" => {
                let pieces = self.read_str_slice(&vals[0])?;
                let args = self.read_slice(&vals[1])?.into_iter().map(|a| match a {
                    Value::FmtArg(f) => Ok(f),
                    _ => self.ub("a format argument that isn't one"),
                });
                let args = args.collect::<InterpResult<Vec<_>>>()?;
                let placeholders = if name.as_str() == "format_new_v1_formatted" {
                    let ps = self.read_slice(&vals[2])?.into_iter().map(|p| match p {
                        Value::Placeholder(p) => Ok(p),
                        _ => self.ub("a placeholder that isn't one"),
                    });
                    Some(ps.collect::<InterpResult<Vec<_>>>()?)
                } else {
                    None
                };
                Value::FmtArgs(Rc::new(FmtArgs { pieces, args, placeholders }))
            }
            n if n.starts_with("format_arg_") => {
                let trait_ = match &n["format_arg_".len()..] {
                    "display" => FmtTrait::Display,
                    "debug" => FmtTrait::Debug,
                    "lower_exp" => FmtTrait::LowerExp,
                    "upper_exp" => FmtTrait::UpperExp,
                    "octal" => FmtTrait::Octal,
                    "pointer" => FmtTrait::Pointer,
                    "binary" => FmtTrait::Binary,
                    "lower_hex" => FmtTrait::LowerHex,
                    "upper_hex" => FmtTrait::UpperHex,
                    _ => FmtTrait::Usize,
                };
                let ty = if trait_ == FmtTrait::Usize { self.tcx.types.usize } else { last_ty.unwrap_or(self.tcx.types.error) };
                Value::FmtArg(Rc::new(FmtArg { trait_, value: vals[0].clone(), ty }))
            }
            "format_placeholder_new" => {
                let count = |v: &Value<'tcx>| -> InterpResult<Count> {
                    match v {
                        Value::Adt { variant: 0, fields } => Ok(Count::Is(self.as_usize(&fields[0])?)),
                        Value::Adt { variant: 1, fields } => Ok(Count::Param(self.as_usize(&fields[0])?)),
                        _ => Ok(Count::Implied),
                    }
                };
                let align = match &vals[2] {
                    Value::Adt { variant, .. } => *variant as u8,
                    _ => 3,
                };
                let fill = match &vals[1] {
                    Value::Char(c) => *c,
                    _ => ' ',
                };
                Value::Placeholder(Placeholder {
                    position: self.as_usize(&vals[0])?,
                    fill,
                    align,
                    flags: self.as_usize(&vals[3])? as u32,
                    precision: count(&vals[4])?,
                    width: count(&vals[5])?,
                })
            }
            "vec_len" => {
                let v = self.read(&self.deref_value(&vals[0])?)?;
                match v {
                    Value::Vec(xs) => self.usize(xs.len()),
                    _ => return self.ub("`len` of a value that isn't a vector"),
                }
            }
            "vec_push" => {
                let loc = self.deref_value(&vals[0])?;
                let mut v = self.read(&loc)?;
                let Value::Vec(xs) = &mut v else { return self.ub("`push` onto a value that isn't a vector") };
                xs.push(vals[1].clone());
                self.write(&loc, v)?;
                Value::unit()
            }
            "vec_pop" => {
                let loc = self.deref_value(&vals[0])?;
                let mut v = self.read(&loc)?;
                let Value::Vec(xs) = &mut v else { return self.ub("`pop` from a value that isn't a vector") };
                let popped = xs.pop();
                self.write(&loc, v)?;
                self.option(popped)
            }
            "slice_len" => match &vals[0] {
                Value::SlicePtr { len, .. } => self.usize(*len),
                _ => return self.ub("`sliceLen` of a value that isn't a slice"),
            },
            "range_next" => {
                // `Range { start, end }`: yields `start` and moves it on.
                let loc = self.deref_value(&vals[0])?;
                let Value::Adt { fields, .. } = self.read(&loc)? else { return self.ub("`next` of a value that isn't a range") };
                let (Value::Int(start), Value::Int(end)) = (&fields[0], &fields[1]) else {
                    return self.unsupported("iterating a range of a type other than an integer");
                };
                if start.to_i128() < end.to_i128() {
                    let next = Int::from_bits(start.bits.wrapping_add(1), start.size, start.signed);
                    let mut start_loc = loc.clone();
                    start_loc.path.push(0);
                    self.write(&start_loc, Value::Int(next))?;
                    self.option(Some(Value::Int(*start)))
                } else {
                    self.option(None)
                }
            }
            other => {
                let _ = ty_arg;
                return self.unsupported(format!("the intrinsic `{other}`"));
            }
        };
        Ok(Some(v))
    }

    fn lang_item(&mut self, item: LangItem, args: GenericArgsRef<'tcx>, vals: &[Value<'tcx>]) -> InterpResult<Option<Value<'tcx>>> {
        Ok(Some(match item {
            LangItem::Print | LangItem::Eprint => {
                let text = self.format_value_args(&vals[0])?;
                if item == LangItem::Print {
                    self.out.stdout(&text);
                } else {
                    self.out.stderr(&text);
                }
                Value::unit()
            }
            LangItem::Format => Value::Str(self.format_value_args(&vals[0])?),
            LangItem::PanicFmt => {
                let text = self.format_value_args(&vals[0])?;
                return self.panic(text);
            }
            LangItem::AssertFailed => {
                let tys: Vec<Ty<'tcx>> = args.iter().filter_map(|a| if let GenericArg::Type(t) = a { Some(*t) } else { None }).collect();
                let op = match &vals[0] {
                    Value::Adt { variant: 1, .. } => "!=",
                    _ => "==",
                };
                let left = self.debug_string(&vals[1], self.tcx.mk_ref(self.tcx.regions.re_erased, tys[0], ty::Mutability::Not))?;
                let right = self.debug_string(&vals[2], self.tcx.mk_ref(self.tcx.regions.re_erased, tys[1], ty::Mutability::Not))?;
                let msg = match &vals[3] {
                    Value::Adt { fields, .. } if !fields.is_empty() => format!(": {}", self.format_value_args(&fields[0])?),
                    _ => String::new(),
                };
                return self.panic(format!("assertion `left {op} right` failed{msg}\n  left: {left}\n right: {right}"));
            }
            LangItem::VecNew => Value::Vec(Vec::new()),
            LangItem::VecFromArray => match &vals[0] {
                Value::Array(xs) => Value::Vec(xs.clone()),
                _ => return self.ub("`vec[…]` of a value that isn't an array"),
            },
            LangItem::VecFromElem => {
                let n = self.as_usize(&vals[1])?;
                Value::Vec(vec![vals[0].clone(); n])
            }
            _ => return Ok(None),
        }))
    }

    /// `Some(v)` or `None`, for the `Option` lang items.
    pub fn option(&self, v: Option<Value<'tcx>>) -> Value<'tcx> {
        let tcx = self.tcx;
        let li = tcx.lang_items();
        let variant_of = |item| {
            li.get(item).and_then(|d| {
                let adt = tcx.adt_def(tcx.parent(d));
                adt.variants().iter().position(|v| v.def_id == d)
            })
        };
        match v {
            Some(x) => Value::Adt { variant: variant_of(LangItem::OptionSome).unwrap_or(1), fields: vec![x] },
            None => Value::Adt { variant: variant_of(LangItem::OptionNone).unwrap_or(0), fields: Vec::new() },
        }
    }

    /// The elements a slice pointer covers.
    pub fn read_slice(&self, v: &Value<'tcx>) -> InterpResult<Vec<Value<'tcx>>> {
        match v {
            Value::SlicePtr { array, start, len } => match self.read(&Location { alloc: array.alloc, path: array.path.clone(), slice: None })? {
                Value::Array(xs) => Ok(xs[*start..*start + *len].to_vec()),
                Value::Vec(xs) => Ok(xs[*start..*start + *len].to_vec()),
                _ => self.ub("a slice pointer that doesn't point into an array"),
            },
            Value::Ptr(p) => match self.read(&Location { alloc: p.alloc, path: p.path.clone(), slice: None })? {
                Value::Array(xs) => Ok(xs),
                _ => self.ub("expected a slice"),
            },
            _ => self.ub("expected a slice"),
        }
    }

    fn read_str_slice(&self, v: &Value<'tcx>) -> InterpResult<Vec<String>> {
        self.read_slice(v)?.iter().map(|s| self.read_str(s)).collect()
    }
}
