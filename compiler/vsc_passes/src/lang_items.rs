//! Collecting `@lang(name)` attributes (rustc's `lang_items.rs`).

use vsc_ast::{AttrArgs, AttrKind, Attribute};
use vsc_errors::ErrCode;
use vsc_hir::def::DefKind;
use vsc_hir::intravisit::{self, Visitor};
use vsc_hir::*;
use vsc_middle::middle::lang_items::LanguageItems;
use vsc_middle::ty::TyCtxt;
use vsc_span::Span;
use vsc_span::def_id::{CRATE_DEF_ID, LocalDefId};

pub fn collect(tcx: TyCtxt<'_>) -> LanguageItems {
    let mut c = Collector { tcx, items: LanguageItems::default(), spans: Default::default() };
    if let OwnerNode::Crate(m) = &tcx.hir_crate.owner(CRATE_DEF_ID).node {
        c.visit_mod(m);
    }
    c.items
}

struct Collector<'tcx> {
    tcx: TyCtxt<'tcx>,
    items: LanguageItems,
    spans: vsc_data_structures::FxHashMap<LangItem, Span>,
}

/// The name in `@lang(name)` or `@lang("name")`.
fn lang_attr(a: &Attribute) -> Option<(String, Span)> {
    let AttrKind::Normal(n) = &a.kind else { return None };
    if a.name()?.as_str() != "lang" {
        return None;
    }
    match &n.args {
        AttrArgs::Delimited(d) => {
            let text = vsc_ast_pretty::pprust::tts_to_string(&d.tokens);
            Some((text.trim().trim_matches('"').to_string(), a.span))
        }
        AttrArgs::Eq { expr, .. } => Some((vsc_ast_pretty::expr_to_string(expr).trim().trim_matches('"').to_string(), a.span)),
        AttrArgs::Empty => Some((String::new(), a.span)),
    }
}

impl<'tcx> Collector<'tcx> {
    fn check(&mut self, owner: LocalDefId, local: ItemLocalId, def: LocalDefId) {
        let Some(attrs) = self.tcx.hir_crate.owner(owner).attrs.get(&local) else { return };
        for a in attrs {
            let Some((name, span)) = lang_attr(a) else { continue };
            let Some(item) = LangItem::from_name(vsc_span::Symbol::intern(&name)) else {
                self.tcx
                    .dcx()
                    .struct_span_err(span, format!("definition of an unknown lang item: `{name}`"))
                    .with_code(ErrCode(522))
                    .emit();
                continue;
            };
            let kind = self.tcx.def_kind(def.to_def_id());
            let fits = match item.target() {
                "struct" => matches!(kind, DefKind::Struct),
                "enum" => matches!(kind, DefKind::Enum),
                "variant" => matches!(kind, DefKind::Variant),
                "method" => matches!(kind, DefKind::AssocFn),
                "fn" => matches!(kind, DefKind::Fn),
                "trait" => matches!(kind, DefKind::Trait),
                _ => false,
            };
            if !fits {
                self.tcx
                    .dcx()
                    .struct_span_err(span, format!("`{name}` lang item must be applied to a {}", item.target()))
                    .with_code(ErrCode(718))
                    .with_span_label(span, format!("attribute should be applied to a {}, not a {}", item.target(), kind.descr()))
                    .emit();
                continue;
            }
            if let Some(&prev) = self.spans.get(&item) {
                self.tcx
                    .dcx()
                    .struct_span_err(span, format!("found duplicate lang item `{name}`"))
                    .with_code(ErrCode(152))
                    .with_span_note(prev, "the lang item is first defined here")
                    .emit();
                continue;
            }
            self.spans.insert(item, span);
            self.items.items.insert(item, def.to_def_id());
        }
    }
}

impl<'tcx> Visitor<'tcx> for Collector<'tcx> {
    fn krate(&self) -> Option<&'tcx Crate> {
        Some(self.tcx.hir_crate)
    }
    fn visit_item(&mut self, i: &'tcx Item) {
        self.check(i.owner_id, ItemLocalId::ZERO, i.owner_id);
        intravisit::walk_item(self, i)
    }
    fn visit_assoc_item(&mut self, i: &'tcx AssocItem) {
        self.check(i.owner_id, ItemLocalId::ZERO, i.owner_id);
        intravisit::walk_assoc_item(self, i)
    }
    fn visit_foreign_item(&mut self, i: &'tcx ForeignItem) {
        self.check(i.owner_id, ItemLocalId::ZERO, i.owner_id);
        intravisit::walk_foreign_item(self, i)
    }
    fn visit_variant(&mut self, v: &'tcx Variant) {
        self.check(v.hir_id.owner, v.hir_id.local_id, v.def_id);
        intravisit::walk_variant(self, v)
    }
}
