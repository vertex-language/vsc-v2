//! Lowering patterns to THIR (rustc's `thir/pattern`): default binding
//! modes' dereferences become explicit `Deref` patterns, and literals and
//! ranges become constant values.

use vsc_ast::UnOp;
use vsc_ast::token::LitKind;
use vsc_hir::def::{DefKind, Res};
use vsc_hir::{self as hir};
use vsc_middle::thir::*;
use vsc_middle::ty::{Ty, TyKind};
use vsc_span::def_id::DefId;

use super::cx::Cx;

impl<'tcx> Cx<'tcx> {
    pub fn lower_pattern(&mut self, p: &hir::Pat) -> Pat<'tcx> {
        let mut pat = self.lower_pattern_unadjusted(p);
        if let Some(refs) = self.results.pat_adjustments.get(&p.hir_id) {
            for &ref_ty in refs.iter().rev() {
                pat = Pat { ty: ref_ty, span: p.span, kind: PatKind::Deref { subpattern: Box::new(pat) } };
            }
        }
        pat
    }

    fn pat_ty(&self, p: &hir::Pat) -> Ty<'tcx> {
        self.results.node_type(p.hir_id).unwrap_or(self.tcx.types.error)
    }

    fn lower_pattern_unadjusted(&mut self, p: &hir::Pat) -> Pat<'tcx> {
        let tcx = self.tcx;
        let ty = self.pat_ty(p);
        let kind = match &p.kind {
            hir::PatKind::Wild => PatKind::Wild,
            hir::PatKind::Err => PatKind::Error,
            hir::PatKind::Binding(hir::BindingMode(_, mutbl), id, ident, sub) => {
                let mode = match self.results.pat_binding_modes.get(&p.hir_id) {
                    Some(hir::BindingMode(hir::ByRef::Yes(m), _)) => ByRef::Yes(*m),
                    _ => ByRef::No,
                };
                let var_ty = match mode {
                    ByRef::Yes(m) => tcx.mk_ref(tcx.regions.re_erased, ty, m),
                    ByRef::No => ty,
                };
                PatKind::Binding {
                    name: ident.name,
                    mutability: *mutbl,
                    mode,
                    var: *id,
                    ty: var_ty,
                    subpattern: sub.as_ref().map(|s| Box::new(self.lower_pattern(s))),
                }
            }
            hir::PatKind::Struct(q, fields, _) => {
                let subpatterns = fields
                    .iter()
                    .map(|f| FieldPat { field: self.results.field_indices.get(&f.hir_id).copied().unwrap_or(0), pattern: self.lower_pattern(&f.pat) })
                    .collect();
                self.adt_pattern(ty, self.variant_of_path(q, p.hir_id, ty), subpatterns)
            }
            hir::PatKind::TupleStruct(q, pats, rest) => {
                let variant = self.variant_of_path(q, p.hir_id, ty);
                let n = match *ty.kind() {
                    TyKind::Adt(adt, _) => adt.variants().get(variant).map_or(0, |v| v.fields.len()),
                    _ => 0,
                };
                let subpatterns = self.positional(pats, *rest, n);
                self.adt_pattern(ty, variant, subpatterns)
            }
            hir::PatKind::Path(q) => match self.pat_path_res(q) {
                Some(Res::Def(DefKind::Const | DefKind::AssocConst, def)) => PatKind::Constant { value: PatConst::Opaque(def) },
                _ => {
                    let variant = self.variant_of_path(q, p.hir_id, ty);
                    self.adt_pattern(ty, variant, Vec::new())
                }
            },
            hir::PatKind::Tuple(pats, rest) => {
                let n = match *ty.kind() {
                    TyKind::Tuple(tys) => tys.len(),
                    _ => pats.len(),
                };
                PatKind::Leaf { subpatterns: self.positional(pats, *rest, n) }
            }
            hir::PatKind::Ref(inner, _) => PatKind::Deref { subpattern: Box::new(self.lower_pattern(inner)) },
            hir::PatKind::Expr(e) => match self.const_of_expr(e) {
                Some(value) => PatKind::Constant { value },
                None => PatKind::Error,
            },
            hir::PatKind::Range(lo, hi, end) => PatKind::Range {
                lo: lo.as_ref().and_then(|e| self.const_of_expr(e)),
                hi: hi.as_ref().and_then(|e| self.const_of_expr(e)),
                end: match end {
                    hir::RangeEnd::Included => RangeEnd::Included,
                    hir::RangeEnd::Excluded => RangeEnd::Excluded,
                },
            },
            hir::PatKind::Slice(before, slice, after) => {
                let prefix = before.iter().map(|x| self.lower_pattern(x)).collect();
                let slice = slice.as_ref().map(|x| Box::new(self.lower_pattern(x)));
                let suffix = after.iter().map(|x| self.lower_pattern(x)).collect();
                match ty.kind() {
                    TyKind::Array(..) => PatKind::Array { prefix, slice, suffix },
                    _ => PatKind::Slice { prefix, slice, suffix },
                }
            }
            hir::PatKind::Or(alts) => PatKind::Or { pats: alts.iter().map(|a| self.lower_pattern(a)).collect() },
        };
        Pat { ty, span: p.span, kind }
    }

    /// Elements with a `..` among them, as fields by position.
    fn positional(&mut self, pats: &[hir::Pat], rest: Option<usize>, n: usize) -> Vec<FieldPat<'tcx>> {
        pats.iter()
            .enumerate()
            .map(|(i, x)| {
                let field = match rest {
                    Some(r) if i >= r => n.saturating_sub(pats.len() - i),
                    _ => i,
                };
                FieldPat { field, pattern: self.lower_pattern(x) }
            })
            .collect()
    }

    fn adt_pattern(&self, ty: Ty<'tcx>, variant_index: usize, subpatterns: Vec<FieldPat<'tcx>>) -> PatKind<'tcx> {
        match *ty.kind() {
            TyKind::Adt(adt_def, args) if adt_def.is_enum() => PatKind::Variant { adt_def, args, variant_index, subpatterns },
            TyKind::Adt(..) => PatKind::Leaf { subpatterns },
            _ => PatKind::Error,
        }
    }

    fn pat_path_res(&self, q: &hir::QPath) -> Option<Res<hir::HirId>> {
        match q {
            hir::QPath::Resolved(_, p) => Some(p.res),
            hir::QPath::LangItem(item, _) => {
                let d = self.tcx.lang_items().get(*item)?;
                Some(Res::Def(self.tcx.def_kind(d), d))
            }
            hir::QPath::TypeRelative(..) => None,
        }
    }

    /// Which variant a pattern's path names.
    fn variant_of_path(&self, q: &hir::QPath, _id: hir::HirId, ty: Ty<'tcx>) -> usize {
        let TyKind::Adt(adt, _) = *ty.kind() else { return 0 };
        if !adt.is_enum() {
            return 0;
        }
        let def: Option<DefId> = match q {
            hir::QPath::TypeRelative(_, seg) => return adt.variants().iter().position(|v| v.name == seg.ident.name).unwrap_or(0),
            _ => match self.pat_path_res(q) {
                Some(Res::Def(DefKind::Ctor(..), c)) => Some(self.tcx.parent(c)),
                Some(Res::Def(DefKind::Variant, v)) => Some(v),
                _ => None,
            },
        };
        def.and_then(|d| adt.variants().iter().position(|v| v.def_id == d)).unwrap_or(0)
    }

    /// The constant a literal or constant path in a pattern denotes.
    fn const_of_expr(&self, e: &hir::Expr) -> Option<PatConst> {
        let ty = self.results.node_type(e.hir_id)?;
        match &e.kind {
            hir::ExprKind::Lit(lit) => lit_const(lit, ty, false),
            hir::ExprKind::Unary(UnOp::Neg, inner) => match &inner.kind {
                hir::ExprKind::Lit(lit) => lit_const(lit, ty, true),
                _ => None,
            },
            hir::ExprKind::Path(q) => match self.pat_path_res(q) {
                Some(Res::Def(DefKind::Const | DefKind::AssocConst, d)) => Some(PatConst::Opaque(d)),
                _ => None,
            },
            _ => None,
        }
    }
}

/// A literal's value in a pattern of type `ty`.
pub fn lit_const(lit: &vsc_ast::token::Lit, ty: Ty<'_>, neg: bool) -> Option<PatConst> {
    let text = lit.symbol.as_str();
    match lit.kind {
        LitKind::Bool => Some(PatConst::Bool(text == "true")),
        LitKind::Char => parse_char_lit(text).map(|c| PatConst::Int(c as u128)),
        LitKind::Byte => parse_char_lit(text).map(|c| PatConst::Int(c as u128)),
        LitKind::Integer => {
            let v = parse_int_lit(text)?;
            let v = if neg { (v as i128).wrapping_neg() as u128 } else { v };
            match ty.kind() {
                TyKind::Int(_) | TyKind::Uint(_) => Some(PatConst::Int(v)),
                _ => Some(PatConst::Int(v)),
            }
        }
        LitKind::Float => Some(PatConst::Float(if neg { vsc_span::Symbol::intern(&format!("-{text}")) } else { lit.symbol })),
        LitKind::Str | LitKind::StrRaw(_) => Some(PatConst::Str(lit.symbol)),
        _ => None,
    }
}
