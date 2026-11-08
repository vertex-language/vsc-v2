//! Checking patterns (rustc's `pat.rs`), with Rust's default binding modes:
//! matching a reference against a non-reference pattern looks through it
//! and binds by reference.

use vsc_data_structures::FxHashMap;
use vsc_errors::ErrCode;
use vsc_hir::def::{CtorKind, DefKind, Res};
use vsc_hir::{self as hir, ByRef, HirId};
use vsc_middle::ty::*;
use vsc_span::{Span, Symbol};

use crate::FnCtxt;

/// How bindings bind when nothing is written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DefaultBinding {
    Move,
    Ref(Mutability),
}

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    pub fn check_pat_top(&self, pat: &hir::Pat, expected: Ty<'tcx>) {
        self.check_pat(pat, expected, DefaultBinding::Move);
    }

    fn check_pat(&self, pat: &hir::Pat, expected: Ty<'tcx>, def_bm: DefaultBinding) {
        let tcx = self.tcx;
        // Default binding modes: peel references off the scrutinee for
        // patterns that aren't references or bindings.
        let (expected, def_bm) = if peels_refs(pat) {
            // The pattern looks at the type's shape: prove pending bounds
            // first, so a projection (a `for` loop's element type, say) is
            // known before the pattern decides it.
            if matches!(self.infcx.shallow_resolve(expected).kind(), TyKind::Infer(InferTy::TyVar(_))) {
                self.select_where_possible();
            }
            let mut t = self.infcx.shallow_resolve(expected);
            let mut bm = def_bm;
            let mut peeled = Vec::new();
            while let TyKind::Ref(_, inner, m) = *t.kind() {
                peeled.push(t);
                bm = match bm {
                    DefaultBinding::Ref(Mutability::Not) => DefaultBinding::Ref(Mutability::Not),
                    _ => DefaultBinding::Ref(m),
                };
                t = self.infcx.shallow_resolve(inner);
            }
            if !peeled.is_empty() {
                self.results.borrow_mut().pat_adjustments.insert(pat.hir_id, peeled);
            }
            (t, bm)
        } else {
            (expected, def_bm)
        };
        let ty = match &pat.kind {
            hir::PatKind::Wild | hir::PatKind::Err => expected,
            hir::PatKind::Binding(hir::BindingMode(by_ref, _), id, _, sub) => {
                let (local, mode) = match (by_ref, def_bm) {
                    (ByRef::Yes(m), _) => (tcx.mk_ref(tcx.regions.re_erased, expected, *m), ByRef::Yes(*m)),
                    (ByRef::No, DefaultBinding::Ref(m)) => (tcx.mk_ref(tcx.regions.re_erased, expected, m), ByRef::Yes(m)),
                    (ByRef::No, DefaultBinding::Move) => (expected, ByRef::No),
                };
                let hir::PatKind::Binding(hir::BindingMode(_, mutbl), ..) = pat.kind else { unreachable!() };
                self.results.borrow_mut().pat_binding_modes.insert(pat.hir_id, hir::BindingMode(mode, mutbl));
                self.declare_local(*id, local);
                if let Some(s) = sub {
                    self.check_pat(s, expected, def_bm);
                }
                // The pattern matches `expected`; the variable's type
                // follows from its binding mode.
                let _ = local;
                expected
            }
            hir::PatKind::Struct(q, fields, rest) => {
                match self.resolve_struct_path(q, pat.span) {
                    Some((ty, adt, args, vi)) => {
                        self.demand_eq(pat.span, expected, ty);
                        let variant = &adt.variants()[vi];
                        let mut seen = vec![false; variant.fields.len()];
                        for f in fields {
                            match variant.fields.iter().position(|vf| vf.name == f.ident.name) {
                                Some(i) => {
                                    seen[i] = true;
                                    self.write_field_index(f.hir_id, i);
                                    let ft = self.instantiate_type_of(variant.fields[i].did, args, f.span);
                                    self.check_pat(&f.pat, ft, def_bm);
                                }
                                None => {
                                    self.check_pat(&f.pat, tcx.types.error, def_bm);
                                    self.err(f.ident.span, format!("`{}` does not have a field named `{}`", tcx.def_path_str(variant.def_id), f.ident))
                                        .with_code(ErrCode(26))
                                        .with_span_label(f.ident.span, "unknown field")
                                        .emit();
                                }
                            }
                        }
                        let missing: Vec<String> =
                            variant.fields.iter().zip(&seen).filter(|(_, s)| !**s).map(|(f, _)| format!("`{}`", f.name)).collect();
                        if !rest && !missing.is_empty() && adt.kind() != AdtKind::Union {
                            let what = if missing.len() == 1 { "field" } else { "fields" };
                            self.err(pat.span, format!("pattern does not mention {what} {}", missing.join(", ")))
                                .with_code(ErrCode(27))
                                .with_help("ignore the other fields with `..`")
                                .emit();
                        }
                        expected
                    }
                    None => {
                        for f in fields {
                            self.check_pat(&f.pat, tcx.types.error, def_bm);
                        }
                        tcx.types.error
                    }
                }
            }
            hir::PatKind::TupleStruct(q, pats, rest) => self.check_pat_tuple_struct(pat, q, pats, *rest, expected, def_bm),
            hir::PatKind::Path(q) => {
                let t = self.check_pat_path(q, pat.hir_id, pat.span);
                self.demand_eq(pat.span, expected, t);
                expected
            }
            hir::PatKind::Tuple(pats, rest) => {
                let t = self.infcx.shallow_resolve(expected);
                let elems: Vec<Ty<'tcx>> = match *t.kind() {
                    TyKind::Tuple(tys) => tys.to_vec(),
                    _ => {
                        // Unknown: a tuple of new variables, unified below.
                        let n = if rest.is_some() { pats.len() } else { pats.len() };
                        let tys: Vec<_> = (0..n).map(|_| self.infcx.next_ty_var()).collect();
                        if rest.is_none() {
                            let tup = tcx.mk_tup(&tys);
                            self.demand_eq(pat.span, expected, tup);
                        } else if !matches!(t.kind(), TyKind::Error) {
                            self.err(pat.span, "type annotations needed").with_code(ErrCode(282)).emit();
                        }
                        tys
                    }
                };
                self.check_elements(pat.span, pats, *rest, &elems, def_bm, "tuple", &format!("a tuple with {} elements", elems.len()));
                expected
            }
            hir::PatKind::Ref(inner, m) => {
                let t = self.infcx.shallow_resolve(expected);
                let inner_ty = match *t.kind() {
                    TyKind::Ref(_, it, em) if em == *m => it,
                    TyKind::Error => tcx.types.error,
                    _ => {
                        let v = self.infcx.next_ty_var();
                        let r = tcx.mk_ref(tcx.regions.re_erased, v, *m);
                        self.demand_eq(pat.span, expected, r);
                        v
                    }
                };
                self.check_pat(inner, inner_ty, DefaultBinding::Move);
                expected
            }
            hir::PatKind::Expr(e) => {
                let t = self.check_expr_with_expectation(e, crate::Expectation::HasType(expected));
                // A string literal pattern matches `&string` and `string`.
                self.demand_eq(pat.span, expected, t);
                expected
            }
            hir::PatKind::Range(lo, hi, _) => {
                for e in [lo, hi].into_iter().flatten() {
                    let t = self.check_expr_with_expectation(e, crate::Expectation::HasType(expected));
                    self.demand_eq(e.span, expected, t);
                }
                let t = self.infcx.shallow_resolve(expected);
                if !matches!(t.kind(), TyKind::Char | TyKind::Int(_) | TyKind::Uint(_) | TyKind::Float(_) | TyKind::Infer(_) | TyKind::Error) {
                    self.err(pat.span, "only `char` and numeric types are allowed in range patterns").with_code(ErrCode(29)).emit();
                }
                expected
            }
            hir::PatKind::Slice(before, slice, after) => {
                let t = self.infcx.shallow_resolve(expected);
                let (elem, len) = match *t.kind() {
                    TyKind::Array(e, n) => (e, Some(n)),
                    TyKind::Slice(e) => (e, None),
                    TyKind::Error => (tcx.types.error, None),
                    _ => {
                        self.err(pat.span, format!("expected an array or slice, found `{}`", self.ty_str(t))).with_code(ErrCode(529)).emit();
                        (tcx.types.error, None)
                    }
                };
                for p in before.iter().chain(after.iter()) {
                    self.check_pat(p, elem, def_bm);
                }
                let fixed = (before.len() + after.len()) as u128;
                if let Some(s) = slice {
                    let rest_ty = match len.map(|n| *n.kind()) {
                        Some(ConstKind::Value(ut, n)) if n >= fixed => {
                            tcx.mk_ty(TyKind::Array(elem, tcx.mk_const(ConstKind::Value(ut, n - fixed))))
                        }
                        Some(_) => tcx.mk_ty(TyKind::Array(elem, tcx.mk_const(ConstKind::Error))),
                        None => tcx.mk_ty(TyKind::Slice(elem)),
                    };
                    self.check_pat(s, rest_ty, def_bm);
                } else if let Some(n) = len {
                    if let ConstKind::Value(_, n) = *n.kind() {
                        if n != fixed {
                            self.err(pat.span, format!("pattern requires {fixed} elements but array has {n}")).with_code(ErrCode(527)).emit();
                        }
                    }
                }
                expected
            }
            hir::PatKind::Or(alts) => {
                for a in alts {
                    self.check_pat(a, expected, def_bm);
                }
                // Every alternative binds the same names, of the same types.
                let mut first: FxHashMap<Symbol, (HirId, Span)> = FxHashMap::default();
                collect_bindings(&alts[0], &mut first);
                for a in &alts[1..] {
                    let mut these = FxHashMap::default();
                    collect_bindings(a, &mut these);
                    for (name, (id, span)) in these {
                        if let (Some(&(fid, _)), Some(t)) = (first.get(&name), self.local_ty(id)) {
                            if let Some(ft) = self.local_ty(fid) {
                                if self.infcx.commit_if_ok(|| self.infcx.unify(ft, t)).is_err() {
                                    self.err(span, "mismatched types")
                                        .with_code(ErrCode(308))
                                        .with_span_label(span, format!("expected `{}`, found `{}`", self.ty_str(ft), self.ty_str(t)))
                                        .with_note("in the same arm, a binding must have the same type in all alternatives")
                                        .emit();
                                }
                            }
                        }
                    }
                }
                expected
            }
        };
        self.write_ty(pat.hir_id, pat.span, ty);
    }

    fn check_elements(
        &self,
        span: Span,
        pats: &[hir::Pat],
        rest: Option<usize>,
        elems: &[Ty<'tcx>],
        def_bm: DefaultBinding,
        what: &str,
        has: &str,
    ) {
        let ok = match rest {
            None => pats.len() == elems.len(),
            Some(_) => pats.len() <= elems.len(),
        };
        if !ok {
            let plural = |n: usize| if n == 1 { "" } else { "s" };
            self.err(span, format!("this pattern has {} field{}, but the corresponding {what} has {}", pats.len(), plural(pats.len()), elems.len()))
                .with_code(ErrCode(if what == "tuple" { 308 } else { 23 }))
                .with_span_label(span, format!("expected {has}"))
                .emit();
            for p in pats {
                self.check_pat(p, self.tcx.types.error, def_bm);
            }
            return;
        }
        for (i, p) in pats.iter().enumerate() {
            // After `..`, elements count from the end.
            let index = match rest {
                Some(r) if i >= r => elems.len() - (pats.len() - i),
                _ => i,
            };
            self.check_pat(p, elems[index], def_bm);
        }
    }

    fn check_pat_tuple_struct(
        &self,
        pat: &hir::Pat,
        q: &hir::QPath,
        pats: &[hir::Pat],
        rest: Option<usize>,
        expected: Ty<'tcx>,
        def_bm: DefaultBinding,
    ) -> Ty<'tcx> {
        let tcx = self.tcx;
        let ctor = match q {
            hir::QPath::Resolved(None, path) => match path.res {
                Res::Def(DefKind::Ctor(_, CtorKind::Fn), ctor) => Some(ctor),
                Res::SelfCtor(imp) => match *tcx.erase_regions(tcx.type_of(imp)).kind() {
                    TyKind::Adt(adt, _) if !adt.is_enum() => adt.non_enum_variant().ctor.map(|(_, c)| c),
                    _ => None,
                },
                _ => None,
            },
            hir::QPath::LangItem(item, span) => tcx.require_lang_item(*item, *span).and_then(|v| {
                let adt = tcx.adt_def(tcx.parent(v));
                adt.variants().iter().find(|x| x.def_id == v).and_then(|x| x.ctor).map(|(_, c)| c)
            }),
            hir::QPath::TypeRelative(base, seg) => {
                let bt = self.lower_ty(base);
                match *self.infcx.shallow_resolve(bt).kind() {
                    TyKind::Adt(adt, _) if adt.is_enum() => {
                        adt.variants().iter().find(|v| v.name == seg.ident.name).and_then(|v| v.ctor).map(|(_, c)| c)
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        let Some(ctor) = ctor else {
            for p in pats {
                self.check_pat(p, tcx.types.error, def_bm);
            }
            return tcx.types.error;
        };
        let args = self.infcx.fresh_args_for_item(ctor);
        let sig = self.instantiate_sig(ctor, args, pat.span);
        self.demand_eq(pat.span, expected, sig.output());
        let what = if tcx.def_kind(tcx.parent(ctor)) == DefKind::Variant { "tuple variant" } else { "tuple struct" };
        let has = format!("{} field{}", sig.inputs().len(), if sig.inputs().len() == 1 { "" } else { "s" });
        self.check_elements(pat.span, pats, rest, sig.inputs(), def_bm, what, &has);
        expected
    }

    /// A path pattern: a unit struct or variant, or a constant.
    fn check_pat_path(&self, q: &hir::QPath, id: HirId, span: Span) -> Ty<'tcx> {
        let tcx = self.tcx;
        match q {
            hir::QPath::LangItem(item, span) => match tcx.require_lang_item(*item, *span) {
                Some(v) if tcx.def_kind(v) == DefKind::Variant => {
                    let adt = tcx.adt_def(tcx.parent(v));
                    tcx.mk_adt(adt, self.infcx.fresh_args_for_item(adt.did()))
                }
                _ => tcx.types.error,
            },
            _ => self.check_expr_path(q, id, span),
        }
    }
}

/// Patterns that look through references under default binding modes.
fn peels_refs(pat: &hir::Pat) -> bool {
    matches!(
        pat.kind,
        hir::PatKind::Struct(..)
            | hir::PatKind::TupleStruct(..)
            | hir::PatKind::Tuple(..)
            | hir::PatKind::Slice(..)
            | hir::PatKind::Range(..)
            | hir::PatKind::Path(_)
    ) || matches!(&pat.kind, hir::PatKind::Expr(e) if !matches!(&e.kind, hir::ExprKind::Lit(l) if matches!(l.kind, vsc_ast::token::LitKind::Str | vsc_ast::token::LitKind::StrRaw(_))))
}

fn collect_bindings(pat: &hir::Pat, out: &mut FxHashMap<Symbol, (HirId, Span)>) {
    use vsc_hir::intravisit::{self, Visitor};
    struct C<'a>(&'a mut FxHashMap<Symbol, (HirId, Span)>);
    impl<'v> Visitor<'v> for C<'_> {
        fn visit_pat(&mut self, p: &'v hir::Pat) {
            if let hir::PatKind::Binding(_, id, ident, _) = p.kind {
                self.0.insert(ident.name, (id, p.span));
            }
            intravisit::walk_pat(self, p)
        }
    }
    C(out).visit_pat(pat);
}
