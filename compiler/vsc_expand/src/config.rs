//! `@cfg` and `@cfgAttr` (rustc's `expand/config.rs`).
//!
//! The predicate language is Rust's: `name`, `name = "value"`, `all(…)`,
//! `any(…)`, `not(…)`, `true`, `false`. Configuration names keep Rust's
//! spelling (`target_os`); only the attribute names use Go casing.

use vsc_ast::mut_visit::MutVisitor;
use vsc_ast::token::{Delimiter, IdentIsRaw, Lit, LitKind, TokenKind};
use vsc_ast::tokenstream::TokenTree;
use vsc_ast::{AttrArgs, AttrKind, AttrVec, Attribute};
use vsc_errors::ErrorGuaranteed;
use vsc_session::ParseSess;
use vsc_session::config::Cfg;
use vsc_span::{Span, Symbol, kw, sym};

/// A parsed configuration predicate.
#[derive(Clone, Debug, PartialEq)]
pub enum CfgPred {
    Option(Symbol, Option<Symbol>, Span),
    All(Vec<CfgPred>),
    Any(Vec<CfgPred>),
    Not(Box<CfgPred>),
    Bool(bool),
}

impl CfgPred {
    pub fn eval(&self, cfg: &Cfg) -> bool {
        match self {
            CfgPred::Option(name, value, _) => cfg.contains(*name, *value),
            CfgPred::All(ps) => ps.iter().all(|p| p.eval(cfg)),
            CfgPred::Any(ps) => ps.iter().any(|p| p.eval(cfg)),
            CfgPred::Not(p) => !p.eval(cfg),
            CfgPred::Bool(b) => *b,
        }
    }
}

/// Parses one predicate from the start of `tts`. Returns it and how many
/// token trees it used.
pub fn parse_cfg_pred(psess: &ParseSess, tts: &[TokenTree]) -> Result<(CfgPred, usize), ErrorGuaranteed> {
    let err = |span: Span, msg: &str| Err(psess.dcx.span_err(span, msg.to_string()));
    let Some(TokenTree::Token(first)) = tts.first() else {
        let span = tts.first().map(|t| t.span()).unwrap_or(vsc_span::DUMMY_SP);
        return err(span, "expected a configuration predicate");
    };
    let TokenKind::Ident(name, IdentIsRaw::No) = first.kind else {
        return err(first.span, "expected a configuration predicate");
    };
    if name == kw::True || name == kw::False {
        return Ok((CfgPred::Bool(name == kw::True), 1));
    }
    // `all(…)`, `any(…)`, `not(…)`
    if let Some(TokenTree::Delimited(dspan, Delimiter::Parenthesis, inner)) = tts.get(1) {
        let list = parse_cfg_pred_list(psess, inner)?;
        let pred = if name == sym::all {
            CfgPred::All(list)
        } else if name == sym::any {
            CfgPred::Any(list)
        } else if name == sym::not {
            if list.len() != 1 {
                return err(dspan.entire(), "`not` takes exactly one predicate");
            }
            CfgPred::Not(Box::new(list.into_iter().next().unwrap()))
        } else {
            return err(first.span, &format!("invalid predicate `{name}`: expected `all`, `any` or `not`"));
        };
        return Ok((pred, 2));
    }
    // `name = "value"`
    if let Some(TokenTree::Token(eq)) = tts.get(1) {
        if eq.kind == TokenKind::Eq {
            return match tts.get(2) {
                Some(TokenTree::Token(t)) => match t.kind {
                    TokenKind::Literal(Lit { kind: LitKind::Str | LitKind::StrRaw(_), symbol, suffix: None }) => {
                        Ok((CfgPred::Option(name, Some(symbol), first.span.to(t.span)), 3))
                    }
                    _ => err(t.span, "expected a string literal"),
                },
                other => err(other.map(|t| t.span()).unwrap_or(eq.span), "expected a string literal"),
            };
        }
    }
    Ok((CfgPred::Option(name, None, first.span), 1))
}

fn parse_cfg_pred_list(psess: &ParseSess, tts: &[TokenTree]) -> Result<Vec<CfgPred>, ErrorGuaranteed> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tts.len() {
        let (pred, used) = parse_cfg_pred(psess, &tts[i..])?;
        out.push(pred);
        i += used;
        match tts.get(i) {
            None => break,
            Some(TokenTree::Token(t)) if t.kind == TokenKind::Comma => i += 1,
            Some(other) => return Err(psess.dcx.span_err(other.span(), "expected `,`")),
        }
    }
    Ok(out)
}

/// The predicate of `@cfg(pred)`, or `None` with an error reported.
fn cfg_attr_pred(psess: &ParseSess, attr: &Attribute) -> Option<CfgPred> {
    let AttrKind::Normal(n) = &attr.kind else { return None };
    let AttrArgs::Delimited(d) = &n.args else {
        psess.dcx.struct_span_err(attr.span, "malformed `cfg` attribute").with_help("write `@cfg(predicate)`").emit();
        return None;
    };
    match parse_cfg_pred(psess, &d.tokens) {
        Ok((pred, used)) if used == d.tokens.len() => Some(pred),
        Ok((_, used)) => {
            psess.dcx.span_err(d.tokens[used].span(), "`cfg` takes one predicate; combine them with `all` or `any`");
            None
        }
        Err(_) => None,
    }
}

/// Removes what `@cfg` turns off and expands `@cfgAttr`.
pub struct StripUnconfigured<'a> {
    pub psess: &'a ParseSess,
    pub cfg: &'a Cfg,
}

impl StripUnconfigured<'_> {
    /// Expands `@cfgAttr(pred, a, b)` into `@a @b` where `pred` holds.
    fn expand_cfg_attrs(&self, attrs: &mut AttrVec) {
        let mut out = AttrVec::with_capacity(attrs.len());
        for attr in attrs.drain(..) {
            if attr.name() != Some(sym::cfgAttr) {
                out.push(attr);
                continue;
            }
            let AttrKind::Normal(n) = &attr.kind else { unreachable!() };
            let AttrArgs::Delimited(d) = &n.args else {
                self.psess.dcx.span_err(attr.span, "malformed `cfgAttr` attribute: write `@cfgAttr(predicate, attribute)`");
                continue;
            };
            let Ok((pred, used)) = parse_cfg_pred(self.psess, &d.tokens) else { continue };
            let rest = &d.tokens[used..];
            let rest = match rest.first() {
                Some(TokenTree::Token(t)) if t.kind == TokenKind::Comma => &rest[1..],
                _ => {
                    self.psess.dcx.span_err(attr.span, "malformed `cfgAttr` attribute: write `@cfgAttr(predicate, attribute)`");
                    continue;
                }
            };
            if pred.eval(self.cfg) {
                let mut inner = vsc_parse::parse_attrs_from_tts(self.psess, rest);
                // The expanded attributes may hold more `cfgAttr`s.
                self.expand_cfg_attrs(&mut inner);
                out.extend(inner);
            }
        }
        *attrs = out;
    }
}

impl MutVisitor for StripUnconfigured<'_> {
    fn keep(&mut self, attrs: &mut AttrVec) -> bool {
        self.expand_cfg_attrs(attrs);
        for attr in attrs.iter() {
            if attr.name() == Some(sym::cfg) {
                match cfg_attr_pred(self.psess, attr) {
                    Some(pred) if !pred.eval(self.cfg) => return false,
                    _ => {}
                }
            }
        }
        // `@cfg` has done its job.
        attrs.retain(|a| a.name() != Some(sym::cfg));
        true
    }
}
