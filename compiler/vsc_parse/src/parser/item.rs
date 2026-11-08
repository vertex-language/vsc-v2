//! Items.

use vsc_ast::token::{Delimiter, TokenKind};
use vsc_ast::{
    AngleBracketedArg, AssocItem, AssocItemKind, AttrVec, BindingMode, ByRef, Const, ConstItem, DUMMY_NODE_ID, EnumDef,
    Extern, FieldDef, Fn, FnDecl, FnHeader, FnRetTy, FnSig, ForeignItem, ForeignItemKind, ForeignMod, GenericArg,
    GenericArgs, Generics, Impl, ImplOrigin, Item, ItemKind, MutTy, Mutability, P, Param, Pat, PatKind, Safety,
    StaticItem, Trait, TraitRef, Ty, TyAlias, TyKind, Variant, VariantData, Visibility, VisibilityKind,
};
use vsc_errors::Applicability;
use vsc_span::{Ident, Span, kw, sym};

use super::{PResult, Parser};

impl<'a> Parser<'a> {
    // -----------------------------------------------------------------------
    // Recognizing items
    // -----------------------------------------------------------------------

    /// Does an item start here? Used where statements and items mix.
    pub fn is_item_start(&self) -> bool {
        let t = &self.token;
        let next = self.look_ahead(1);
        if t.is_keyword(kw::Public) || t.is_keyword(kw::Package) || t.is_keyword(kw::Pub) {
            return true;
        }
        if [kw::Struct, kw::Class, kw::Enum, kw::Trait, kw::Implement, kw::Static, kw::Extern]
            .iter()
            .any(|&k| t.is_keyword(k))
        {
            return true;
        }
        if t.is_keyword(kw::Type) && next.is_non_keyword_ident() {
            return true;
        }
        if t.is_keyword(kw::Union) && next.is_non_keyword_ident() {
            return true;
        }
        if t.is_keyword(kw::Unsafe)
            && [kw::Trait, kw::Implement, kw::Extern, kw::Impl].iter().any(|&k| next.is_keyword(k))
        {
            return true;
        }
        if t.is_keyword(kw::Const)
            && (next.is_non_keyword_ident() || next.is_keyword(kw::Underscore))
            && self.look_ahead(2).kind == TokenKind::Colon
        {
            return true;
        }
        if self.check_fn_front_matter() {
            return true;
        }
        if t.is_keyword(sym::globalAsm) && next.kind == TokenKind::OpenParen {
            return true;
        }
        // Rust spellings, reported when the item is parsed.
        if t.is_keyword(kw::Fn) && next.is_non_keyword_ident() {
            return true;
        }
        if t.is_keyword(kw::Impl) && (next.kind == TokenKind::Lt || next.is_non_keyword_ident()) {
            return true;
        }
        t.is_keyword(kw::Mod) || t.is_keyword(kw::Use)
    }

    /// `const? async? (unsafe|safe)? (extern "abi"?)? func Name` or
    /// `… func (receiver) Name`.
    pub fn check_fn_front_matter(&self) -> bool {
        let mut i = 0;
        if self.look_ahead(i).is_keyword(kw::Const) {
            i += 1;
        }
        if self.look_ahead(i).is_keyword(kw::Async) {
            i += 1;
        }
        if self.look_ahead(i).is_keyword(kw::Unsafe) || self.look_ahead(i).is_keyword(kw::Safe) {
            i += 1;
        }
        if self.look_ahead(i).is_keyword(kw::Extern) {
            i += 1;
            if matches!(self.look_ahead(i).kind, TokenKind::Literal(..)) {
                i += 1;
            }
        }
        if !(self.look_ahead(i).is_keyword(kw::Func) || self.look_ahead(i).is_keyword(kw::Fn)) {
            return false;
        }
        let name = self.look_ahead(i + 1);
        if name.is_non_keyword_ident() {
            return true;
        }
        // A flat receiver: `func (t: &Task) Name`.
        if name.kind == TokenKind::OpenParen {
            let open = self.pos + i + 1;
            if let Some(close) = self.buf.partner(open) {
                return self.buf.tokens.get(close + 1).is_some_and(|t| t.is_non_keyword_ident());
            }
        }
        false
    }

    // -----------------------------------------------------------------------
    // Item lists
    // -----------------------------------------------------------------------

