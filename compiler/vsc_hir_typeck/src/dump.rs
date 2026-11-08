//! `-Z dump-types`: each body's bindings and what its method calls and
//! type-relative paths resolved to, for tests and debugging.

use std::fmt::Write;

use vsc_hir::intravisit::{self, Visitor};
use vsc_hir::{self as hir, ItemId};
use vsc_middle::ty::{TyCtxt, TypeckResults};

pub fn dump_types(tcx: TyCtxt<'_>) -> String {
    let mut out = String::new();
    for (local, _) in tcx.defs.iter_enumerated() {
        let Some(body_id) = crate::body_of(tcx, local) else { continue };
        if matches!(tcx.def_kind(local.to_def_id()), vsc_hir::def::DefKind::AnonConst) {
            continue;
        }
        // The core stubs' bodies (`//@ add-core-stubs`) aren't the test's.
        let file = tcx.sess.source_map.lookup_source_file(tcx.def_span(local.to_def_id()).lo());
        if file.is_some_and(|f| f.name.to_string().ends_with("minicore.vs")) {
            continue;
        }
        let results = tcx.typeck(local.to_def_id());
        let _ = writeln!(out, "{}", tcx.def_path_str(local.to_def_id()));
        let mut v = Dump { tcx, results, lines: Vec::new() };
        v.visit_body(tcx.hir_crate.body(body_id));
        for line in v.lines {
            let _ = writeln!(out, "    {line}");
        }
    }
    out
}

struct Dump<'a, 'tcx> {
    tcx: TyCtxt<'tcx>,
    results: &'a TypeckResults<'tcx>,
    lines: Vec<String>,
}

impl<'tcx> Visitor<'tcx> for Dump<'_, 'tcx> {
    fn krate(&self) -> Option<&'tcx hir::Crate> {
        Some(self.tcx.hir_crate)
    }

    // Nested items are bodies of their own.
    fn visit_nested_item(&mut self, _: ItemId) {}

    fn visit_pat(&mut self, p: &'tcx hir::Pat) {
        if let hir::PatKind::Binding(_, _, ident, _) = p.kind {
            // Desugarings' bindings (`iter`, `val`, …) show too.
            if let Some(t) = self.results.node_type(p.hir_id) {
                let t = match self.results.pat_binding_modes.get(&p.hir_id) {
                    Some(vsc_hir::BindingMode(vsc_hir::ByRef::Yes(m), _)) => self.tcx.mk_ref(self.tcx.regions.re_erased, t, *m),
                    _ => t,
                };
                self.lines.push(format!("{ident}: {}", self.tcx.ty_to_string(t)));
            }
        }
        intravisit::walk_pat(self, p)
    }

    fn visit_expr(&mut self, e: &'tcx hir::Expr) {
        match &e.kind {
            hir::ExprKind::MethodCall(seg, ..) => {
                if let Some(&(_, def)) = self.results.type_dependent_defs.get(&e.hir_id) {
                    self.lines.push(format!(".{}() -> {}", seg.ident, self.tcx.def_path_str(def)));
                }
            }
            hir::ExprKind::Path(hir::QPath::TypeRelative(_, seg)) => {
                if let Some(&(_, def)) = self.results.type_dependent_defs.get(&e.hir_id) {
                    let ty = self.results.node_type(e.hir_id).map(|t| self.tcx.ty_to_string(t)).unwrap_or_default();
                    self.lines.push(format!("{} -> {}: {ty}", seg.ident, self.tcx.def_path_str(def)));
                }
            }
            _ => {}
        }
        intravisit::walk_expr(self, e)
    }
}
