//! Printing HIR (rustc's `rustc_hir_pretty`), for `-Z unpretty=hir`.
//!
//! HIR has no Vertex spellings left, so it prints in Rust's syntax, as
//! rustc's printer does: `fn`, `::`, `'a` for origins, `#[lang = "…"]` for
//! lang item paths. Desugarings show as what they became. The few things
//! Rust has no syntax for keep Vertex's: `class`, `func(T) -> U` function
//! value types, and `'[a, b]` for an origin naming several parameters.

use std::fmt::Write;

use vsc_ast::{BinOpKind, ByRef, CaptureBy, Mutability, RangeEnd, Safety, UnOp};
use vsc_hir::*;
use vsc_span::kw;

/// The whole crate, from its root package.
pub fn crate_to_string(krate: &Crate) -> String {
    let mut p = Printer { krate, out: String::new(), indent: 0 };
    let root = krate.owner(vsc_span::def_id::CRATE_DEF_ID);
    p.owner_attrs(root);
    if let OwnerNode::Crate(m) = &root.node {
        p.mod_items(m);
    }
    while p.out.ends_with("\n\n") {
        p.out.pop();
    }
    p.out
}

struct Printer<'a> {
    krate: &'a Crate,
    out: String,
    indent: usize,
}

/// Binary operator precedence, as in Rust: higher binds tighter.
fn prec(op: BinOpKind) -> u8 {
    use BinOpKind::*;
    match op {
        Mul | Div | Rem => 10,
        Add | Sub => 9,
        Shl | Shr => 8,
        BitAnd => 7,
        BitXor => 6,
        BitOr => 5,
        Eq | Lt | Le | Ne | Ge | Gt => 4,
        And => 3,
        Or => 2,
    }
}

const PREC_PREFIX: u8 = 12;
const PREC_POSTFIX: u8 = 13;
/// Assignments, closures, `return`, `break`, ranges: anything that must be
/// parenthesized as an operand.
const PREC_LOWEST: u8 = 0;

fn expr_prec(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Binary(op, ..) => prec(op.node),
        ExprKind::Cast(..) => 11,
        ExprKind::Unary(..) | ExprKind::AddrOf(..) => PREC_PREFIX,
        ExprKind::Assign(..)
        | ExprKind::AssignOp(..)
        | ExprKind::Closure(..)
        | ExprKind::Ret(..)
        | ExprKind::Break(..)
        | ExprKind::Let(..) => PREC_LOWEST,
        _ => PREC_POSTFIX,
    }
}

