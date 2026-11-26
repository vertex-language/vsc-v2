//! Exhaustiveness and reachability of patterns (rustc's
//! `rustc_pattern_analysis`), by the usefulness algorithm of Maranget's
//! "Warnings for pattern matching".
//!
//! A pattern is deconstructed into a constructor and its fields. A row of
//! patterns is *useful* against a matrix of earlier rows if some value
//! matches it and no earlier row. An arm is unreachable if its pattern isn't
//! useful; a match is exhaustive if the wildcard isn't useful against all of
//! its unguarded arms. When it is, the algorithm builds example values that
//! no arm matches (witnesses), which the error shows.
//!
//! Integers and `char`s are ranges of their (biased, so signed order is
//! unsigned order) values, split at every boundary the patterns use, so
//! `0..=9` and `10..` cover a `uint8`. Slices are split by length.

use std::fmt::Write;

use vsc_hir::LangItem;
use vsc_middle::thir::{self, PatConst, PatKind, RangeEnd};
use vsc_middle::ty::{IntTy, Ty, TyCtxt, TyKind, UintTy};

/// An inclusive range of biased integer values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IntRange {
    pub lo: u128,
    pub hi: u128,
}

impl IntRange {
    fn contains(self, other: IntRange) -> bool {
        self.lo <= other.lo && other.hi <= self.hi
    }
    fn intersects(self, other: IntRange) -> bool {
        self.lo <= other.hi && other.lo <= self.hi
    }
}

/// The lengths a slice pattern matches.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SliceKind {
    Fixed(usize),
    /// `prefix` elements, a rest, `suffix` elements: any length at least
    /// their sum.
    Var(usize, usize),
}

impl SliceKind {
    fn arity(self) -> usize {
        match self {
            SliceKind::Fixed(n) => n,
            SliceKind::Var(p, s) => p + s,
        }
    }
    /// Does a pattern of this kind match every slice of kind `other`?
    fn covers(self, other: SliceKind) -> bool {
        match (self, other) {
            (SliceKind::Fixed(a), SliceKind::Fixed(b)) => a == b,
            (SliceKind::Fixed(_), SliceKind::Var(..)) => false,
            (SliceKind::Var(..), _) => self.arity() <= other.arity(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Constructor {
    /// An enum variant, by index.
    Variant(usize),
    /// The only constructor of a struct, tuple or reference.
    Single,
    Bool(bool),
    IntRange(IntRange),
    Slice(SliceKind),
    /// A string, float or named constant: matches only itself, and no set
    /// of them is exhaustive.
    Opaque(PatConst),
    /// The values no listed constructor covers (floats, strings, …).
    NonExhaustive,
    Wildcard,
    Or,
}

#[derive(Clone, Debug)]
pub struct DPat<'tcx> {
    pub ctor: Constructor,
    pub fields: Vec<DPat<'tcx>>,
    pub ty: Ty<'tcx>,
}

pub struct MatchCx<'tcx> {
    pub tcx: TyCtxt<'tcx>,
}

/// Limits on the work and on the witnesses reported.
const MAX_WITNESSES: usize = 4;

impl<'tcx> MatchCx<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>) -> Self {
        MatchCx { tcx }
    }