    /// Items separated by line breaks or `;`, up to `}` (if `in_braces`) or
    /// the end of the file.
    pub fn parse_item_list(&mut self, in_braces: bool) -> Vec<P<Item>> {
        let mut items = Vec::new();
        loop {
            while self.eat(TokenKind::Semi) {}
            if self.check(TokenKind::Eof) || (in_braces && self.check(TokenKind::CloseBrace)) {
                break;
            }
            let before = self.pos;
            if self.check(TokenKind::CloseBrace) {
                self.dcx().span_err(self.token.span, "unexpected closing delimiter: `}`");
                self.bump();
                continue;
            }
            if self.check_keyword(kw::Import) {
                let span = self.token.span;
                match self.parse_import_decl() {
                    Ok(_) => {
                        self.dcx()
                            .struct_span_err(span.to(self.prev_token.span), "imports must come before the file's items")
                            .with_note("Go's rule: a file is `package`, then imports, then everything else")
                            .emit();
                    }
                    Err(err) => {
                        err.emit();
                        self.recover_item();
                    }
                }
                continue;
            }
            match self.parse_item_in_list() {
                Ok(item) => {
                    items.push(item);
                    if let Err(err) = self.expect_terminator("items") {
                        err.emit();
                        self.recover_item();
                    }
                }
                Err(err) => {
                    err.emit();
                    self.recover_item();
                }
            }
            if self.pos == before {
                self.bump();
            }
        }
        items
    }

