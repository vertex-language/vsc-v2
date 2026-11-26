//! Formatting, as Rust's `core::fmt` does it: primitive values are
//! formatted by Rust's own formatter (so `2.0` prints `2` with `{}` and
//! `2.0` with `{:?}`, as rustc's output does), then padded with the
//! placeholder's width, fill, alignment, sign and zero flags. `Debug` for
//! structs, enums, tuples and sequences prints what `@derive(Debug)` would.
//!
//! `Display` for user types needs the `Display` trait from the library;
//! until then it's reported as unsupported.

use vsc_hir::LangItem;
use vsc_middle::ty::{Ty, TyKind};

use super::*;

/// A placeholder's options, resolved.
#[derive(Clone, Copy, Default)]
struct Spec {
    fill: char,
    /// `None`: the type's default (right for numbers, left otherwise).
    align: Option<u8>,
    plus: bool,
    alternate: bool,
    zero: bool,
    debug_lower_hex: bool,
    debug_upper_hex: bool,
    width: Option<usize>,
    precision: Option<usize>,
}

impl<'a, 'tcx> Interpreter<'a, 'tcx> {
    /// The text of a `formatArguments` value.
    pub fn format_value_args(&mut self, v: &Value<'tcx>) -> InterpResult<String> {
        let Value::FmtArgs(fa) = v else { return self.ub("expected format arguments") };
        let fa = fa.clone();
        let mut out = String::new();
        match &fa.placeholders {
            None => {
                for (i, arg) in fa.args.iter().enumerate() {
                    out.push_str(fa.pieces.get(i).map_or("", String::as_str));
                    let s = self.format_arg(arg, Spec { fill: ' ', ..Spec::default() })?;
                    out.push_str(&s);
                }
                for p in fa.pieces.iter().skip(fa.args.len()) {
                    out.push_str(p);
                }
            }
            Some(ps) => {
                for (i, p) in ps.iter().enumerate() {
                    out.push_str(fa.pieces.get(i).map_or("", String::as_str));
                    let count = |this: &Self, c: Count| -> InterpResult<Option<usize>> {
                        Ok(match c {
                            Count::Is(n) => Some(n),
                            Count::Param(i) => {
                                let arg = &fa.args[i];
                                let v = this.read(&this.deref_value(&arg.value)?)?;
                                Some(this.as_usize(&v)?)
                            }
                            Count::Implied => None,
                        })
                    };
                    let spec = Spec {
                        fill: p.fill,
                        align: (p.align < 3).then_some(p.align),
                        plus: p.flags & 1 != 0,
                        alternate: p.flags & 4 != 0,
                        zero: p.flags & 8 != 0,
                        debug_lower_hex: p.flags & 16 != 0,
                        debug_upper_hex: p.flags & 32 != 0,
                        width: count(self, p.width)?,
                        precision: count(self, p.precision)?,
                    };
                    let arg = fa.args[p.position].clone();
                    let s = self.format_arg(&arg, spec)?;
                    out.push_str(&s);
                }
                for piece in fa.pieces.iter().skip(ps.len()) {
                    out.push_str(piece);
                }
            }
        }
        Ok(out)
    }

    /// `{:?}` of the value a reference points to.
    pub fn debug_string(&mut self, ptr: &Value<'tcx>, ref_ty: Ty<'tcx>) -> InterpResult<String> {
        let mut s = String::new();
        self.debug(&mut s, ptr, ref_ty, false, 0)?;
        Ok(s)
    }

    fn format_arg(&mut self, arg: &FmtArg<'tcx>, spec: Spec) -> InterpResult<String> {
        let ref_ty = self.tcx.mk_ref(self.tcx.regions.re_erased, arg.ty, vsc_middle::ty::Mutability::Not);
        match arg.trait_ {
            FmtTrait::Display => self.display(&arg.value, ref_ty, spec),
            FmtTrait::Debug => {
                let (v, ty) = self.peel_refs(arg.value.clone(), ref_ty)?;
                if let Some(s) = self.primitive_debug(&v, ty, spec)? {
                    return Ok(s);
                }
                let mut s = String::new();
                self.debug_value(&mut s, &v, ty, spec.alternate, 0)?;
                Ok(pad(&s, spec, false))
            }
            FmtTrait::Pointer => {
                let s = match &arg.value {
                    Value::Ptr(p) => format!("{:#x}", 0x1000 + p.alloc.0 * 0x10),
                    _ => "0x0".to_string(),
                };
                Ok(pad(&s, spec, true))
            }
            FmtTrait::Usize => Ok(String::new()),
            t => {
                let (v, ty) = self.peel_refs(arg.value.clone(), ref_ty)?;
                self.radix_or_exp(&v, ty, t, spec)
            }
        }
    }