impl<'a> Printer<'a> {
    fn w(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn nl(&mut self) {
        self.out.push('\n');
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
    }

    fn comma_sep<T>(&mut self, items: &[T], mut f: impl FnMut(&mut Self, &T)) {
        for (i, x) in items.iter().enumerate() {
            if i > 0 {
                self.w(", ");
            }
            f(self, x);
        }
    }

    fn owner_attrs(&mut self, owner: &OwnerInfo) {
        if let Some(attrs) = owner.attrs.get(&ItemLocalId::ZERO) {
            for a in attrs {
                self.w(&vsc_ast_pretty::attribute_to_string(a));
                self.nl();
            }
        }
    }

    /// Attributes on a node inside an owner.
    fn attrs(&mut self, id: HirId) {
        let owner = self.krate.owner(id.owner);
        if id.local_id == ItemLocalId::ZERO {
            return;
        }
        if let Some(attrs) = owner.attrs.get(&id.local_id) {
            for a in attrs {
                self.w(&vsc_ast_pretty::attribute_to_string(a));
                self.nl();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Items
    // -----------------------------------------------------------------------

    fn mod_items(&mut self, m: &Mod) {
        for (i, &id) in m.item_ids.iter().enumerate() {
            if i > 0 {
                self.nl();
            }
            self.item(id);
            self.nl();
        }
    }

    fn safety(&mut self, s: Safety) {
        match s {
            Safety::Unsafe(_) => self.w("unsafe "),
            Safety::Safe(_) => self.w("safe "),
            Safety::Default => {}
        }
    }

    fn vis(&mut self, span: vsc_span::Span) {
        // Visibility is checked in `vsc_resolve`; HIR keeps only where it was
        // written. A non-empty span means something was written.
        if span.lo() != span.hi() {
            self.w("pub ");
        }
    }

    fn item(&mut self, id: ItemId) {
        let owner = self.krate.owner(id.owner_id);
        self.owner_attrs(owner);
        let item = self.krate.item(id);
        self.vis(item.vis_span);
        let name = item.ident.name.as_str();
        match &item.kind {
            ItemKind::Mod(m) => {
                let _ = write!(self.out, "mod {name} {{");
                self.indent += 1;
                self.nl();
                self.mod_items(m);
                self.indent -= 1;
                self.trim_trailing_indent();
                self.w("}");
            }
            ItemKind::Fn { sig, generics, body } => {
                self.fn_sig(name, sig, generics, Some(*body), None);
                self.w(" ");
                self.body_value(*body);
            }
            ItemKind::Static(ty, m, body) => {
                self.w("static ");
                if m.is_mut() {
                    self.w("mut ");
                }
                let _ = write!(self.out, "{name}: ");
                self.ty(ty);
                self.w(" = ");
                self.body_value(*body);
                self.w(";");
            }
            ItemKind::Const(ty, _, body) => {
                let _ = write!(self.out, "const {name}: ");
                self.ty(ty);
                self.w(" = ");
                self.body_value(*body);
                self.w(";");
            }
            ItemKind::TyAlias(ty, g) => {
                let _ = write!(self.out, "type {name}");
                self.generic_params(&g.params);
                self.where_clause(g);
                self.w(" = ");
                self.ty(ty);
                self.w(";");
            }
            ItemKind::Enum(def, g) => {
                let _ = write!(self.out, "enum {name}");
                self.generic_params(&g.params);
                self.where_clause(g);
                self.w(" {");
                self.indent += 1;
                for v in &def.variants {
                    self.nl();
                    self.attrs(v.hir_id);
                    self.w(v.ident.name.as_str());
                    self.variant_data(&v.data, false);
                    if let Some(d) = &v.disr_expr {
                        self.w(" = ");
                        self.anon_const(d);
                    }
                    self.w(",");
                }
                self.indent -= 1;
                self.nl();
                self.w("}");
            }
            ItemKind::Struct(data, g) | ItemKind::Class(data, g) | ItemKind::Union(data, g) => {
                let kw = match item.kind {
                    ItemKind::Struct(..) => "struct",
                    ItemKind::Class(..) => "class",
                    _ => "union",
                };
                let _ = write!(self.out, "{kw} {name}");
                self.generic_params(&g.params);
                self.where_clause(g);
                self.variant_data(data, true);
            }
            ItemKind::Trait { safety, generics, bounds, items } => {
                self.safety(*safety);
                let _ = write!(self.out, "trait {name}");
                self.generic_params(&generics.params);
                if !bounds.is_empty() {
                    self.w(": ");
                    self.bounds(bounds);
                }
                self.where_clause(generics);
                self.assoc_items(items);
            }
            ItemKind::Impl(i) => {
                if i.flat_receiver {
                    self.w("// flat receiver");
                    self.nl();
                }
                self.safety(i.safety);
                self.w("impl");
                self.generic_params(&i.generics.params);
                self.w(" ");
                if let Some(t) = &i.of_trait {
                    if i.negative {
                        self.w("!");
                    }
                    self.path(&t.path);
                    self.w(" for ");
                }
                self.ty(&i.self_ty);
                self.where_clause(&i.generics);
                self.assoc_items(&i.items);
            }
            ItemKind::ForeignMod { abi, items } => {
                self.w("extern ");
                if let Some(abi) = abi {
                    let _ = write!(self.out, "\"{abi}\" ");
                }
                self.w("{");
                self.indent += 1;
                for &def in items {
                    self.nl();
                    let owner = self.krate.owner(def);
                    self.owner_attrs(owner);
                    let fi = self.krate.foreign_item(def);
                    self.vis(fi.vis_span);
                    let fname = fi.ident.name.as_str();
                    match &fi.kind {
                        ForeignItemKind::Fn(sig, names, g) => {
                            self.fn_sig(fname, sig, g, None, Some(names));
                            self.w(";");
                        }
                        ForeignItemKind::Static(ty, m, safety) => {
                            self.safety(*safety);
                            self.w("static ");
                            if m.is_mut() {
                                self.w("mut ");
                            }
                            let _ = write!(self.out, "{fname}: ");
                            self.ty(ty);
                            self.w(";");
                        }
                        ForeignItemKind::Type => {
                            let _ = write!(self.out, "type {fname};");
                        }
                    }
                }
                self.indent -= 1;
                self.nl();
                self.w("}");
            }
            ItemKind::GlobalAsm(asm) => {
                self.inline_asm(asm);
                self.w(";");
            }
        }
    }

    fn trim_trailing_indent(&mut self) {
        while self.out.ends_with(' ') {
            self.out.pop();
        }
    }

    fn assoc_items(&mut self, items: &[vsc_span::def_id::LocalDefId]) {
        self.w(" {");
        self.indent += 1;
        for &def in items {
            self.nl();
            let owner = self.krate.owner(def);
            self.owner_attrs(owner);
            let ai = self.krate.assoc_item(def);
            self.vis(ai.vis_span);
            let name = ai.ident.name.as_str();
            match &ai.kind {
                AssocItemKind::Fn(sig, body) => {
                    self.fn_sig(name, sig, &ai.generics, *body, None);
                    match body {
                        Some(b) => {
                            self.w(" ");
                            self.body_value(*b);
                        }
                        None => self.w(";"),
                    }
                }
                AssocItemKind::Const(ty, body) => {
                    let _ = write!(self.out, "const {name}: ");
                    self.ty(ty);
                    if let Some(b) = body {
                        self.w(" = ");
                        self.body_value(*b);
                    }
                    self.w(";");
                }
                AssocItemKind::Type(bounds, ty) => {
                    let _ = write!(self.out, "type {name}");
                    self.generic_params(&ai.generics.params);
                    if !bounds.is_empty() {
                        self.w(": ");
                        self.bounds(bounds);
                    }
                    self.where_clause(&ai.generics);
                    if let Some(t) = ty {
                        self.w(" = ");
                        self.ty(t);
                    }
                    self.w(";");
                }
            }
        }
        self.indent -= 1;
        self.nl();
        self.w("}");
    }

    fn variant_data(&mut self, data: &VariantData, is_item: bool) {
        match data {
            VariantData::Struct { fields } => {
                self.w(" {");
                self.indent += 1;
                for f in fields {
                    self.nl();
                    self.attrs(f.hir_id);
                    self.vis(f.vis_span);
                    let _ = write!(self.out, "{}: ", f.ident);
                    self.ty(&f.ty);
                    self.w(",");
                }
                self.indent -= 1;
                if !fields.is_empty() {
                    self.nl();
                }
                self.w("}");
            }
            VariantData::Tuple(fields, ..) => {
                self.w("(");
                self.comma_sep(fields, |p, f| {
                    p.vis(f.vis_span);
                    p.ty(&f.ty)
                });
                self.w(")");
                if is_item {
                    self.w(";");
                }
            }
            VariantData::Unit(..) => {
                if is_item {
                    self.w(";");
                }
            }
        }
    }

    /// `fn name<G>(params) -> R where …`. Parameter names come from the
    /// body's patterns, or a foreign function's names.
    fn fn_sig(&mut self, name: &str, sig: &FnSig, g: &Generics, body: Option<BodyId>, names: Option<&[vsc_span::Ident]>) {
        let h = &sig.header;
        if h.is_const {
            self.w("const ");
        }
        if h.is_async {
            self.w("async ");
        }
        self.safety(h.safety);
        if let Some(abi) = h.abi {
            let _ = write!(self.out, "extern \"{abi}\" ");
        }
        let _ = write!(self.out, "fn {name}");
        self.generic_params(&g.params);
        self.w("(");
        let body = body.map(|b| self.krate.body(b));
        for (i, ty) in sig.decl.inputs.iter().enumerate() {
            if i > 0 {
                self.w(", ");
            }
            if i == 0 && self.self_param(sig.decl.implicit_self, ty) {
                continue;
            }
            match (body, names) {
                (Some(b), _) if i < b.params.len() => {
                    self.pat(&b.params[i].pat);
                    self.w(": ");
                }
                (_, Some(n)) if i < n.len() => {
                    let _ = write!(self.out, "{}: ", n[i]);
                }
                _ => {}
            }
            self.ty(ty);
        }
        if sig.decl.c_variadic {
            if !sig.decl.inputs.is_empty() {
                self.w(", ");
            }
            self.w("...");
        }
        self.w(")");
        self.fn_ret(&sig.decl.output);
        self.where_clause(g);
    }

    /// `self`, `mut self`, `&self`, `&'a mut self`: a receiver written
    /// without a type. Returns false for other first parameters.
    fn self_param(&mut self, kind: ImplicitSelfKind, ty: &Ty) -> bool {
        match (kind, &ty.kind) {
            (ImplicitSelfKind::Imm, _) => self.w("self"),
            (ImplicitSelfKind::Mut, _) => self.w("mut self"),
            (ImplicitSelfKind::RefImm | ImplicitSelfKind::RefMut, TyKind::Ref(origin, _)) => {
                self.w("&");
                if let OriginKind::Named(_) = origin.kind {
                    self.origin(origin);
                    self.w(" ");
                }
                self.w(if kind == ImplicitSelfKind::RefMut { "mut self" } else { "self" });
            }
            _ => return false,
        }
        true
    }

    fn fn_ret(&mut self, r: &FnRetTy) {
        if let FnRetTy::Return(t) = r {
            self.w(" -> ");
            self.ty(t);
        }
    }

    fn body_value(&mut self, id: BodyId) {
        let body = self.krate.body(id);
        self.expr(&body.value);
    }

    fn anon_const(&mut self, c: &AnonConst) {
        self.body_value(c.body);
    }

    // -----------------------------------------------------------------------
    // Generics
    // -----------------------------------------------------------------------

    fn generic_params(&mut self, params: &[GenericParam]) {
        if params.is_empty() {
            return;
        }
        self.w("<");
        self.comma_sep(params, |p, gp| p.generic_param(gp));
        self.w(">");
    }

    fn generic_param(&mut self, gp: &GenericParam) {
        match &gp.kind {
            GenericParamKind::Origin => {
                let _ = write!(self.out, "'{}", gp.name);
            }
            GenericParamKind::Type { default } => {
                self.w(gp.name.name.as_str());
                if let Some(d) = default {
                    self.w(" = ");
                    self.ty(d);
                }
            }
            GenericParamKind::Const { ty, default } => {
                let _ = write!(self.out, "const {}: ", gp.name);
                self.ty(ty);
                if let Some(d) = default {
                    self.w(" = ");
                    self.anon_const(d);
                }
            }
        }
    }

    fn where_clause(&mut self, g: &Generics) {
        if g.predicates.is_empty() {
            return;
        }
        self.w(" where ");
        self.comma_sep(&g.predicates, |p, pred| match pred {
            WherePredicate::Bound { bound_generic_params, bounded_ty, bounds, .. } => {
                if !bound_generic_params.is_empty() {
                    p.w("for");
                    p.generic_params(bound_generic_params);
                    p.w(" ");
                }
                p.ty(bounded_ty);
                p.w(": ");
                p.bounds(bounds);
            }
            WherePredicate::Origin { origin, bounds, .. } => {
                p.origin(origin);
                p.w(": ");
                for (i, b) in bounds.iter().enumerate() {
                    if i > 0 {
                        p.w(" + ");
                    }
                    p.origin(b);
                }
            }
        });
    }

    fn bounds(&mut self, bounds: &[GenericBound]) {
        for (i, b) in bounds.iter().enumerate() {
            if i > 0 {
                self.w(" + ");
            }
            match b {
                GenericBound::Trait(t) => self.poly_trait_ref(t),
                GenericBound::Outlives(o) => self.origin(o),
                GenericBound::Use(args, _) => {
                    self.w("use<");
                    self.comma_sep(args, |p, a| p.w(a.name.as_str()));
                    self.w(">");
                }
            }
        }
    }

    fn poly_trait_ref(&mut self, t: &PolyTraitRef) {
        if !t.bound_generic_params.is_empty() {
            self.w("for");
            self.generic_params(&t.bound_generic_params);
            self.w(" ");
        }
        if t.maybe {
            self.w("?");
        }
        self.path(&t.trait_ref.path);
    }

    fn origin(&mut self, o: &Origin) {
        match &o.kind {
            OriginKind::Elided => self.w("'_"),
            OriginKind::Named(names) if names.len() == 1 => {
                let _ = write!(self.out, "'{}", names[0].0);
            }
            OriginKind::Named(names) => {
                self.w("'[");
                self.comma_sep(names, |p, (n, _)| p.w(n.name.as_str()));
                self.w("]");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    fn ty(&mut self, t: &Ty) {
        match &t.kind {
            TyKind::Slice(inner) => {
                self.w("[");
                self.ty(inner);
                self.w("]");
            }
            TyKind::Array(inner, len) => {
                self.w("[");
                self.ty(inner);
                self.w("; ");
                self.anon_const(len);
                self.w("]");
            }
            TyKind::Ptr(mt) => {
                self.w(if mt.mutbl.is_mut() { "*mut " } else { "*const " });
                self.ty(&mt.ty);
            }
            TyKind::Ref(origin, mt) => {
                self.w("&");
                if let OriginKind::Named(_) = origin.kind {
                    self.origin(origin);
                    self.w(" ");
                }
                if mt.mutbl.is_mut() {
                    self.w("mut ");
                }
                self.ty(&mt.ty);
            }
            TyKind::FnPtr(f) => {
                if !f.generic_params.is_empty() {
                    self.w("for");
                    self.generic_params(&f.generic_params);
                    self.w(" ");
                }
                self.safety(f.safety);
                if let Some(abi) = f.abi {
                    let _ = write!(self.out, "extern \"{abi}\" ");
                }
                self.w("fn(");
                for (i, ty) in f.decl.inputs.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    if let Some(Some(n)) = f.param_idents.get(i) {
                        let _ = write!(self.out, "{n}: ");
                    }
                    self.ty(ty);
                }
                if f.decl.c_variadic {
                    self.w(", ...");
                }
                self.w(")");
                self.fn_ret(&f.decl.output);
            }
            TyKind::FuncValue(f) => {
                self.w("func(");
                self.comma_sep(&f.decl.inputs, |p, t| p.ty(t));
                self.w(")");
                self.fn_ret(&f.decl.output);
            }
            TyKind::Never => self.w("!"),
            TyKind::Tup(tys) => {
                self.w("(");
                self.comma_sep(tys, |p, t| p.ty(t));
                if tys.len() == 1 {
                    self.w(",");
                }
                self.w(")");
            }
            TyKind::Path(q) => self.qpath(q, false),
            TyKind::TraitObject(traits, origin) => {
                self.w("dyn ");
                for (i, t) in traits.iter().enumerate() {
                    if i > 0 {
                        self.w(" + ");
                    }
                    self.poly_trait_ref(t);
                }
                if let Some(o) = origin {
                    self.w(" + ");
                    self.origin(o);
                }
            }
            TyKind::OpaqueDef(bounds) => {
                self.w("impl ");
                self.bounds(bounds);
            }
            TyKind::Infer => self.w("_"),
            TyKind::Err => self.w("/*error*/"),
        }
    }

    // -----------------------------------------------------------------------
    // Paths
    // -----------------------------------------------------------------------

    fn path(&mut self, p: &Path) {
        self.segments(&p.segments, false);
    }

    fn segments(&mut self, segs: &[PathSegment], in_expr: bool) {
        for (i, s) in segs.iter().enumerate() {
            if i > 0 {
                self.w("::");
            }
            self.segment(s, in_expr);
        }
    }

    fn segment(&mut self, s: &PathSegment, in_expr: bool) {
        if s.ident.name != kw::Empty {
            self.w(s.ident.name.as_str());
        }
        if let Some(args) = &s.args {
            self.generic_args(args, in_expr);
        }
    }

    fn generic_args(&mut self, a: &GenericArgs, in_expr: bool) {
        if a.parenthesized {
            // `Fn<(A, B), Output = C>` prints as `Fn(A, B) -> C`.
            self.w("(");
            if let Some(GenericArg::Type(Ty { kind: TyKind::Tup(inputs), .. })) = a.args.first() {
                self.comma_sep(inputs, |p, t| p.ty(t));
            }
            self.w(")");
            if let Some(AssocItemConstraint { kind: AssocItemConstraintKind::Equality(out), .. }) = a.constraints.first() {
                if !matches!(&out.kind, TyKind::Tup(v) if v.is_empty()) {
                    self.w(" -> ");
                    self.ty(out);
                }
            }
            return;
        }
        if a.args.is_empty() && a.constraints.is_empty() {
            return;
        }
        if in_expr {
            self.w("::");
        }
        self.w("<");
        let mut first = true;
        for arg in &a.args {
            if !first {
                self.w(", ");
            }
            first = false;
            match arg {
                GenericArg::Origin(o) => self.origin(o),
                GenericArg::Type(t) => self.ty(t),
                GenericArg::Const(c) => match &c.kind {
                    ConstArgKind::Path(q) => self.qpath(q, false),
                    ConstArgKind::Anon(a) => {
                        self.w("{ ");
                        self.anon_const(a);
                        self.w(" }");
                    }
                },
                GenericArg::Infer(_) => self.w("_"),
            }
        }
        for c in &a.constraints {
            if !first {
                self.w(", ");
            }
            first = false;
            let _ = write!(self.out, "{}", c.ident);
            match &c.kind {
                AssocItemConstraintKind::Equality(t) => {
                    self.w(" = ");
                    self.ty(t);
                }
                AssocItemConstraintKind::Bound(b) => {
                    self.w(": ");
                    self.bounds(b);
                }
            }
        }
        self.w(">");
    }

    fn qpath(&mut self, q: &QPath, in_expr: bool) {
        match q {
            QPath::Resolved(None, p) => self.segments(&p.segments, in_expr),
            QPath::Resolved(Some(qself), p) => {
                // `<T as Trait>::Assoc`: the last segment is the item, the
                // ones before it the trait.
                self.w("<");
                self.ty(qself);
                self.w(" as ");
                let n = p.segments.len();
                self.segments(&p.segments[..n - 1], false);
                self.w(">::");
                self.segment(&p.segments[n - 1], in_expr);
            }
            QPath::TypeRelative(ty, seg) => {
                // A bare path type prints as `T::Assoc`; anything else needs
                // `<…>`.
                match &ty.kind {
                    TyKind::Path(inner) => self.qpath(inner, false),
                    _ => {
                        self.w("<");
                        self.ty(ty);
                        self.w(">");
                    }
                }
                self.w("::");
                self.segment(seg, in_expr);
            }
            QPath::LangItem(item, _) => {
                let _ = write!(self.out, "#[lang = \"{}\"]", item.name());
            }
        }
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    fn pat(&mut self, p: &Pat) {
        match &p.kind {
            PatKind::Wild => self.w("_"),
            PatKind::Binding(BindingMode(by_ref, mutbl), _, ident, sub) => {
                match by_ref {
                    ByRef::Yes(Mutability::Mut) => self.w("ref mut "),
                    ByRef::Yes(Mutability::Not) => self.w("ref "),
                    ByRef::No if mutbl.is_mut() => self.w("mut "),
                    ByRef::No => {}
                }
                self.w(ident.name.as_str());
                if let Some(s) = sub {
                    self.w(" @ ");
                    self.pat(s);
                }
            }
            PatKind::Struct(q, fields, rest) => {
                self.qpath(q, false);
                self.w(" { ");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    if !f.is_shorthand {
                        let _ = write!(self.out, "{}: ", f.ident);
                    }
                    self.pat(&f.pat);
                }
                if *rest {
                    if !fields.is_empty() {
                        self.w(", ");
                    }
                    self.w("..");
                }
                self.w(" }");
            }
            PatKind::TupleStruct(q, pats, rest) => {
                self.qpath(q, false);
                self.tuple_pats(pats, *rest);
            }
            PatKind::Or(alts) => {
                for (i, a) in alts.iter().enumerate() {
                    if i > 0 {
                        self.w(" | ");
                    }
                    self.pat(a);
                }
            }
            PatKind::Path(q) => self.qpath(q, false),
            PatKind::Tuple(pats, rest) => {
                if pats.len() == 1 && rest.is_none() {
                    self.w("(");
                    self.pat(&pats[0]);
                    self.w(",)");
                } else {
                    self.tuple_pats(pats, *rest);
                }
            }
            PatKind::Ref(inner, m) => {
                self.w(if m.is_mut() { "&mut " } else { "&" });
                self.pat(inner);
            }
            PatKind::Expr(e) => self.expr(e),
            PatKind::Range(lo, hi, end) => {
                if let Some(lo) = lo {
                    self.expr(lo);
                }
                self.w(if *end == RangeEnd::Included { "..=" } else { ".." });
                if let Some(hi) = hi {
                    self.expr(hi);
                }
            }
            PatKind::Slice(before, slice, after) => {
                self.w("[");
                let mut first = true;
                for p in before {
                    if !first {
                        self.w(", ");
                    }
                    first = false;
                    self.pat(p);
                }
                if let Some(s) = slice {
                    if !first {
                        self.w(", ");
                    }
                    first = false;
                    match s.kind {
                        PatKind::Wild => {}
                        _ => {
                            self.pat(s);
                            self.w(" @ ");
                        }
                    }
                    self.w("..");
                }
                for p in after {
                    if !first {
                        self.w(", ");
                    }
                    first = false;
                    self.pat(p);
                }
                self.w("]");
            }
            PatKind::Err => self.w("/*error*/"),
        }
    }

    fn tuple_pats(&mut self, pats: &[Pat], rest: Option<usize>) {
        self.w("(");
        let mut first = true;
        for (i, p) in pats.iter().enumerate() {
            if rest == Some(i) {
                if !first {
                    self.w(", ");
                }
                first = false;
                self.w("..");
            }
            if !first {
                self.w(", ");
            }
            first = false;
            self.pat(p);
        }
        if rest == Some(pats.len()) {
            if !first {
                self.w(", ");
            }
            self.w("..");
        }
        self.w(")");
    }

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------

    /// An operand that binds at least as tightly as `min`.
    fn expr_prec(&mut self, e: &Expr, min: u8) {
        if expr_prec(e) < min {
            self.w("(");
            self.expr(e);
            self.w(")");
        } else {
            self.expr(e);
        }
    }

    fn label(&mut self, l: &Option<Label>) {
        if let Some(l) = l {
            let _ = write!(self.out, "'{}: ", l.ident.as_str().trim_start_matches('\''));
        }
    }

    fn expr(&mut self, e: &Expr) {
        self.attrs(e.hir_id);
        match &e.kind {
            ExprKind::ConstBlock(c) => {
                self.w("const ");
                self.body_value(c.body);
            }
            ExprKind::Array(es) => {
                self.w("[");
                self.comma_sep(es, |p, e| p.expr(e));
                self.w("]");
            }
            ExprKind::Call(f, args) => {
                self.expr_prec(f, PREC_POSTFIX);
                self.w("(");
                self.comma_sep(args, |p, e| p.expr(e));
                self.w(")");
            }
            ExprKind::MethodCall(seg, recv, args, _) => {
                self.expr_prec(recv, PREC_POSTFIX);
                self.w(".");
                self.segment(seg, true);
                self.w("(");
                self.comma_sep(args, |p, e| p.expr(e));
                self.w(")");
            }
            ExprKind::Tup(es) => {
                self.w("(");
                self.comma_sep(es, |p, e| p.expr(e));
                if es.len() == 1 {
                    self.w(",");
                }
                self.w(")");
            }
            ExprKind::Binary(op, a, b) => {
                let p = prec(op.node);
                self.expr_prec(a, p);
                let _ = write!(self.out, " {} ", op.node.as_str());
                self.expr_prec(b, p + 1);
            }
            ExprKind::Unary(op, x) => {
                self.w(match op {
                    UnOp::Deref => "*",
                    UnOp::Not => "!",
                    UnOp::Neg => "-",
                });
                self.expr_prec(x, PREC_PREFIX);
            }
            ExprKind::Lit(lit) => {
                let _ = write!(self.out, "{lit}");
            }
            ExprKind::Cast(x, t) => {
                self.expr_prec(x, 11);
                self.w(" as ");
                self.ty(t);
            }
            ExprKind::DropTemps(x) => {
                self.w("drop_temps!(");
                self.expr(x);
                self.w(")");
            }
            ExprKind::Let(l) => {
                self.w("let ");
                self.pat(&l.pat);
                if let Some(t) = &l.ty {
                    self.w(": ");
                    self.ty(t);
                }
                self.w(" = ");
                self.expr_prec(&l.init, 3);
            }
            ExprKind::If(cond, then, els) => {
                self.w("if ");
                self.expr(cond);
                self.w(" ");
                self.expr(then);
                if let Some(els) = els {
                    self.w(" else ");
                    match els.kind {
                        ExprKind::Block(..) | ExprKind::If(..) => self.expr(els),
                        // A desugaring's `else break`.
                        _ => {
                            self.w("{ ");
                            self.expr(els);
                            self.w(" }");
                        }
                    }
                }
            }
            ExprKind::Loop(body, label, _, _) => {
                self.label(label);
                self.w("loop ");
                self.block(body);
            }
            ExprKind::Match(scrutinee, arms, _) => {
                self.w("match ");
                self.expr(scrutinee);
                self.w(" {");
                self.indent += 1;
                for arm in arms {
                    self.nl();
                    self.attrs(arm.hir_id);
                    self.pat(&arm.pat);
                    if let Some(g) = &arm.guard {
                        self.w(" if ");
                        self.expr(g);
                    }
                    self.w(" => ");
                    self.expr(&arm.body);
                    self.w(",");
                }
                self.indent -= 1;
                self.nl();
                self.w("}");
            }
            ExprKind::Closure(c) => {
                let body = self.krate.body(c.body);
                match c.kind {
                    ClosureKind::AsyncBlock => {
                        self.w("async ");
                        if c.capture_clause == CaptureBy::Value {
                            self.w("move ");
                        }
                        self.expr(&body.value);
                        return;
                    }
                    ClosureKind::AsyncClosure => self.w("async "),
                    ClosureKind::Closure => {}
                }
                if c.capture_clause == CaptureBy::Value {
                    self.w("move ");
                }
                self.w("|");
                for (i, param) in body.params.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    self.pat(&param.pat);
                    if let Some(t) = c.fn_decl.inputs.get(i) {
                        if !matches!(t.kind, TyKind::Infer) {
                            self.w(": ");
                            self.ty(t);
                        }
                    }
                }
                self.w("|");
                self.fn_ret(&c.fn_decl.output);
                self.w(" ");
                self.expr(&body.value);
            }
            ExprKind::Block(b, label) => {
                self.label(label);
                self.block(b);
            }
            ExprKind::Assign(l, r, _) => {
                self.expr_prec(l, PREC_LOWEST + 1);
                self.w(" = ");
                self.expr(r);
            }
            ExprKind::AssignOp(op, l, r) => {
                self.expr_prec(l, PREC_LOWEST + 1);
                let _ = write!(self.out, " {}= ", op.node.as_str());
                self.expr(r);
            }
            ExprKind::Field(x, ident) => {
                self.expr_prec(x, PREC_POSTFIX);
                let _ = write!(self.out, ".{ident}");
            }
            ExprKind::Index(x, i, _) => {
                self.expr_prec(x, PREC_POSTFIX);
                self.w("[");
                self.expr(i);
                self.w("]");
            }
            ExprKind::Path(q) => self.qpath(q, true),
            ExprKind::AddrOf(k, m, x) => {
                self.w("&");
                if *k == vsc_ast::BorrowKind::Raw {
                    self.w(if m.is_mut() { "raw mut " } else { "raw const " });
                } else if m.is_mut() {
                    self.w("mut ");
                }
                self.expr_prec(x, PREC_PREFIX);
            }
            ExprKind::Break(dest, value) => {
                self.w("break");
                if let Some(l) = dest.label {
                    let _ = write!(self.out, " '{}", l.ident.as_str().trim_start_matches('\''));
                }
                if let Some(v) = value {
                    self.w(" ");
                    self.expr(v);
                }
            }
            ExprKind::Continue(dest) => {
                self.w("continue");
                if let Some(l) = dest.label {
                    let _ = write!(self.out, " '{}", l.ident.as_str().trim_start_matches('\''));
                }
            }
            ExprKind::Ret(v) => {
                self.w("return");
                if let Some(v) = v {
                    self.w(" ");
                    self.expr(v);
                }
            }
            ExprKind::InlineAsm(asm) => self.inline_asm(asm),
            ExprKind::Struct(q, fields, tail) => {
                self.qpath(q, false);
                self.w(" { ");
                for (i, f) in fields.iter().enumerate() {
                    if i > 0 {
                        self.w(", ");
                    }
                    if f.is_shorthand {
                        self.expr(&f.expr);
                    } else {
                        let _ = write!(self.out, "{}: ", f.ident);
                        self.expr(&f.expr);
                    }
                }
                if let StructTailExpr::Base(b) = tail {
                    if !fields.is_empty() {
                        self.w(", ");
                    }
                    self.w("..");
                    self.expr(b);
                }
                self.w(" }");
            }
            ExprKind::Repeat(x, n) => {
                self.w("[");
                self.expr(x);
                self.w("; ");
                self.anon_const(n);
                self.w("]");
            }
            ExprKind::Await(x) => {
                self.expr_prec(x, PREC_POSTFIX);
                self.w(".await");
            }
            ExprKind::Err => self.w("/*error*/"),
        }
    }

    fn block(&mut self, b: &Block) {
        if b.rules == BlockCheckMode::Unsafe {
            self.w("unsafe ");
        }
        if b.stmts.is_empty() && b.expr.is_none() {
            self.w("{ }");
            return;
        }
        self.w("{");
        self.indent += 1;
        for s in &b.stmts {
            self.nl();
            self.attrs(s.hir_id);
            match &s.kind {
                StmtKind::Let(l) => {
                    self.attrs(l.hir_id);
                    self.w("let ");
                    self.pat(&l.pat);
                    if let Some(t) = &l.ty {
                        self.w(": ");
                        self.ty(t);
                    }
                    if let Some(init) = &l.init {
                        self.w(" = ");
                        self.expr(init);
                    }
                    if let Some(els) = &l.els {
                        self.w(" else ");
                        self.block(els);
                    }
                    self.w(";");
                }
                StmtKind::Item(id) => self.item(*id),
                StmtKind::Semi(e) => {
                    self.expr(e);
                    self.w(";");
                }
            }
        }
        if let Some(e) = &b.expr {
            self.nl();
            self.expr(e);
        }
        self.indent -= 1;
        self.nl();
        self.w("}");
    }

    fn inline_asm(&mut self, asm: &InlineAsm) {
        self.w(match asm.asm_macro {
            vsc_ast::AsmMacro::Asm => "asm!(",
            vsc_ast::AsmMacro::NakedAsm => "naked_asm!(",
            vsc_ast::AsmMacro::GlobalAsm => "global_asm!(",
        });
        let mut first = true;
        let mut sep = |p: &mut Self| {
            if !first {
                p.w(", ");
            }
            first = false;
        };
        for (lit, _) in &asm.templates {
            sep(self);
            let _ = write!(self.out, "{lit}");
        }
        for (op, _) in &asm.operands {
            sep(self);
            let reg = |p: &mut Self, reg: vsc_span::Symbol, explicit: bool| {
                if explicit {
                    let _ = write!(p.out, "(\"{reg}\") ");
                } else {
                    let _ = write!(p.out, "({reg}) ");
                }
            };
            match op {
                InlineAsmOperand::In { reg: r, explicit, expr } => {
                    self.w("in");
                    reg(self, *r, *explicit);
                    self.expr(expr);
                }
                InlineAsmOperand::Out { reg: r, explicit, late, expr } => {
                    self.w(if *late { "lateout" } else { "out" });
                    reg(self, *r, *explicit);
                    match expr {
                        Some(e) => self.expr(e),
                        None => self.w("_"),
                    }
                }
                InlineAsmOperand::InOut { reg: r, explicit, late, expr } => {
                    self.w(if *late { "inlateout" } else { "inout" });
                    reg(self, *r, *explicit);
                    self.expr(expr);
                }
                InlineAsmOperand::SplitInOut { reg: r, explicit, late, in_expr, out_expr } => {
                    self.w(if *late { "inlateout" } else { "inout" });
                    reg(self, *r, *explicit);
                    self.expr(in_expr);
                    self.w(" => ");
                    match out_expr {
                        Some(e) => self.expr(e),
                        None => self.w("_"),
                    }
                }
                InlineAsmOperand::Const { anon_const } => {
                    self.w("const ");
                    self.anon_const(anon_const);
                }
                InlineAsmOperand::SymFn { expr } => {
                    self.w("sym ");
                    self.expr(expr);
                }
                InlineAsmOperand::Label { block } => {
                    self.w("label ");
                    self.block(block);
                }
            }
        }
        for (abi, _) in &asm.clobber_abis {
            sep(self);
            let _ = write!(self.out, "clobber_abi(\"{abi}\")");
        }
        if !asm.options.is_empty() {
            sep(self);
            self.w("options(");
            let opts: Vec<&str> = asm.options.iter().map(|(o, _)| o.as_str()).collect();
            self.w(&opts.join(", "));
            self.w(")");
        }
        self.w(")");
    }
}