    fn parse_item_in_list(&mut self) -> PResult<'a, P<Item>> {
        let attrs = self.parse_outer_attributes()?;
        if !self.is_item_start() {
            let mut err = self.expected_found("item");
            if self.check_keyword(kw::Let) || self.check_keyword(kw::Var) || self.token.can_begin_expr() {
                err.note("statements and expressions go inside a function body");
            }
            return Err(err);
        }
        self.parse_item_common(attrs)
    }

    /// An item, after its attributes.
    pub fn parse_item_common(&mut self, attrs: AttrVec) -> PResult<'a, P<Item>> {
        let lo = attrs.first().map(|a| a.span).unwrap_or(self.token.span);
        let vis = self.parse_visibility()?;
        let (kind, attrs, vis) = self.parse_item_kind(attrs, vis, lo)?;
        Ok(P::new(Item { attrs, id: DUMMY_NODE_ID, span: lo.to(self.prev_token.span), vis, kind }))
    }

    /// `public`, `package`, or nothing (exception 6).
    pub fn parse_visibility(&mut self) -> PResult<'a, Visibility> {
        let lo = self.token.span;
        if self.eat_keyword(kw::Public) {
            return Ok(Visibility { kind: VisibilityKind::Public, span: lo });
        }
        if self.eat_keyword(kw::Package) {
            return Ok(Visibility { kind: VisibilityKind::Package, span: lo });
        }
        if self.check_keyword(kw::Pub) {
            self.bump();
            let mut kind = VisibilityKind::Public;
            let mut span = lo;
            let mut replacement = "public";
            if self.check(TokenKind::OpenParen) && !self.token.line_break_before {
                let is_crate = self.look_ahead(1).is_keyword(kw::Crate);
                self.skip_group_or_token();
                span = lo.to(self.prev_token.span);
                if is_crate {
                    kind = VisibilityKind::Package;
                    replacement = "package";
                }
            }
            self.dcx()
                .struct_span_err(span, format!("visibility is written `{replacement}` in Vertex"))
                .with_span_suggestion(span, format!("use `{replacement}`"), replacement, Applicability::MachineApplicable)
                .emit();
            return Ok(Visibility { kind, span });
        }
        Ok(Visibility::inherited(lo.shrink_to_lo()))
    }

    /// The item after its attributes and visibility. A flat receiver becomes
    /// an impl whose one method keeps the attributes and visibility, so this
    /// hands back what the outer item should get.
    fn parse_item_kind(
        &mut self,
        attrs: AttrVec,
        vis: Visibility,
        lo: Span,
    ) -> PResult<'a, (ItemKind, AttrVec, Visibility)> {
        let inherited = Visibility::inherited(lo.shrink_to_lo());
        if self.check_fn_front_matter() {
            return match self.parse_fn(true)? {
                (f, None) => Ok((ItemKind::Fn(P::new(f)), attrs, vis)),
                (f, Some(receiver)) => {
                    let impl_ = self.flat_receiver_impl(f, receiver, attrs, vis, lo)?;
                    Ok((ItemKind::Impl(P::new(impl_)), AttrVec::new(), inherited))
                }
            };
        }
        let kind = if self.eat_keyword(kw::Struct) {
            let (ident, generics, data) = self.parse_struct_like(true)?;
            ItemKind::Struct(ident, generics, data)
        } else if self.eat_keyword(kw::Class) {
            let (ident, generics, data) = self.parse_struct_like(false)?;
            ItemKind::Class(ident, generics, data)
        } else if self.check_keyword(kw::Union) && self.look_ahead(1).is_non_keyword_ident() {
            self.bump();
            let (ident, generics, data) = self.parse_struct_like(false)?;
            ItemKind::Union(ident, generics, data)
        } else if self.eat_keyword(kw::Enum) {
            self.parse_item_enum()?
        } else if self.check_keyword(kw::Trait) || (self.check_keyword(kw::Unsafe) && self.look_ahead(1).is_keyword(kw::Trait)) {
            self.parse_item_trait()?
        } else if self.check_keyword(kw::Implement)
            || self.check_keyword(kw::Impl)
            || (self.check_keyword(kw::Unsafe)
                && (self.look_ahead(1).is_keyword(kw::Implement) || self.look_ahead(1).is_keyword(kw::Impl)))
        {
            ItemKind::Impl(P::new(self.parse_item_impl()?))
        } else if self.eat_keyword(kw::Type) {
            ItemKind::TyAlias(P::new(self.parse_type_alias()?))
        } else if self.eat_keyword(kw::Const) {
            ItemKind::Const(P::new(self.parse_const_item()?))
        } else if self.check_keyword(kw::Static) {
            ItemKind::Static(P::new(self.parse_static_item(Safety::Default)?))
        } else if self.check_keyword(kw::Extern) || (self.check_keyword(kw::Unsafe) && self.look_ahead(1).is_keyword(kw::Extern)) {
            ItemKind::ForeignMod(self.parse_foreign_mod()?)
        } else if self.token.is_keyword(sym::globalAsm) {
            ItemKind::GlobalAsm(self.parse_asm()?)
        } else if self.check_keyword(kw::Mod) {
            let mut err = self.expected_found("item");
            err.help("a Vertex package is a folder: put the code in a subfolder and `import` it (exception 1)");
            return Err(err);
        } else if self.check_keyword(kw::Use) {
            let mut err = self.expected_found("item");
            err.help("Vertex imports with `import \"path\"` at the top of the file (exception 1)");
            return Err(err);
        } else {
            return Err(self.expected_found("item"));
        };
        Ok((kind, attrs, vis))
    }

    // -----------------------------------------------------------------------
    // Functions
    // -----------------------------------------------------------------------

    fn parse_fn_header(&mut self) -> FnHeader {
        let mut header = FnHeader::default();
        if self.eat_keyword(kw::Const) {
            header.constness = Const::Yes(self.prev_token.span);
        }
        if self.eat_keyword(kw::Async) {
            header.asyncness = Some(self.prev_token.span);
        }
        if self.eat_keyword(kw::Unsafe) {
            header.safety = Safety::Unsafe(self.prev_token.span);
        } else if self.eat_keyword(kw::Safe) {
            header.safety = Safety::Safe(self.prev_token.span);
        }
        if self.eat_keyword(kw::Extern) {
            let span = self.prev_token.span;
            header.ext = match self.parse_str_lit() {
                Some(abi) => Extern::Explicit(abi, span.to(abi.span)),
                None => Extern::Implicit(span),
            };
        }
        header
    }

    /// `func Name<G>(params) -> T where … { body }`, maybe with a flat receiver.
    /// Returns the function and its receiver, if it had one.
    pub fn parse_fn(&mut self, allow_receiver: bool) -> PResult<'a, (Fn, Option<Param>)> {
        let lo = self.token.span;
        let header = self.parse_fn_header();
        if self.check_keyword(kw::Fn) {
            let span = self.token.span;
            self.dcx()
                .struct_span_err(span, "functions are declared with `func` in Vertex")
                .with_span_suggestion(span, "use `func`", "func", Applicability::MachineApplicable)
                .emit();
            self.bump();
        } else {
            self.expect_keyword(kw::Func)?;
        }
        let receiver = if self.check(TokenKind::OpenParen) {
            let recv = self.parse_receiver()?;
            if !allow_receiver {
                self.dcx()
                    .struct_span_err(recv.span, "a method with a receiver can't be declared here")
                    .with_help("inside a trait or `implement` block, use `self`: `func Name(&self)`")
                    .emit();
            }
            Some(recv)
        } else {
            None
        };
        let ident = self.parse_ident()?;
        let mut generics = self.parse_generics()?;
        let decl = self.parse_fn_decl(receiver.is_none())?;
        generics.where_clause = self.parse_where_clause()?;
        let sig_span = lo.to(self.prev_token.span);
        let body = if self.check(TokenKind::OpenBrace) { Some(self.parse_block()?) } else { None };
        let f = Fn { ident, generics, sig: FnSig { header, decl, span: sig_span }, body };
        Ok((f, receiver.filter(|_| allow_receiver)))
    }

    /// `(name: Type)` before a flat method's name (exception 8).
    fn parse_receiver(&mut self) -> PResult<'a, Param> {
        let lo = self.token.span;
        self.bump(); // `(`
        let param = self.with_nl(false, |this| -> PResult<'a, Param> {
            let attrs = this.parse_outer_attributes()?;
            let ident = this.parse_ident()?;
            if !this.eat(TokenKind::Colon) {
                let mut err = this.expected_found("`:`");
                err.note("a receiver is written `(name: Type)`, as in `func (t: &Task) IsOpen()`");
                return Err(err);
            }
            let ty = this.parse_ty()?;
            this.expect(TokenKind::CloseParen)?;
            let pat = P::new(Pat { id: DUMMY_NODE_ID, kind: PatKind::Ident(BindingMode::NONE, ident, None), span: ident.span });
            Ok(Param { attrs, ty, pat, id: DUMMY_NODE_ID, span: lo.to(this.prev_token.span), flat_receiver: true })
        })?;
        Ok(param)
    }

    /// Wraps a flat method in a one-method inherent impl. The impl's own
    /// parameters are the bare names among the receiver type's arguments
    /// (Go's rule), which name resolution sorts into types and origins.
    fn flat_receiver_impl(&mut self, mut f: Fn, receiver: Param, attrs: AttrVec, vis: Visibility, lo: Span) -> PResult<'a, Impl> {
        let self_ty = match &receiver.ty.kind {
            TyKind::Ref(_, MutTy { ty, .. }) => ty.clone(),
            _ => receiver.ty.clone(),
        };
        let mut introduced_params = Vec::new();
        if let TyKind::Path(None, path) = &self_ty.kind {
            if let Some(args) = path.segments.last().and_then(|s| s.args.as_deref()) {
                if let GenericArgs::AngleBracketed(a) = args {
                    for arg in &a.args {
                        match arg {
                            AngleBracketedArg::Arg(GenericArg::Type(ty)) => match &ty.kind {
                                TyKind::Path(None, p) if p.as_ident().is_some() => {
                                    introduced_params.push(p.as_ident().unwrap())
                                }
                                TyKind::Slice(inner) => match &inner.kind {
                                    TyKind::Path(None, p) if p.as_ident().is_some() => {
                                        introduced_params.push(p.as_ident().unwrap())
                                    }
                                    _ => {}
                                },
                                _ => {}
                            },
                            AngleBracketedArg::Arg(GenericArg::Origin(o)) => introduced_params.extend(o.origins.iter().copied()),
                            _ => {}
                        }
                    }
                }
            }
        }
        f.sig.decl.inputs.insert(0, receiver);
        let span = lo.to(self.prev_token.span);
        let method = P::new(AssocItem { attrs, id: DUMMY_NODE_ID, span, vis, kind: AssocItemKind::Fn(P::new(f)) });
        Ok(Impl {
            generics: Generics { span: span.shrink_to_lo(), ..Default::default() },
            safety: Safety::Default,
            negative: false,
            of_trait: None,
            self_ty,
            items: vec![method],
            origin: ImplOrigin::FlatReceiver { introduced_params },
        })
    }

    /// `(params) -> T`
    fn parse_fn_decl(&mut self, allow_self: bool) -> PResult<'a, P<FnDecl>> {
        let mut first = true;
        let (inputs, _) = self.parse_delim_comma_seq(Delimiter::Parenthesis, |p| {
            let is_first = std::mem::replace(&mut first, false);
            p.parse_param(is_first && allow_self)
        })?;
        let output = if self.eat(TokenKind::RArrow) {
            FnRetTy::Ty(self.parse_ty()?)
        } else {
            FnRetTy::Default(self.prev_token.span.shrink_to_hi())
        };
        Ok(P::new(FnDecl { inputs, output }))
    }

    fn parse_param(&mut self, allow_self: bool) -> PResult<'a, Param> {
        let attrs = self.parse_outer_attributes()?;
        let lo = self.token.span;
        if allow_self {
            if let Some(mut param) = self.parse_self_param()? {
                param.attrs = attrs;
                return Ok(param);
            }
        }
        if self.eat(TokenKind::DotDotDot) {
            let span = self.prev_token.span;
            return Ok(Param {
                attrs,
                ty: P::new(Ty { id: DUMMY_NODE_ID, kind: TyKind::CVarArgs, span }),
                pat: P::new(Pat { id: DUMMY_NODE_ID, kind: PatKind::Wild, span }),
                id: DUMMY_NODE_ID,
                span,
                flat_receiver: false,
            });
        }
        let pat = self.parse_pat_no_top_alt()?;
        if !self.eat(TokenKind::Colon) {
            let mut err = self.expected_found("`:`");
            err.note("every parameter needs a type: `name: Type`");
            return Err(err);
        }
        let ty = if self.eat(TokenKind::DotDotDot) {
            P::new(Ty { id: DUMMY_NODE_ID, kind: TyKind::CVarArgs, span: self.prev_token.span })
        } else {
            self.parse_ty()?
        };
        Ok(Param { attrs, ty, pat, id: DUMMY_NODE_ID, span: lo.to(self.prev_token.span), flat_receiver: false })
    }

    /// `self`, `var self`, `&self`, `&mut self`, `&[a] self`, `&[a] mut self`,
    /// `self: T`, `var self: T`.
    fn parse_self_param(&mut self) -> PResult<'a, Option<Param>> {
        let lo = self.token.span;
        let is_self = |t: &vsc_ast::token::Token| t.is_keyword(kw::SelfLower);
        let mk_param = |this: &Self, mode: BindingMode, self_span: Span, ty_kind: TyKind| {
            let span = lo.to(this.prev_token.span);
            Param {
                attrs: AttrVec::new(),
                ty: P::new(Ty { id: DUMMY_NODE_ID, kind: ty_kind, span }),
                pat: P::new(Pat {
                    id: DUMMY_NODE_ID,
                    kind: PatKind::Ident(mode, Ident::new(kw::SelfLower, self_span), None),
                    span: self_span,
                }),
                id: DUMMY_NODE_ID,
                span,
                flat_receiver: false,
            }
        };

        // `self` and `var self`, maybe with `: T`.
        let mutable = if is_self(&self.token) {
            Some(Mutability::Not)
        } else if (self.check_keyword(kw::Var) || self.check_keyword(kw::Mut)) && is_self(self.look_ahead(1)) {
            if self.check_keyword(kw::Mut) {
                let span = self.token.span;
                self.dcx()
                    .struct_span_err(span, "a mutable `self` is written `var self` in Vertex")
                    .with_span_suggestion(span, "use `var`", "var", Applicability::MachineApplicable)
                    .emit();
            }
            self.bump();
            Some(Mutability::Mut)
        } else {
            None
        };
        if let Some(m) = mutable {
            let self_span = self.token.span;
            self.bump();
            let mode = BindingMode(ByRef::No, m);
            if self.eat(TokenKind::Colon) {
                let ty = self.parse_ty()?;
                let span = lo.to(self.prev_token.span);
                let mut p = mk_param(self, mode, self_span, TyKind::Err);
                p.ty = ty;
                p.span = span;
                return Ok(Some(p));
            }
            return Ok(Some(mk_param(self, mode, self_span, TyKind::ImplicitSelf)));
        }

        // `&self` and friends (`&a self`, `&a mut self`, `&static self`, `&_ self`, `&[a] self`).
        if self.check(TokenKind::And) {
            let n1 = self.look_ahead(1);
            let (has_bracket_origin, has_ident_origin, after_origin) = if n1.kind == TokenKind::OpenBracket {
                let open = self.pos + 1;
                match self.buf.partner(open) {
                    Some(close) => (true, false, close + 1 - self.pos),
                    None => (false, false, 1),
                }
            } else if n1.lifetime().is_some()
                || n1.is_keyword(kw::Static)
                || n1.is_keyword(kw::Underscore)
                || (n1.is_non_keyword_ident() && !n1.is_keyword(kw::Mut) && !is_self(n1))
            {
                (false, true, 2)
            } else {
                (false, false, 1)
            };
            let t = self.look_ahead(after_origin);
            let is_ref_self = is_self(t) || (t.is_keyword(kw::Mut) && is_self(self.look_ahead(after_origin + 1)));
            if !is_ref_self {
                return Ok(None);
            }
            self.bump(); // `&`
            let origin = if has_bracket_origin {
                Some(self.parse_origin_ref()?)
            } else if let Some(label) = self.token.lifetime() {
                self.bump();
                let name = label.name.as_str().trim_start_matches('\'');
                self.dcx()
                    .struct_span_err(label.span, "lifetimes have no `'` in Vertex")
                    .with_span_suggestion(
                        label.span,
                        "remove the leading `'`",
                        name.to_string(),
                        Applicability::MachineApplicable,
                    )
                    .emit();
                if name == "_" {
                    None
                } else {
                    Some(vsc_ast::OriginRef {
                        origins: vec![Ident::new(vsc_span::Symbol::intern(name), label.span)],
                        span: label.span,
                    })
                }
            } else if has_ident_origin {
                if self.eat_keyword(kw::Underscore) {
                    None
                } else if self.eat_keyword(kw::Static) {
                    let span = self.prev_token.span;
                    Some(vsc_ast::OriginRef { origins: vec![Ident::new(kw::Static, span)], span })
                } else {
                    let ident = self.parse_ident()?;
                    Some(vsc_ast::OriginRef { origins: vec![ident], span: ident.span })
                }
            } else {
                None
            };
            let mutbl = if self.eat_keyword(kw::Mut) { Mutability::Mut } else { Mutability::Not };
            let self_span = self.token.span;
            self.bump(); // `self`
            let implicit = P::new(Ty { id: DUMMY_NODE_ID, kind: TyKind::ImplicitSelf, span: self_span });
            return Ok(Some(mk_param(self, BindingMode::NONE, self_span, TyKind::Ref(origin, MutTy { ty: implicit, mutbl }))));
        }
        Ok(None)
    }

    // -----------------------------------------------------------------------
    // Structs, classes, unions, enums (exception 10: fields on their own lines)
    // -----------------------------------------------------------------------

    /// `Name<G> { fields }`, `Name<G>(fields)` (structs only) or `Name`
    /// (structs only).
    fn parse_struct_like(&mut self, is_struct: bool) -> PResult<'a, (Ident, Generics, VariantData)> {
        let keyword = self.prev_token.span;
        let ident = self.parse_ident()?;
        let mut generics = self.parse_generics()?;
        if is_struct && self.check(TokenKind::OpenParen) {
            let fields = self.parse_tuple_fields()?;
            generics.where_clause = self.parse_where_clause()?;
            return Ok((ident, generics, VariantData::Tuple(fields, DUMMY_NODE_ID)));
        }
        generics.where_clause = self.parse_where_clause()?;
        if self.check(TokenKind::OpenBrace) {
            let fields = self.parse_record_fields()?;
            return Ok((ident, generics, VariantData::Struct { fields }));
        }
        if is_struct {
            return Ok((ident, generics, VariantData::Unit(DUMMY_NODE_ID)));
        }
        let mut err = self.expected_found("`{`");
        err.span_label(keyword, "fields are listed in braces");
        Err(err)
    }

    fn parse_tuple_fields(&mut self) -> PResult<'a, Vec<FieldDef>> {
        let (fields, _) = self.parse_delim_comma_seq(Delimiter::Parenthesis, |p| {
            let attrs = p.parse_outer_attributes()?;
            let lo = p.token.span;
            let vis = p.parse_visibility()?;
            let ty = p.parse_ty()?;
            Ok(FieldDef { attrs, id: DUMMY_NODE_ID, span: lo.to(p.prev_token.span), vis, ident: None, ty })
        })?;
        Ok(fields)
    }

    /// `{ a: T \n b: U }` or `{ a: T; b: U }`.
    fn parse_record_fields(&mut self) -> PResult<'a, Vec<FieldDef>> {
        self.parse_terminated_braced("fields", "struct fields", |p| {
            let attrs = p.parse_outer_attributes()?;
            let lo = p.token.span;
            let vis = p.parse_visibility()?;
            let ident = p.parse_ident()?;
            if !p.eat(TokenKind::Colon) {
                let mut err = p.expected_found("`:`");
                err.note("a field is written `name: Type`");
                return Err(err);
            }
            let ty = p.parse_ty()?;
            Ok(FieldDef { attrs, id: DUMMY_NODE_ID, span: lo.to(p.prev_token.span), vis, ident: Some(ident), ty })
        })
    }

    /// `{ x \n y \n z }`: things separated by terminators. A Rust-style
    /// comma is reported once and accepted.
    fn parse_terminated_braced<T>(
        &mut self,
        plural: &str,
        what: &str,
        mut f: impl FnMut(&mut Self) -> PResult<'a, T>,
    ) -> PResult<'a, Vec<T>> {
        if !self.check(TokenKind::OpenBrace) {
            return Err(self.expected_found("`{`"));
        }
        self.bump();
        let items = self.with_nl(true, |this| {
            let mut items = Vec::new();
            let mut reported_comma = false;
            loop {
                while this.eat(TokenKind::Semi) {}
                if this.check(TokenKind::CloseBrace) || this.check(TokenKind::Eof) {
                    break;
                }
                let before = this.pos;
                match f(this) {
                    Ok(item) => items.push(item),
                    Err(err) => {
                        err.emit();
                        this.recover_stmt();
                        if this.pos == before {
                            this.bump();
                        }
                        continue;
                    }
                }
                if this.check(TokenKind::Comma) {
                    if !reported_comma {
                        reported_comma = true;
                        let span = this.token.span;
                        let mut err = this
                            .dcx()
                            .struct_span_err(span, format!("{what} are separated by line breaks, not `,`"))
                            .with_note(format!("put {plural} on their own lines, or separate them with `;` on one line"));
                        this.comma_separator_suggestion(&mut err, span);
                        err.emit();
                    }
                    this.bump();
                    continue;
                }
                if let Err(err) = this.expect_terminator(plural) {
                    err.emit();
                    this.recover_stmt();
                }
            }
            items
        });
        self.expect(TokenKind::CloseBrace)?;
        Ok(items)
    }

    /// For a Rust-style `,` between fields, variants or arms: remove it at a
    /// line end, or turn it into `;` when the next one is on the same line.
    pub(crate) fn comma_separator_suggestion(&self, err: &mut vsc_errors::Diag<'a>, comma: Span) {
        let next = self.look_ahead(1);
        if next.line_break_before || next.kind == TokenKind::CloseBrace {
            err.span_suggestion(comma, "remove the comma", "", Applicability::MachineApplicable);
        } else {
            err.span_suggestion(comma, "use `;` to separate them on one line", ";", Applicability::MachineApplicable);
        }
    }

    fn parse_item_enum(&mut self) -> PResult<'a, ItemKind> {
        let ident = self.parse_ident()?;
        let mut generics = self.parse_generics()?;
        generics.where_clause = self.parse_where_clause()?;
        let variants = self.parse_terminated_braced("variants", "enum variants", |p| {
            let attrs = p.parse_outer_attributes()?;
            let lo = p.token.span;
            let vis = p.parse_visibility()?;
            let ident = p.parse_ident()?;
            let data = if p.check(TokenKind::OpenParen) && !p.token.line_break_before {
                VariantData::Tuple(p.parse_tuple_fields()?, DUMMY_NODE_ID)
            } else if p.check(TokenKind::OpenBrace) && !p.token.line_break_before {
                VariantData::Struct { fields: p.parse_record_fields()? }
            } else {
                VariantData::Unit(DUMMY_NODE_ID)
            };
            let disr_expr = if p.eat(TokenKind::Eq) { Some(p.parse_anon_const_expr()?) } else { None };
            Ok(Variant { attrs, id: DUMMY_NODE_ID, span: lo.to(p.prev_token.span), vis, ident, data, disr_expr })
        })?;
        Ok(ItemKind::Enum(ident, generics, EnumDef { variants }))
    }

    // -----------------------------------------------------------------------
    // Traits and implementations (exception 8)
    // -----------------------------------------------------------------------

    fn parse_item_trait(&mut self) -> PResult<'a, ItemKind> {
        let safety = if self.eat_keyword(kw::Unsafe) { Safety::Unsafe(self.prev_token.span) } else { Safety::Default };
        self.expect_keyword(kw::Trait)?;
        let ident = self.parse_ident()?;
        let mut generics = self.parse_generics()?;
        let bounds = if self.eat(TokenKind::Colon) { self.parse_generic_bounds()? } else { Vec::new() };
        generics.where_clause = self.parse_where_clause()?;
        let items = self.parse_assoc_items()?;
        Ok(ItemKind::Trait(P::new(Trait { safety, ident, generics, bounds, items })))
    }

    /// `implement<G> Type { … }` or `implement<G> Trait for Type { … }`.
    fn parse_item_impl(&mut self) -> PResult<'a, Impl> {
        let safety = if self.eat_keyword(kw::Unsafe) { Safety::Unsafe(self.prev_token.span) } else { Safety::Default };
        if self.check_keyword(kw::Impl) {
            let span = self.token.span;
            self.dcx()
                .struct_span_err(span, "implementations are written `implement` in Vertex")
                .with_span_suggestion(span, "use `implement`", "implement", Applicability::MachineApplicable)
                .emit();
            self.bump();
        } else {
            self.expect_keyword(kw::Implement)?;
        }
        let mut generics = if self.check(TokenKind::Lt) && !self.token.line_break_before {
            self.parse_generics()?
        } else {
            Generics { span: self.prev_token.span.shrink_to_hi(), ..Default::default() }
        };
        let negative = self.eat(TokenKind::Bang);
        let first = self.parse_ty_no_plus()?;
        let (of_trait, self_ty) = if self.eat_keyword(kw::For) {
            let trait_ref = match first.kind {
                TyKind::Path(None, path) => TraitRef { path, ref_id: DUMMY_NODE_ID },
                _ => {
                    return Err(self.struct_span_err(first.span, "expected a trait, found type"));
                }
            };
            (Some(trait_ref), self.parse_ty()?)
        } else {
            if negative {
                self.dcx().span_err(first.span, "inherent implementations can't be negative");
            }
            (None, first)
        };
        generics.where_clause = self.parse_where_clause()?;
        let items = self.parse_assoc_items()?;
        Ok(Impl { generics, safety, negative, of_trait, self_ty, items, origin: ImplOrigin::Block })
    }

    /// `{ items }` in a trait or `implement` block.
    fn parse_assoc_items(&mut self) -> PResult<'a, Vec<P<AssocItem>>> {
        self.parse_terminated_braced("items", "items", |p| {
            let attrs = p.parse_outer_attributes()?;
            let lo = attrs.first().map(|a| a.span).unwrap_or(p.token.span);
            let vis = p.parse_visibility()?;
            let kind = if p.check_fn_front_matter() {
                let (f, _) = p.parse_fn(false)?;
                AssocItemKind::Fn(P::new(f))
            } else if p.eat_keyword(kw::Type) {
                AssocItemKind::Type(P::new(p.parse_type_alias()?))
            } else if p.eat_keyword(kw::Const) {
                AssocItemKind::Const(P::new(p.parse_const_item()?))
            } else {
                return Err(p.expected_found("`func`, `type` or `const`"));
            };
            Ok(P::new(AssocItem { attrs, id: DUMMY_NODE_ID, span: lo.to(p.prev_token.span), vis, kind }))
        })
    }

    // -----------------------------------------------------------------------
    // Aliases, constants, statics, extern blocks
    // -----------------------------------------------------------------------

    /// After `type`: `Name<G>: Bounds where … = T`.
    fn parse_type_alias(&mut self) -> PResult<'a, TyAlias> {
        let ident = self.parse_ident()?;
        let mut generics = self.parse_generics()?;
        let bounds = if self.eat(TokenKind::Colon) { self.parse_generic_bounds()? } else { Vec::new() };
        generics.where_clause = self.parse_where_clause()?;
        let ty = if self.eat(TokenKind::Eq) { Some(self.parse_ty()?) } else { None };
        if ty.is_some() && self.check_keyword(kw::Where) {
            let wc = self.parse_where_clause()?;
            generics.where_clause.predicates.extend(wc.predicates);
        }
        Ok(TyAlias { ident, generics, bounds, ty })
    }

    /// After `const`: `NAME: T = expr`.
    fn parse_const_item(&mut self) -> PResult<'a, ConstItem> {
        let ident = self.parse_ident_or_underscore()?;
        if !self.eat(TokenKind::Colon) {
            let mut err = self.expected_found("`:`");
            err.note("constants need a type: `const Name: Type = value`");
            return Err(err);
        }
        let ty = self.parse_ty()?;
        let expr = if self.eat(TokenKind::Eq) { Some(self.parse_expr()?) } else { None };
        Ok(ConstItem { ident, ty, expr })
    }

    /// `static var? NAME: T = expr`.
    fn parse_static_item(&mut self, safety: Safety) -> PResult<'a, StaticItem> {
        self.expect_keyword(kw::Static)?;
        let mutability = if self.eat_keyword(kw::Var) {
            Mutability::Mut
        } else if self.check_keyword(kw::Mut) {
            let span = self.token.span;
            self.dcx()
                .struct_span_err(span, "a mutable static is written `static var` in Vertex")
                .with_span_suggestion(span, "use `var`", "var", Applicability::MachineApplicable)
                .emit();
            self.bump();
            Mutability::Mut
        } else {
            Mutability::Not
        };
        let ident = self.parse_ident()?;
        self.expect(TokenKind::Colon)?;
        let ty = self.parse_ty()?;
        let expr = if self.eat(TokenKind::Eq) { Some(self.parse_expr()?) } else { None };
        Ok(StaticItem { ident, ty, safety, mutability, expr })
    }

    /// `unsafe extern "C" { items }`.
    fn parse_foreign_mod(&mut self) -> PResult<'a, ForeignMod> {
        let lo = self.token.span;
        let safety = if self.eat_keyword(kw::Unsafe) { Safety::Unsafe(self.prev_token.span) } else { Safety::Default };
        self.expect_keyword(kw::Extern)?;
        let abi = self.parse_str_lit();
        if matches!(safety, Safety::Default) {
            self.dcx()
                .struct_span_err(lo.to(self.prev_token.span), "extern blocks must be unsafe")
                .with_span_suggestion(lo.shrink_to_lo(), "add `unsafe`", "unsafe ", Applicability::MachineApplicable)
                .emit();
        }
        let items = self.parse_terminated_braced("items", "items", |p| {
            let attrs = p.parse_outer_attributes()?;
            let lo = attrs.first().map(|a| a.span).unwrap_or(p.token.span);
            let vis = p.parse_visibility()?;
            let kind = if p.check_fn_front_matter() {
                let (f, _) = p.parse_fn(false)?;
                ForeignItemKind::Fn(P::new(f))
            } else if p.check_keyword(kw::Static)
                || ((p.check_keyword(kw::Safe) || p.check_keyword(kw::Unsafe)) && p.look_ahead(1).is_keyword(kw::Static))
            {
                let safety = if p.eat_keyword(kw::Safe) {
                    Safety::Safe(p.prev_token.span)
                } else if p.eat_keyword(kw::Unsafe) {
                    Safety::Unsafe(p.prev_token.span)
                } else {
                    Safety::Default
                };
                ForeignItemKind::Static(P::new(p.parse_static_item(safety)?))
            } else if p.eat_keyword(kw::Type) {
                let ident = p.parse_ident()?;
                ForeignItemKind::TyAlias(P::new(TyAlias {
                    ident,
                    generics: Generics::default(),
                    bounds: Vec::new(),
                    ty: None,
                }))
            } else {
                return Err(p.expected_found("`func`, `static` or `type`"));
            };
            Ok(P::new(ForeignItem { attrs, id: DUMMY_NODE_ID, span: lo.to(p.prev_token.span), vis, kind }))
        })?;
        Ok(ForeignMod { safety, abi, items })
    }
}