    /// Follows references to the value they point to.
    fn peel_refs(&self, mut v: Value<'tcx>, mut ty: Ty<'tcx>) -> InterpResult<(Value<'tcx>, Ty<'tcx>)> {
        loop {
            match *ty.kind() {
                TyKind::Ref(_, inner, _) => match inner.kind() {
                    // `&string` and `&[T]` stay as slice pointers.
                    TyKind::Str | TyKind::Slice(_) => return Ok((v, ty)),
                    _ => {
                        v = self.read(&self.deref_value(&v)?)?;
                        ty = inner;
                    }
                },
                _ => return Ok((v, ty)),
            }
        }
    }

    fn display(&mut self, ptr: &Value<'tcx>, ref_ty: Ty<'tcx>, spec: Spec) -> InterpResult<String> {
        let (v, ty) = self.peel_refs(ptr.clone(), ref_ty)?;
        let tcx = self.tcx;
        match (&v, ty.kind()) {
            (Value::Int(i), _) => Ok(pad_number(&int_decimal(*i), spec)),
            (Value::F64(f), _) => Ok(pad_number(&match spec.precision {
                Some(p) => format!("{:.*}", p, f),
                None => format!("{f}"),
            }, spec)),
            (Value::F32(f), _) => Ok(pad_number(&match spec.precision {
                Some(p) => format!("{:.*}", p, f),
                None => format!("{f}"),
            }, spec)),
            (Value::Bool(b), _) => Ok(pad(&b.to_string(), spec, false)),
            (Value::Char(c), _) => Ok(pad(&c.to_string(), spec, false)),
            (Value::Str(s), _) => Ok(pad(&truncate(s, spec.precision), spec, false)),
            (Value::SlicePtr { .. }, TyKind::Ref(_, inner, _)) if matches!(inner.kind(), TyKind::Str) => {
                let s = self.read_str(&v)?;
                Ok(pad(&truncate(&s, spec.precision), spec, false))
            }
            (Value::FmtArgs(_), _) => {
                let s = self.format_value_args(&v)?;
                Ok(pad(&s, spec, false))
            }
            _ => {
                let _ = tcx;
                self.unsupported(format!("formatting `{}` with `{{}}`: it needs the `Display` trait from the library", self.tcx.ty_to_string(ty)))
            }
        }
    }

    /// `{:?}` of a primitive, padded; `None` for compound values.
    fn primitive_debug(&mut self, v: &Value<'tcx>, ty: Ty<'tcx>, spec: Spec) -> InterpResult<Option<String>> {
        Ok(Some(match v {
            Value::Int(i) => {
                if spec.debug_lower_hex || spec.debug_upper_hex {
                    return Ok(Some(self.radix_or_exp(v, ty, if spec.debug_lower_hex { FmtTrait::LowerHex } else { FmtTrait::UpperHex }, spec)?));
                }
                pad_number(&int_decimal(*i), spec)
            }
            Value::F64(f) => pad_number(&match spec.precision {
                Some(p) => format!("{:.*?}", p, f),
                None => format!("{f:?}"),
            }, spec),
            Value::F32(f) => pad_number(&match spec.precision {
                Some(p) => format!("{:.*?}", p, f),
                None => format!("{f:?}"),
            }, spec),
            Value::Bool(b) => pad(&b.to_string(), spec, false),
            Value::Char(c) => pad(&format!("{c:?}"), spec, false),
            Value::Str(s) => pad(&format!("{s:?}"), spec, false),
            Value::SlicePtr { .. } if matches!(ty.kind(), TyKind::Ref(_, i, _) if matches!(i.kind(), TyKind::Str)) => {
                let s = self.read_str(v)?;
                pad(&format!("{s:?}"), spec, false)
            }
            _ => return Ok(None),
        }))
    }

