//! Coercions (rustc's `coercion.rs`): the implicit conversions at a site
//! with a known expected type.
//!
//! - `never` to any type;
//! - `&mut T` to `&T`, and `&T` / `&mut T` to raw pointers;
//! - `&[T; N]` to `&[T]`;
//! - `&&T` to `&T` (deref coercion through references);
//! - a function item to a function pointer;
//! - a function literal or function to a `func` value (exception 14).

use vsc_hir::{self as hir, HirId, LangItem};
use vsc_infer::TypeError;
use vsc_middle::ty::*;

use crate::FnCtxt;

impl<'a, 'tcx> FnCtxt<'a, 'tcx> {
    /// Coerces `e`, whose type is `from`, to `to`; reports a mismatch.
    pub fn demand_coerce(&self, e: &hir::Expr, from: Ty<'tcx>, to: Ty<'tcx>) -> Ty<'tcx> {
        match self.try_coerce(Some(e.hir_id), from, to) {
            Ok(t) => t,
            Err(_) => {
                self.report_mismatch(e.span, to, from);
                to
            }
        }
    }

    pub fn try_coerce(&self, id: Option<HirId>, from: Ty<'tcx>, to: Ty<'tcx>) -> Result<Ty<'tcx>, TypeError> {
        let infcx = self.infcx;
        let r = infcx.commit_if_ok(|| self.coerce_inner(from, to));
        match r {
            Ok((ty, adj)) => {
                if let Some(id) = id {
                    self.write_adjustments(id, adj);
                }
                Ok(ty)
            }
            Err(e) => Err(e),
        }
    }

    fn coerce_inner(&self, from: Ty<'tcx>, to: Ty<'tcx>) -> Result<(Ty<'tcx>, Vec<Adjustment<'tcx>>), TypeError> {
        let tcx = self.tcx;
        let infcx = self.infcx;
        let a = infcx.shallow_resolve(from);
        let b = infcx.shallow_resolve(to);
        if matches!(a.kind(), TyKind::Never) {
            if matches!(b.kind(), TyKind::Never) {
                return Ok((b, Vec::new()));
            }
            return Ok((b, vec![Adjustment { kind: Adjust::NeverToAny, target: b }]));
        }
        if matches!(a.kind(), TyKind::Infer(InferTy::TyVar(_))) || matches!(b.kind(), TyKind::Infer(InferTy::TyVar(_))) {
            infcx.unify(a, b)?;
            return Ok((b, Vec::new()));
        }
        match (*a.kind(), *b.kind()) {
            (TyKind::Ref(_, ta, ma), TyKind::Ref(_, tb, mb)) if ma.is_mut() || !mb.is_mut() => {
                // The same type, possibly reborrowed `&mut` to `&`.
                if infcx.probe(|| infcx.unify(ta, tb).is_ok()) {
                    infcx.unify(ta, tb)?;
                    let adj = if ma != mb {
                        vec![Adjustment { kind: Adjust::Deref, target: ta }, Adjustment { kind: Adjust::Borrow(mb), target: b }]
                    } else {
                        Vec::new()
                    };
                    return Ok((b, adj));
                }
                // `&[T; N]` to `&[T]`.
                if let (TyKind::Array(t, _), TyKind::Slice(u)) = (*infcx.shallow_resolve(ta).kind(), *infcx.shallow_resolve(tb).kind()) {
                    infcx.unify(t, u)?;
                    return Ok((b, vec![Adjustment { kind: Adjust::Unsize, target: b }]));
                }
                // `&&T` to `&T`.
                let mut t = infcx.shallow_resolve(ta);
                let mut adj = vec![Adjustment { kind: Adjust::Deref, target: ta }];
                while let TyKind::Ref(_, inner, _) = *t.kind() {
                    adj.push(Adjustment { kind: Adjust::Deref, target: inner });
                    if infcx.probe(|| infcx.unify(inner, tb).is_ok()) {
                        infcx.unify(inner, tb)?;
                        adj.push(Adjustment { kind: Adjust::Borrow(mb), target: b });
                        return Ok((b, adj));
                    }
                    t = infcx.shallow_resolve(inner);
                }
                Err(TypeError)
            }
            (TyKind::Ref(_, ta, ma), TyKind::RawPtr(tb, mb)) if ma.is_mut() || !mb.is_mut() => {
                infcx.unify(ta, tb)?;
                Ok((b, vec![Adjustment { kind: Adjust::Deref, target: ta }, Adjustment { kind: Adjust::Borrow(mb), target: b }]))
            }
            (TyKind::RawPtr(ta, Mutability::Mut), TyKind::RawPtr(tb, Mutability::Not)) => {
                infcx.unify(ta, tb)?;
                Ok((b, Vec::new()))
            }
            (TyKind::FnDef(def, args), TyKind::FnPtr(sig)) => {
                let fsig = self.raw_sig(def, args);
                infcx.unify_sigs(fsig, sig.value)?;
                Ok((b, vec![Adjustment { kind: Adjust::ReifyFnPointer, target: b }]))
            }
            (TyKind::Closure(_, cargs), TyKind::FnPtr(sig)) => {
                let csig = closure_sig(cargs);
                infcx.unify_sigs(csig, sig.value)?;
                Ok((b, vec![Adjustment { kind: Adjust::ReifyFnPointer, target: b }]))
            }
            // A `func` value from a function or function literal
            // (exception 14).
            (TyKind::FnDef(..) | TyKind::Closure(..) | TyKind::FnPtr(_), TyKind::Adt(d, bargs))
                if tcx.is_lang_item(d.did(), LangItem::Func) && bargs.len() == 1 =>
            {
                let TyKind::FnPtr(target) = *infcx.shallow_resolve(bargs[0].expect_ty()).kind() else {
                    infcx.unify(a, b)?;
                    return Ok((b, Vec::new()));
                };
                let sig = match *a.kind() {
                    TyKind::FnDef(def, args) => self.raw_sig(def, args),
                    TyKind::Closure(_, cargs) => closure_sig(cargs),
                    TyKind::FnPtr(s) => s.value,
                    _ => unreachable!(),
                };
                // A `func` value is called with Rust's ABI and is safe.
                let sig = FnSig { safety: target.value.safety, abi: target.value.abi, ..sig };
                infcx.unify_sigs(sig, target.value)?;
                Ok((b, vec![Adjustment { kind: Adjust::ToFunc, target: b }]))
            }
            _ => {
                infcx.unify(a, b)?;
                Ok((b, Vec::new()))
            }
        }
    }
}

/// A function literal's signature, kept as its type's argument.
pub fn closure_sig(cargs: GenericArgsRef<'_>) -> FnSig<'_> {
    match *cargs[0].expect_ty().kind() {
        TyKind::FnPtr(s) => s.value,
        _ => unreachable!("a function literal's argument is its signature"),
    }
}