    fn wild(&self, ty: Ty<'tcx>) -> DPat<'tcx> {
        DPat { ctor: Constructor::Wildcard, fields: Vec::new(), ty }
    }

    // -----------------------------------------------------------------------
    // Integers
    // -----------------------------------------------------------------------

    /// Width and signedness of an integer-like type (`char` is `uint32`).
    fn int_info(&self, ty: Ty<'tcx>) -> Option<(u32, bool)> {
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
            TyKind::Char => (32, false),
            _ => return None,
        })
    }

    fn mask(bits: u32) -> u128 {
        if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 }
    }

    /// A value's bits (sign-extended for signed types) to its biased
    /// position in the type's range.
    fn bias(&self, ty: Ty<'tcx>, v: u128) -> u128 {
        let Some((bits, signed)) = self.int_info(ty) else { return v };
        if signed { v.wrapping_add(1u128 << (bits - 1)) & Self::mask(bits) } else { v & Self::mask(bits) }
    }

    fn unbias(&self, ty: Ty<'tcx>, b: u128) -> i128 {
        let Some((bits, signed)) = self.int_info(ty) else { return b as i128 };
        if signed {
            let v = b.wrapping_sub(1u128 << (bits - 1)) & Self::mask(bits);
            // Sign-extend from `bits`.
            let shift = 128 - bits;
            ((v << shift) as i128) >> shift
        } else {
            b as i128
        }
    }

    fn full_range(&self, ty: Ty<'tcx>) -> Option<IntRange> {
        let (bits, _) = self.int_info(ty)?;
        Some(IntRange { lo: 0, hi: Self::mask(bits) })
    }

    // -----------------------------------------------------------------------
    // Deconstruction
    // -----------------------------------------------------------------------

    /// The types of a constructor's fields.
    fn field_tys(&self, ty: Ty<'tcx>, ctor: Constructor) -> Vec<Ty<'tcx>> {
        let tcx = self.tcx;
        match (ctor, *ty.kind()) {
            (Constructor::Single, TyKind::Tuple(tys)) => tys.to_vec(),
            (Constructor::Single, TyKind::Ref(_, inner, _)) => vec![inner],
            (Constructor::Single, TyKind::Adt(adt, args)) => {
                let adt = self.class_inner(adt, args).unwrap_or((adt, args));
                adt.0.variants()[0].fields.iter().map(|f| tcx.erase_regions(tcx.instantiate(tcx.type_of(f.did), adt.1))).collect()
            }
            (Constructor::Variant(i), TyKind::Adt(adt, args)) => {
                adt.variants()[i].fields.iter().map(|f| tcx.erase_regions(tcx.instantiate(tcx.type_of(f.did), args))).collect()
            }
            (Constructor::Slice(k), TyKind::Array(e, _) | TyKind::Slice(e)) => vec![e; k.arity()],
            _ => Vec::new(),
        }
    }

    /// A class value's struct: its fields are the class's.
    fn class_inner(&self, adt: vsc_middle::ty::AdtDef<'tcx>, args: vsc_middle::ty::GenericArgsRef<'tcx>) -> Option<(vsc_middle::ty::AdtDef<'tcx>, vsc_middle::ty::GenericArgsRef<'tcx>)> {
        if !self.tcx.is_lang_item(adt.did(), LangItem::Class) {
            return None;
        }
        match *args.first()?.expect_ty().kind() {
            TyKind::Adt(a, x) => Some((a, x)),
            _ => None,
        }
    }

    pub fn deconstruct(&self, p: &thir::Pat<'tcx>) -> DPat<'tcx> {
        let ty = p.ty;
        let with_fields = |ctor: Constructor, subs: &[thir::FieldPat<'tcx>]| -> DPat<'tcx> {
            let tys = self.field_tys(ty, ctor);
            let mut fields: Vec<DPat<'tcx>> = tys.iter().map(|&t| self.wild(t)).collect();
            for s in subs {
                if s.field < fields.len() {
                    fields[s.field] = self.deconstruct(&s.pattern);
                }
            }
            DPat { ctor, fields, ty }
        };
        match &p.kind {
            PatKind::Wild | PatKind::Error | PatKind::Binding { subpattern: None, .. } => self.wild(ty),
            PatKind::Binding { subpattern: Some(s), .. } => self.deconstruct(s),
            PatKind::Deref { subpattern } => DPat { ctor: Constructor::Single, fields: vec![self.deconstruct(subpattern)], ty },
            PatKind::Leaf { subpatterns } => with_fields(Constructor::Single, subpatterns),
            PatKind::Variant { variant_index, subpatterns, .. } => with_fields(Constructor::Variant(*variant_index), subpatterns),
            PatKind::Constant { value } => match *value {
                PatConst::Bool(b) => DPat { ctor: Constructor::Bool(b), fields: Vec::new(), ty },
                PatConst::Int(v) if self.int_info(ty).is_some() => {
                    let b = self.bias(ty, v);
                    DPat { ctor: Constructor::IntRange(IntRange { lo: b, hi: b }), fields: Vec::new(), ty }
                }
                other => DPat { ctor: Constructor::Opaque(other), fields: Vec::new(), ty },
            },
            PatKind::Range { lo, hi, end } => {
                let Some(full) = self.full_range(ty) else { return self.wild(ty) };
                let lo = match lo {
                    Some(PatConst::Int(v)) => self.bias(ty, *v),
                    _ => full.lo,
                };
                let hi_given = hi.is_some();
                let mut hi = match hi {
                    Some(PatConst::Int(v)) => self.bias(ty, *v),
                    _ => full.hi,
                };
                // `lo..` has no end to exclude.
                if *end == RangeEnd::Excluded && hi_given {
                    if hi == 0 || hi <= lo {
                        // An empty range matches nothing.
                        return DPat { ctor: Constructor::Opaque(PatConst::Bool(false)), fields: Vec::new(), ty };
                    }
                    hi -= 1;
                }
                DPat { ctor: Constructor::IntRange(IntRange { lo, hi: hi.max(lo) }), fields: Vec::new(), ty }
            }
            PatKind::Slice { prefix, slice, suffix } | PatKind::Array { prefix, slice, suffix } => {
                let array_len = match *ty.kind() {
                    TyKind::Array(_, n) => match *n.kind() {
                        vsc_middle::ty::ConstKind::Value(_, n) => Some(n as usize),
                        _ => None,
                    },
                    _ => None,
                };
                let elem = match *ty.kind() {
                    TyKind::Array(e, _) | TyKind::Slice(e) => e,
                    _ => self.tcx.types.error,
                };
                let mut fields: Vec<DPat<'tcx>> = prefix.iter().map(|x| self.deconstruct(x)).collect();
                let suffix: Vec<DPat<'tcx>> = suffix.iter().map(|x| self.deconstruct(x)).collect();
                let kind = match (slice, array_len) {
                    // On an array, the rest has a known length.
                    (Some(_), Some(n)) => {
                        let middle = n.saturating_sub(prefix.len() + suffix.len());
                        fields.extend((0..middle).map(|_| self.wild(elem)));
                        SliceKind::Fixed(n)
                    }
                    (Some(_), None) => SliceKind::Var(prefix.len(), suffix.len()),
                    (None, _) => SliceKind::Fixed(prefix.len() + suffix.len()),
                };
                fields.extend(suffix);
                DPat { ctor: Constructor::Slice(kind), fields, ty }
            }
            PatKind::Or { pats } => DPat { ctor: Constructor::Or, fields: pats.iter().map(|x| self.deconstruct(x)).collect(), ty },
        }
    }

    // -----------------------------------------------------------------------
    // Constructors of a type
    // -----------------------------------------------------------------------

    /// Every constructor of `ty`, split so that each one is either inside
    /// or outside each constructor in `column`.
    fn split_all(&self, ty: Ty<'tcx>, column: &[Constructor]) -> Vec<Constructor> {
        match *ty.kind() {
            TyKind::Bool => vec![Constructor::Bool(false), Constructor::Bool(true)],
            TyKind::Char => {
                // The scalar values: everything but the surrogates.
                let ranges = [IntRange { lo: 0, hi: 0xD7FF }, IntRange { lo: 0xE000, hi: 0x10FFFF }];
                ranges.iter().flat_map(|&r| split_range(r, column)).map(Constructor::IntRange).collect()
            }
            TyKind::Int(_) | TyKind::Uint(_) => {
                split_range(self.full_range(ty).unwrap(), column).into_iter().map(Constructor::IntRange).collect()
            }
            TyKind::Adt(adt, args) if adt.is_enum() && self.class_inner(adt, args).is_none() => {
                (0..adt.variants().len()).map(Constructor::Variant).collect()
            }
            TyKind::Adt(..) | TyKind::Tuple(_) | TyKind::Ref(..) => vec![Constructor::Single],
            TyKind::Never => Vec::new(),
            TyKind::Array(_, n) => match *n.kind() {
                vsc_middle::ty::ConstKind::Value(_, n) => vec![Constructor::Slice(SliceKind::Fixed(n as usize))],
                _ => vec![Constructor::NonExhaustive],
            },
            TyKind::Slice(_) => {
                // Lengths below the longest any pattern spells out, then
                // "that long or longer".
                let mut max_fixed = 0;
                let mut max_var = 0;
                for c in column {
                    match c {
                        Constructor::Slice(SliceKind::Fixed(n)) => max_fixed = max_fixed.max(*n + 1),
                        Constructor::Slice(SliceKind::Var(p, s)) => max_var = max_var.max(p + s),
                        _ => {}
                    }
                }
                let l = max_fixed.max(max_var);
                let mut v: Vec<Constructor> = (0..l).map(|n| Constructor::Slice(SliceKind::Fixed(n))).collect();
                v.push(Constructor::Slice(SliceKind::Var(l, 0)));
                v
            }
            _ => vec![Constructor::NonExhaustive],
        }
    }

    /// Does pattern constructor `pat` match every value of `c` (a split
    /// constructor)?
    fn covers(&self, pat: Constructor, c: Constructor) -> bool {
        match (pat, c) {
            (Constructor::Wildcard, _) => true,
            (Constructor::Variant(a), Constructor::Variant(b)) => a == b,
            (Constructor::Single, Constructor::Single) => true,
            (Constructor::Bool(a), Constructor::Bool(b)) => a == b,
            (Constructor::IntRange(a), Constructor::IntRange(b)) => a.contains(b),
            (Constructor::Slice(a), Constructor::Slice(b)) => a.covers(b),
            (Constructor::Opaque(a), Constructor::Opaque(b)) => a == b,
            _ => false,
        }
    }

    /// The fields of `p` when specialized to `c`, or `None` if `p` doesn't
    /// match `c`.
    fn specialize(&self, p: &DPat<'tcx>, c: Constructor, tys: &[Ty<'tcx>]) -> Option<Vec<DPat<'tcx>>> {
        match p.ctor {
            Constructor::Wildcard => Some(tys.iter().map(|&t| self.wild(t)).collect()),
            // A rest pattern stretched to a longer length.
            Constructor::Slice(SliceKind::Var(pre, suf)) => {
                if !self.covers(p.ctor, c) {
                    return None;
                }
                let n = tys.len();
                let mut fields = p.fields[..pre].to_vec();
                fields.extend((0..n - pre - suf).map(|i| self.wild(tys[pre + i])));
                fields.extend(p.fields[pre..].iter().cloned());
                Some(fields)
            }
            pc if self.covers(pc, c) => Some(p.fields.clone()),
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // Usefulness
    // -----------------------------------------------------------------------

    /// Rows whose first pattern is an or-pattern become one row per
    /// alternative.
    fn expand_or(rows: Vec<Vec<DPat<'tcx>>>) -> Vec<Vec<DPat<'tcx>>> {
        let mut out = Vec::new();
        let mut stack: Vec<Vec<DPat<'tcx>>> = rows.into_iter().rev().collect();
        while let Some(row) = stack.pop() {
            match row.first() {
                Some(p) if p.ctor == Constructor::Or => {
                    for alt in p.fields.iter().rev() {
                        let mut r = vec![alt.clone()];
                        r.extend(row[1..].iter().cloned());
                        stack.push(r);
                    }
                }
                _ => out.push(row),
            }
        }
        out
    }

    /// The values matched by `v` and no row: witnesses, each one pattern
    /// per column. Empty if `v` isn't useful.
    pub fn witnesses(&self, rows: &[Vec<DPat<'tcx>>], v: &[DPat<'tcx>]) -> Vec<Vec<Witness<'tcx>>> {
        if v.is_empty() {
            return if rows.is_empty() { vec![Vec::new()] } else { Vec::new() };
        }
        let rows = Self::expand_or(rows.to_vec());
        let head = &v[0];
        if head.ctor == Constructor::Or {
            let mut out = Vec::new();
            for alt in &head.fields {
                let mut nv = vec![alt.clone()];
                nv.extend(v[1..].iter().cloned());
                out.extend(self.witnesses(&rows, &nv));
                if out.len() >= MAX_WITNESSES {
                    break;
                }
            }
            return out;
        }
        let ty = head.ty;
        let column: Vec<Constructor> = rows.iter().map(|r| r[0].ctor).filter(|c| *c != Constructor::Wildcard).collect();
        let all = self.split_all(ty, &column);
        let mut out = Vec::new();
        if head.ctor != Constructor::Wildcard {
            for &c in all.iter().filter(|&&c| self.covers(head.ctor, c)) {
                out.extend(self.witnesses_for_ctor(&rows, v, c));
                if out.len() >= MAX_WITNESSES {
                    break;
                }
            }
            // A constructor `split_all` doesn't list (an opaque constant):
            // it's useful only if no row covers it.
            if all.iter().all(|&c| !self.covers(head.ctor, c)) {
                out.extend(self.witnesses_for_ctor(&rows, v, head.ctor));
            }
            return out;
        }
        let missing: Vec<Constructor> = all.iter().copied().filter(|&c| !column.iter().any(|&pc| self.covers(pc, c))).collect();
        if missing.is_empty() {
            for &c in &all {
                out.extend(self.witnesses_for_ctor(&rows, v, c));
                if out.len() >= MAX_WITNESSES {
                    break;
                }
            }
        } else {
            // Only rows starting with a wildcard can match a missing
            // constructor.
            let default: Vec<Vec<DPat<'tcx>>> =
                rows.iter().filter(|r| r[0].ctor == Constructor::Wildcard).map(|r| r[1..].to_vec()).collect();
            let rest = self.witnesses(&default, &v[1..]);
            let report_wild = column.is_empty() || missing.contains(&Constructor::NonExhaustive);
            for w in rest {
                if report_wild {
                    let mut x = vec![Witness::wild(ty)];
                    x.extend(w);
                    out.push(x);
                } else {
                    for &c in &missing {
                        let fields = self.field_tys(ty, c).into_iter().map(Witness::wild).collect();
                        let mut x = vec![Witness { ctor: c, fields, ty }];
                        x.extend(w.iter().cloned());
                        out.push(x);
                    }
                }
                if out.len() >= MAX_WITNESSES {
                    break;
                }
            }
        }
        out
    }

    fn witnesses_for_ctor(&self, rows: &[Vec<DPat<'tcx>>], v: &[DPat<'tcx>], c: Constructor) -> Vec<Vec<Witness<'tcx>>> {
        let ty = v[0].ty;
        let tys = self.field_tys(ty, c);
        let srows: Vec<Vec<DPat<'tcx>>> = rows
            .iter()
            .filter_map(|r| {
                let mut f = self.specialize(&r[0], c, &tys)?;
                f.extend(r[1..].iter().cloned());
                Some(f)
            })
            .collect();
        let Some(mut sv) = self.specialize(&v[0], c, &tys) else { return Vec::new() };
        sv.extend(v[1..].iter().cloned());
        let arity = tys.len();
        self.witnesses(&srows, &sv)
            .into_iter()
            .map(|w| {
                let mut out = vec![Witness { ctor: c, fields: w[..arity].to_vec(), ty }];
                out.extend(w[arity..].iter().cloned());
                out
            })
            .collect()
    }

    /// Analyzes a `match`: which arms are unreachable, and which values no
    /// arm matches. Guarded arms don't cover anything.
    pub fn analyze(&self, arms: &[(DPat<'tcx>, bool)], scrut_ty: Ty<'tcx>) -> MatchReport<'tcx> {
        let mut rows: Vec<Vec<DPat<'tcx>>> = Vec::new();
        let mut unreachable = Vec::new();
        for (i, (pat, has_guard)) in arms.iter().enumerate() {
            if self.witnesses(&rows, std::slice::from_ref(pat)).is_empty() {
                unreachable.push(i);
            }
            if !has_guard {
                rows.push(vec![pat.clone()]);
            }
        }
        let missing = self.witnesses(&rows, &[self.wild(scrut_ty)]).into_iter().map(|mut w| w.remove(0)).collect();
        MatchReport { unreachable, missing }
    }

    // -----------------------------------------------------------------------
    // Printing witnesses
    // -----------------------------------------------------------------------

    pub fn witness_to_string(&self, w: &Witness<'tcx>) -> String {
        let tcx = self.tcx;
        let fields = |s: &mut String, fs: &[Witness<'tcx>]| {
            let parts: Vec<String> = fs.iter().map(|f| self.witness_to_string(f)).collect();
            s.push_str(&parts.join(", "));
        };
        let mut s = String::new();
        match (w.ctor, *w.ty.kind()) {
            (Constructor::Wildcard | Constructor::NonExhaustive | Constructor::Or, _) => s.push('_'),
            (Constructor::Bool(b), _) => s.push_str(if b { "true" } else { "false" }),
            (Constructor::IntRange(r), _) => s.push_str(&self.range_to_string(w.ty, r)),
            (Constructor::Opaque(_), _) => s.push('_'),
            (Constructor::Variant(i), TyKind::Adt(adt, _)) => {
                let v = &adt.variants()[i];
                s.push_str(&tcx.def_path_str(v.def_id));
                self.write_variant_fields(&mut s, v, &w.fields);
            }
            (Constructor::Single, TyKind::Tuple(_)) => {
                s.push('(');
                fields(&mut s, &w.fields);
                if w.fields.len() == 1 {
                    s.push(',');
                }
                s.push(')');
            }
            (Constructor::Single, TyKind::Ref(..)) => {
                s.push('&');
                s.push_str(&self.witness_to_string(&w.fields[0]));
            }
            (Constructor::Single, TyKind::Adt(adt, _)) => {
                let v = &adt.variants()[0];
                s.push_str(&tcx.def_path_str(adt.did()));
                self.write_variant_fields(&mut s, v, &w.fields);
            }
            (Constructor::Slice(k), _) => {
                s.push('[');
                let parts: Vec<String> = w.fields.iter().map(|f| self.witness_to_string(f)).collect();
                match k {
                    SliceKind::Fixed(_) => s.push_str(&parts.join(", ")),
                    SliceKind::Var(pre, _) => {
                        let mut all: Vec<String> = parts[..pre].to_vec();
                        all.push("..".to_string());
                        all.extend(parts[pre..].iter().cloned());
                        s.push_str(&all.join(", "));
                    }
                }
                s.push(']');
            }
            _ => s.push('_'),
        }
        let _ = write!(&mut String::new(), "");
        s
    }

    fn write_variant_fields(&self, s: &mut String, v: &vsc_middle::ty::VariantDef, fields: &[Witness<'tcx>]) {
        match v.ctor {
            Some((vsc_hir::def::CtorKind::Const, _)) => {}
            Some((vsc_hir::def::CtorKind::Fn, _)) => {
                s.push('(');
                let parts: Vec<String> = fields.iter().map(|f| self.witness_to_string(f)).collect();
                s.push_str(&parts.join(", "));
                s.push(')');
            }
            None => {
                let shown: Vec<String> = v
                    .fields
                    .iter()
                    .zip(fields)
                    .filter(|(_, w)| w.ctor != Constructor::Wildcard)
                    .map(|(f, w)| format!("{}: {}", f.name, self.witness_to_string(w)))
                    .collect();
                if shown.is_empty() {
                    s.push_str(" { .. }");
                } else {
                    s.push_str(&format!(" {{ {}, .. }}", shown.join(", ")));
                }
            }
        }
    }

    fn range_to_string(&self, ty: Ty<'tcx>, r: IntRange) -> String {
        let full = self.full_range(ty);
        let name = self.tcx.ty_to_string(ty);
        let value = |b: u128| -> String {
            if let TyKind::Char = ty.kind() {
                return match char::from_u32(b as u32) {
                    Some(c) => format!("{:?}", c),
                    None => format!("'\\u{{{b:x}}}'"),
                };
            }
            match full {
                Some(f) if b == f.lo && self.unbias(ty, b) != 0 => format!("{name}.MIN"),
                Some(f) if b == f.hi => format!("{name}.MAX"),
                _ => self.unbias(ty, b).to_string(),
            }
        };
        if r.lo == r.hi { value(r.lo) } else { format!("{}..={}", value(r.lo), value(r.hi)) }
    }
}

/// An example value no arm matches.
#[derive(Clone, Debug)]
pub struct Witness<'tcx> {
    pub ctor: Constructor,
    pub fields: Vec<Witness<'tcx>>,
    pub ty: Ty<'tcx>,
}

impl<'tcx> Witness<'tcx> {
    fn wild(ty: Ty<'tcx>) -> Self {
        Witness { ctor: Constructor::Wildcard, fields: Vec::new(), ty }
    }
}

pub struct MatchReport<'tcx> {
    /// Arms (by index) no value reaches.
    pub unreachable: Vec<usize>,
    /// Values no arm matches.
    pub missing: Vec<Witness<'tcx>>,
}

/// Splits `range` at every boundary of the ranges in `column`, so each
/// piece is inside or outside each of them.
fn split_range(range: IntRange, column: &[Constructor]) -> Vec<IntRange> {
    let mut cuts: Vec<u128> = vec![range.lo];
    for c in column {
        if let Constructor::IntRange(r) = c {
            if r.intersects(range) {
                if r.lo > range.lo {
                    cuts.push(r.lo);
                }
                if r.hi < range.hi {
                    cuts.push(r.hi + 1);
                }
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = Vec::with_capacity(cuts.len());
    for (i, &lo) in cuts.iter().enumerate() {
        let hi = cuts.get(i + 1).map_or(range.hi, |&n| n - 1);
        out.push(IntRange { lo, hi });
    }
    out
}