    fn radix_or_exp(&mut self, v: &Value<'tcx>, ty: Ty<'tcx>, t: FmtTrait, spec: Spec) -> InterpResult<String> {
        match v {
            Value::Int(i) => {
                let (body, prefix) = match t {
                    FmtTrait::LowerHex => (format!("{:x}", i.bits), "0x"),
                    FmtTrait::UpperHex => (format!("{:X}", i.bits), "0x"),
                    FmtTrait::Octal => (format!("{:o}", i.bits), "0o"),
                    FmtTrait::Binary => (format!("{:b}", i.bits), "0b"),
                    FmtTrait::LowerExp => (format!("{:e}", i.to_i128()), ""),
                    _ => (format!("{:E}", i.to_i128()), ""),
                };
                let full = if spec.alternate { format!("{prefix}{body}") } else { body };
                Ok(pad_number(&full, spec))
            }
            Value::F64(f) => Ok(pad_number(&exp_float(*f, t, spec.precision), spec)),
            Value::F32(f) => Ok(pad_number(&exp_float(*f as f64, t, spec.precision), spec)),
            _ => self.unsupported(format!("formatting `{}` that way", self.tcx.ty_to_string(ty))),
        }
    }

    // -----------------------------------------------------------------------
    // Debug for compound values
    // -----------------------------------------------------------------------

    fn debug(&mut self, out: &mut String, ptr: &Value<'tcx>, ref_ty: Ty<'tcx>, pretty: bool, indent: usize) -> InterpResult<()> {
        let (v, ty) = self.peel_refs(ptr.clone(), ref_ty)?;
        self.debug_value(out, &v, ty, pretty, indent)
    }

    fn debug_value(&mut self, out: &mut String, v: &Value<'tcx>, ty: Ty<'tcx>, pretty: bool, indent: usize) -> InterpResult<()> {
        let tcx = self.tcx;
        if let Some(s) = self.primitive_debug(v, ty, Spec { fill: ' ', ..Spec::default() })? {
            out.push_str(&s);
            return Ok(());
        }
        match (v, *ty.kind()) {
            (Value::Tuple(xs), TyKind::Tuple(tys)) => {
                let items: Vec<(Option<String>, Value<'tcx>, Ty<'tcx>)> = xs.iter().cloned().zip(tys.iter().copied()).map(|(v, t)| (None, v, t)).collect();
                self.debug_list(out, "", "(", ")", &items, pretty, indent, xs.len() == 1)
            }
            (Value::Array(xs), TyKind::Array(e, _)) => {
                let items: Vec<_> = xs.iter().cloned().map(|v| (None, v, e)).collect();
                self.debug_list(out, "", "[", "]", &items, pretty, indent, false)
            }
            (Value::Vec(xs), TyKind::Adt(_, args)) => {
                let e = args.first().map_or(tcx.types.error, |a| a.expect_ty());
                let items: Vec<_> = xs.iter().cloned().map(|v| (None, v, e)).collect();
                self.debug_list(out, "", "[", "]", &items, pretty, indent, false)
            }
            (Value::SlicePtr { .. }, TyKind::Ref(_, inner, _)) => {
                let TyKind::Slice(e) = *inner.kind() else { return self.unsupported("this slice") };
                let xs = self.read_slice(v)?;
                let items: Vec<_> = xs.into_iter().map(|v| (None, v, e)).collect();
                self.debug_list(out, "", "[", "]", &items, pretty, indent, false)
            }
            (Value::Ptr(_), TyKind::Ref(_, inner, _)) | (Value::Ptr(_), TyKind::RawPtr(inner, _)) => {
                let inner_v = self.read(&self.deref_value(v)?)?;
                self.debug_value(out, &inner_v, inner, pretty, indent)
            }
            (Value::Class(a), TyKind::Adt(_, args)) => {
                let inner_v = self.read(&Location::of(*a))?;
                let inner_ty = args.first().map_or(tcx.types.error, |a| a.expect_ty());
                self.debug_value(out, &inner_v, inner_ty, pretty, indent)
            }
            (Value::Adt { variant, fields }, TyKind::Adt(adt, args)) => {
                if tcx.is_lang_item(adt.did(), LangItem::String) {
                    return self.unsupported("this string value");
                }
                let var = &adt.variants()[*variant];
                // Derived `Debug` names enum variants alone, structs by name.
                let name = if adt.is_enum() { var.name.to_string() } else { tcx.item_name(adt.did()).to_string() };
                let field_tys: Vec<Ty<'tcx>> = var.fields.iter().map(|f| tcx.erase_regions(tcx.instantiate(tcx.type_of(f.did), args))).collect();
                match var.ctor {
                    Some((vsc_hir::def::CtorKind::Const, _)) => {
                        out.push_str(&name);
                        Ok(())
                    }
                    Some((vsc_hir::def::CtorKind::Fn, _)) => {
                        let items: Vec<_> = fields.iter().cloned().zip(field_tys).map(|(v, t)| (None, v, t)).collect();
                        self.debug_list(out, &name, "(", ")", &items, pretty, indent, false)
                    }
                    None => {
                        let items: Vec<_> = fields.iter().cloned().zip(field_tys).zip(var.fields.iter()).map(|((v, t), f)| (Some(f.name.to_string()), v, t)).collect();
                        self.debug_list(out, &name, " {", "}", &items, pretty, indent, false)
                    }
                }
            }
            _ => self.unsupported(format!("formatting `{}` with `{{:?}}`", tcx.ty_to_string(ty))),
        }
    }

