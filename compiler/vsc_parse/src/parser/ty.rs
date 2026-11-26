//! Types.

use vsc_ast::token::{Delimiter, TokenKind};
use vsc_ast::{
    AnonConst, DUMMY_NODE_ID, Extern, FnDecl, FnPtrTy, FnRetTy, FuncValueTy, GenericBound, GenericParam, MutTy,
    Mutability, Lifetime, P, Param, Pat, PatKind, Safety, Ty, TyKind,
};
use vsc_errors::Applicability;
use vsc_span::{Ident, Span, kw, sym};

use super::path::PathStyle;
use super::{PResult, Parser};

impl<'a> Parser<'a> {
    /// A type, where `A + B` is allowed (`impl A + B`, `dyn A + B`).
    pub fn parse_ty(&mut self) -> PResult<'a, P<Ty>> {
        self.parse_ty_common(true)
    }

    /// A type where `+` would be ambiguous: after `&`, after `as`, in a
    /// function's return type inside another type.
    pub fn parse_ty_no_plus(&mut self) -> PResult<'a, P<Ty>> {
        self.parse_ty_common(false)
    }

    fn mk_ty(&self, kind: TyKind, span: Span) -> P<Ty> {
        P::new(Ty { id: DUMMY_NODE_ID, kind, span })
    }

    fn parse_ty_common(&mut self, allow_plus: bool) -> PResult<'a, P<Ty>> {
        let lo = self.token.span;
        let kind = if self.check(TokenKind::OpenParen) {
            return self.parse_ty_tuple_or_parens(allow_plus);
        } else if self.check(TokenKind::Bang) {
            let span = self.token.span;
            self.bump();
            self.dcx()
                .struct_span_err(span, "the never type is written `never` in Vertex")
                .with_span_suggestion(span, "use `never`", "never", Applicability::MachineApplicable)
                .emit();
            TyKind::Never
        } else if self.check(TokenKind::Star) {
            self.parse_ty_ptr()?
        } else if self.check(TokenKind::OpenBracket) {
            self.parse_array_or_slice_ty()?
        } else if self.check(TokenKind::And) || self.check(TokenKind::AndAnd) {
            self.parse_borrowed_ty()?
        } else if self.eat_keyword(kw::Underscore) {
            TyKind::Infer
        } else if self.check_keyword(kw::Func)
            || self.check_keyword(kw::Unsafe)
            || self.check_keyword(kw::Extern)
        {
            self.parse_fn_ty(Vec::new())?
        } else if self.check_keyword(kw::For) {
            let params = self.parse_for_lifetimes()?;
            if self.check_keyword(kw::Func) || self.check_keyword(kw::Unsafe) || self.check_keyword(kw::Extern) {
                self.parse_fn_ty(params)?
            } else {
                let bound = self.parse_generic_bound_after_for(lo, params)?;
                self.dcx()
                    .struct_span_err(lo.to(self.prev_token.span), "trait objects must be written with `dyn`")
                    .emit();
                TyKind::TraitObject(vec![bound])
            }
        } else if self.eat_keyword(kw::Dyn) {
            let bounds = if allow_plus { self.parse_generic_bounds()? } else { vec![self.parse_generic_bound()?] };
            TyKind::TraitObject(bounds)
        } else if self.eat_keyword(kw::Impl) {
            let bounds = if allow_plus { self.parse_generic_bounds()? } else { vec![self.parse_generic_bound()?] };
            TyKind::ImplTrait(DUMMY_NODE_ID, bounds)
        } else if self.check(TokenKind::Lt) || self.check(TokenKind::Shl) {
            let (qself, path) = self.parse_qpath(PathStyle::Type)?;
            TyKind::Path(Some(qself), path)
        } else if self.token.is_keyword(sym::never) && self.look_ahead(1).kind != TokenKind::Dot {
            self.bump();
            TyKind::Never
        } else if self.check_path_segment_ident() {
            let path = self.parse_path(PathStyle::Type)?;
            if allow_plus && self.check(TokenKind::Plus) && !self.at_line_break() {
                self.dcx()
                    .struct_span_err(lo.to(self.prev_token.span), "trait objects must be written with `dyn`")
                    .with_span_suggestion(lo.shrink_to_lo(), "add `dyn`", "dyn ", Applicability::MachineApplicable)
                    .emit();
                let first = self.trait_bound_from_path(path, lo);
                self.bump();
                let mut bounds = vec![first];
                bounds.extend(self.parse_generic_bounds()?);
                TyKind::TraitObject(bounds)
            } else {
                TyKind::Path(None, path)
            }
        } else if let Some(label) = self.token.lifetime() {
            self.bump();
            self.dcx()
                .struct_span_err(label.span, "lifetimes have no `'` in Vertex")
                .with_help("write `&a T` instead of `&'a T` (exception 13)")
                .emit();
            TyKind::Err
        } else if self.check(TokenKind::DotDotDot) {
            self.bump();
            TyKind::CVarArgs
        } else {
            return Err(self.expected_found("type"));
        };
        Ok(self.mk_ty(kind, lo.to(self.prev_token.span)))
    }

    fn trait_bound_from_path(&self, path: vsc_ast::Path, lo: Span) -> GenericBound {
        GenericBound::Trait(vsc_ast::PolyTraitRef {
            bound_generic_params: vec![],
            modifiers: Default::default(),
            span: lo.to(path.span),
            trait_ref: vsc_ast::TraitRef { path, ref_id: DUMMY_NODE_ID },
            parens: false,
        })
    }

    fn parse_ty_tuple_or_parens(&mut self, allow_plus: bool) -> PResult<'a, P<Ty>> {
        let lo = self.token.span;
        let (mut tys, trailing) = self.parse_delim_comma_seq(Delimiter::Parenthesis, |p| p.parse_ty())?;
        let span = lo.to(self.prev_token.span);
        let kind = if tys.len() == 1 && !trailing {
            let inner = tys.pop().unwrap();
            if allow_plus && self.check(TokenKind::Plus) && !self.at_line_break() {
                // `(Trait) + Other` without `dyn`.
                self.dcx().struct_span_err(span, "trait objects must be written with `dyn`").emit();
            }
            TyKind::Paren(inner)
        } else {
            TyKind::Tup(tys)
        };
        Ok(self.mk_ty(kind, span))
    }

    fn parse_ty_ptr(&mut self) -> PResult<'a, TyKind> {
        self.bump(); // `*`
        let mutbl = if self.eat_keyword(kw::Mut) {
            Mutability::Mut
        } else if self.eat_keyword(kw::Const) {
            Mutability::Not
        } else {
            let span = self.prev_token.span;
            self.dcx()
                .struct_span_err(span, "expected `mut` or `const` keyword in raw pointer type")
                .with_span_suggestion(
                    span.shrink_to_hi(),
                    "add `mut` or `const` here",
                    "const ",
                    Applicability::HasPlaceholders,
                )
                .emit();
            Mutability::Not
        };
        let ty = self.parse_ty_no_plus()?;
        Ok(TyKind::Ptr(MutTy { ty, mutbl }))
    }

    fn parse_array_or_slice_ty(&mut self) -> PResult<'a, TyKind> {
        self.bump(); // `[`
        self.with_nl(false, |this| -> PResult<'a, _> {
            let elt = this.parse_ty()?;
            let kind = if this.eat(TokenKind::Semi) {
                let value = this.parse_expr()?;
                TyKind::Array(elt, AnonConst { id: DUMMY_NODE_ID, value })
            } else {
                TyKind::Slice(elt)
            };
            this.expect(TokenKind::CloseBracket)?;
            Ok(kind)
        })
    }

    /// `&T`, `&mut T`, `&a T`, `&a mut T`, `&static T`, `&_ T` (exception 13).
    /// `&&T` is two references.
    fn parse_borrowed_ty(&mut self) -> PResult<'a, TyKind> {
        if self.check(TokenKind::AndAnd) {
            self.break_and_eat(TokenKind::And);
            let inner_lo = self.token.span;
            let inner = self.parse_borrowed_ty()?;
            let inner = self.mk_ty(inner, inner_lo.to(self.prev_token.span));
            return Ok(TyKind::Ref(None, MutTy { ty: inner, mutbl: Mutability::Not }));
        }
        self.bump(); // `&`
        let lifetime = if let Some(label) = self.token.lifetime() {
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
                Some(Lifetime { ident: Ident::new(vsc_span::Symbol::intern(name), label.span) })
            }
        } else if self.is_lifetime_after_ampersand() {
            if self.eat_keyword(kw::Underscore) {
                None
            } else if self.eat_keyword(kw::Static) {
                Some(Lifetime { ident: Ident::new(kw::Static, self.prev_token.span) })
            } else {
                Some(Lifetime { ident: self.parse_ident()? })
            }
        } else {
            None
        };
        let mutbl = if self.eat_keyword(kw::Mut) { Mutability::Mut } else { Mutability::Not };
        let ty = self.parse_ty_no_plus()?;
        Ok(TyKind::Ref(lifetime, MutTy { ty, mutbl }))
    }

    /// After `&`, is the current token a quote-free lifetime (`&a T`,
    /// `&a mut T`, `&static T`, `&_ T`) rather than the referenced type?
    pub(super) fn is_lifetime_after_ampersand(&self) -> bool {
        let is_lifetime_tok = self.token.is_keyword(kw::Static)
            || self.token.is_keyword(kw::Underscore)
            || matches!(
                self.token.ident(),
                Some((ident, _))
                    if !ident.is_reserved()
                        && !ident.name.is_builtin_name()
                        && ident.name.as_str().starts_with(|c: char| c.is_ascii_lowercase())
            );
        if !is_lifetime_tok {
            return false;
        }
        let next = self.look_ahead(1);
        if next.line_break_before
            || matches!(next.kind, TokenKind::Dot | TokenKind::Lt | TokenKind::Shl | TokenKind::Colon)
        {
            return false;
        }
        next.is_keyword(kw::Mut) || next.can_begin_type()
    }

    /// `for<a>`
    pub fn parse_for_lifetimes(&mut self) -> PResult<'a, Vec<GenericParam>> {
        self.expect_keyword(kw::For)?;
        let generics = self.parse_generics()?;
        Ok(generics.params)
    }

    /// `func(T) -> U`, `unsafe extern "C" func(T) -> U`.
    fn parse_fn_ty(&mut self, generic_params: Vec<GenericParam>) -> PResult<'a, TyKind> {
        let lo = self.token.span;
        let safety = if self.eat_keyword(kw::Unsafe) { Safety::Unsafe(self.prev_token.span) } else { Safety::Default };
        let ext = if self.eat_keyword(kw::Extern) {
            let span = self.prev_token.span;
            match self.parse_str_lit() {
                Some(abi) => Extern::Explicit(abi, span.to(abi.span)),
                None => Extern::Implicit(span),
            }
        } else {
            Extern::None
        };
        if self.check_keyword(kw::Fn) {
            let span = self.token.span;
            self.dcx()
                .struct_span_err(span, "function types are written `func` in Vertex")
                .with_span_suggestion(span, "use `func`", "func", Applicability::MachineApplicable)
                .emit();
            self.bump();
        } else {
            self.expect_keyword(kw::Func)?;
        }
        let (inputs, _) = self.parse_delim_comma_seq(Delimiter::Parenthesis, |p| p.parse_maybe_named_param())?;
        let output = if self.eat(TokenKind::RArrow) {
            FnRetTy::Ty(self.parse_ty_no_plus()?)
        } else {
            FnRetTy::Default(self.prev_token.span.shrink_to_hi())
        };
        let decl_span = lo.to(self.prev_token.span);
        let decl = P::new(FnDecl { inputs, output });
        let is_value = matches!(safety, Safety::Default) && matches!(ext, Extern::None);
        Ok(if is_value {
            TyKind::FuncValue(P::new(FuncValueTy { generic_params, decl, decl_span }))
        } else {
            TyKind::FnPtr(P::new(FnPtrTy { safety, ext, generic_params, decl, decl_span }))
        })
    }

    /// A parameter of a function type: `T`, `name: T`, `_: T`, or `...`.
    fn parse_maybe_named_param(&mut self) -> PResult<'a, Param> {
        let attrs = self.parse_outer_attributes()?;
        let lo = self.token.span;
        let named = (self.check_ident() || self.check_keyword(kw::Underscore))
            && self.look_ahead(1).kind == TokenKind::Colon
            && self.look_ahead(2).kind != TokenKind::Colon;
        let pat = if named {
            let ident = self.parse_ident_or_underscore()?;
            self.bump(); // `:`
            let kind = if ident.name == kw::Underscore {
                PatKind::Wild
            } else {
                PatKind::Ident(vsc_ast::BindingMode::NONE, ident, None)
            };
            P::new(Pat { id: DUMMY_NODE_ID, kind, span: ident.span })
        } else {
            P::new(Pat { id: DUMMY_NODE_ID, kind: PatKind::Wild, span: lo.shrink_to_lo() })
        };
        let ty = self.parse_ty()?;
        Ok(Param {
            attrs,
            ty,
            pat,
            id: DUMMY_NODE_ID,
            span: lo.to(self.prev_token.span),
            flat_receiver: false,
        })
    }

    /// The type after `as`: no `+`, and no generic `<` ambiguity.
    pub fn parse_as_cast_ty(&mut self) -> PResult<'a, P<Ty>> {
        self.parse_ty_no_plus()
    }
}