    /// `name(a, b)`, `name { f: a }`, `[a, b]`, compact or pretty.
    #[allow(clippy::too_many_arguments)]
    fn debug_list(
        &mut self,
        out: &mut String,
        name: &str,
        open: &str,
        close: &str,
        items: &[(Option<String>, Value<'tcx>, Ty<'tcx>)],
        pretty: bool,
        indent: usize,
        one_tuple: bool,
    ) -> InterpResult<()> {
        out.push_str(name);
        if items.is_empty() {
            out.push_str(if open == " {" { "" } else { open });
            if open != " {" {
                out.push_str(close);
            }
            return Ok(());
        }
        out.push_str(open);
        let named = items[0].0.is_some();
        for (i, (field, v, t)) in items.iter().enumerate() {
            if pretty {
                out.push('\n');
                out.push_str(&"    ".repeat(indent + 1));
            } else if i > 0 {
                out.push_str(", ");
            } else if named {
                out.push(' ');
            }
            if let Some(f) = field {
                out.push_str(f);
                out.push_str(": ");
            }
            self.debug_value(out, v, *t, pretty, indent + 1)?;
            if pretty {
                out.push(',');
            }
        }
        if pretty {
            out.push('\n');
            out.push_str(&"    ".repeat(indent));
        } else {
            if one_tuple {
                out.push(',');
            }
            if named {
                out.push(' ');
            }
        }
        out.push_str(close);
        Ok(())
    }
}

fn int_decimal(i: Int) -> String {
    if i.signed { i.to_i128().to_string() } else { i.bits.to_string() }
}

fn truncate(s: &str, precision: Option<usize>) -> String {
    match precision {
        Some(p) => s.chars().take(p).collect(),
        None => s.to_string(),
    }
}

fn exp_float(f: f64, t: FmtTrait, precision: Option<usize>) -> String {
    match (t, precision) {
        (FmtTrait::UpperExp, Some(p)) => format!("{:.*E}", p, f),
        (FmtTrait::UpperExp, None) => format!("{f:E}"),
        (_, Some(p)) => format!("{:.*e}", p, f),
        (_, None) => format!("{f:e}"),
    }
}

/// Pads a number: a `+` sign if asked, zeros after the sign and prefix with
/// the `0` flag, else the fill, right-aligned by default.
fn pad_number(s: &str, spec: Spec) -> String {
    let negative = s.starts_with('-');
    let mut body = s.to_string();
    if spec.plus && !negative {
        body.insert(0, '+');
    }
    if spec.zero {
        if let Some(w) = spec.width {
            let len = body.chars().count();
            if len < w {
                let sign_len = if body.starts_with(['+', '-']) { 1 } else { 0 };
                let prefix_len = if body[sign_len..].starts_with("0x") || body[sign_len..].starts_with("0b") || body[sign_len..].starts_with("0o") { 2 } else { 0 };
                let at = sign_len + prefix_len;
                body.insert_str(at, &"0".repeat(w - len));
            }
        }
        return body;
    }
    pad(&body, spec, true)
}

/// Pads to the width with the fill and alignment.
fn pad(s: &str, spec: Spec, numeric: bool) -> String {
    let Some(w) = spec.width else { return s.to_string() };
    let len = s.chars().count();
    if len >= w {
        return s.to_string();
    }
    let fill = if spec.fill == '\0' { ' ' } else { spec.fill };
    let n = w - len;
    let align = spec.align.unwrap_or(if numeric { 1 } else { 0 });
    let (left, right) = match align {
        0 => (0, n),
        1 => (n, 0),
        _ => (n / 2, n - n / 2),
    };
    let f = |k: usize| fill.to_string().repeat(k);
    format!("{}{s}{}", f(left), f(right))
}

