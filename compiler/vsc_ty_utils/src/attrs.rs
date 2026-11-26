//! Parsing `@repr`, `@unsafe(exportName)`, `@unsafe(noMangle)`, `@inline`,
//! `@linkName`, `@linkSection`, `@used`, `@cold`, and `Drop` destructor lookup.

use vsc_ast::token::{Delimiter, LitKind, TokenKind};
use vsc_ast::tokenstream::TokenTree;
use vsc_ast::{AttrArgs, AttrKind, Attribute, ExprKind};
use vsc_hir::LangItem;
use vsc_hir::def::DefKind;
use vsc_middle::hir::map::Node;
use vsc_middle::middle::codegen_fn_attrs::{CodegenFnAttrFlags, CodegenFnAttrs, InlineAttr};
use vsc_middle::ty::layout::{Align, Integer, IntegerType, ReprFlags, ReprOptions};
use vsc_middle::ty::{AssocKind, TyCtxt, TyKind};
use vsc_span::def_id::DefId;
use vsc_span::{Symbol, sym};

fn parse_str_from_args(args: &AttrArgs) -> Option<Symbol> {
    match args {
        AttrArgs::Eq { expr, .. } => {
            if let ExprKind::Lit(lit) = &expr.kind
                && matches!(lit.kind, LitKind::Str | LitKind::StrRaw(_))
            {
                return Some(lit.symbol);
            }
            None
        }
        AttrArgs::Delimited(delim) => match delim.tokens.as_slice() {
            [TokenTree::Token(tok)] => {
                if let TokenKind::Literal(lit) = tok.kind
                    && matches!(lit.kind, LitKind::Str | LitKind::StrRaw(_))
                {
                    return Some(lit.symbol);
                }
                None
            }
            _ => None,
        },
        AttrArgs::Empty => None,
    }
}

fn parse_int_from_tokens(tokens: &[TokenTree]) -> Option<u64> {
    if let [TokenTree::Token(t)] = tokens
        && let TokenKind::Literal(lit) = t.kind
        && lit.kind == LitKind::Integer
    {
        let clean: String = lit.symbol.as_str().chars().filter(|&c| c != '_').collect();
        if let Some(hex) = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")) {
            return u64::from_str_radix(hex, 16).ok();
        }
        if let Some(bin) = clean.strip_prefix("0b").or_else(|| clean.strip_prefix("0B")) {
            return u64::from_str_radix(bin, 2).ok();
        }
        if let Some(oct) = clean.strip_prefix("0o").or_else(|| clean.strip_prefix("0O")) {
            return u64::from_str_radix(oct, 8).ok();
        }
        return clean.parse().ok();
    }
    None
}

pub(crate) fn repr_options(tcx: TyCtxt<'_>, def_id: DefId) -> ReprOptions {
    let Some(local) = def_id.as_local() else {
        return ReprOptions::default();
    };
    let mut repr = ReprOptions::default();
    for attr in tcx.owner_attrs(local) {
        let AttrKind::Normal(normal) = &attr.kind else {
            continue;
        };
        if !normal.path.is_ident(sym::repr) {
            continue;
        }
        let AttrArgs::Delimited(delim) = &normal.args else {
            continue;
        };
        parse_repr_tokens(&delim.tokens, &mut repr);
    }
    repr
}

fn parse_repr_tokens(tokens: &[TokenTree], repr: &mut ReprOptions) {
    let mut i = 0;
    while i < tokens.len() {
        let TokenTree::Token(tok) = &tokens[i] else {
            i += 1;
            continue;
        };
        match tok.kind {
            TokenKind::Comma => {
                i += 1;
                continue;
            }
            TokenKind::Ident(sym_name, _) => {
                let name = sym_name.as_str();
                match name {
                    "c" | "C" => repr.flags.insert(ReprFlags::IS_C),
                    "transparent" => repr.flags.insert(ReprFlags::IS_TRANSPARENT),
                    "simd" => repr.flags.insert(ReprFlags::IS_SIMD),
                    "packed" => {
                        let mut pack = Align::ONE;
                        if let Some(TokenTree::Delimited(_, Delimiter::Parenthesis, inner)) =
                            tokens.get(i + 1)
                        {
                            if let Some(n) = parse_int_from_tokens(inner)
                                && let Ok(a) = Align::from_bytes(n)
                            {
                                pack = a;
                            }
                            i += 1;
                        }
                        repr.pack = Some(repr.pack.map_or(pack, |prev| prev.min(pack)));
                    }
                    "align" => {
                        if let Some(TokenTree::Delimited(_, Delimiter::Parenthesis, inner)) =
                            tokens.get(i + 1)
                        {
                            if let Some(n) = parse_int_from_tokens(inner)
                                && let Ok(a) = Align::from_bytes(n)
                            {
                                repr.align = Some(repr.align.map_or(a, |prev| prev.max(a)));
                            }
                            i += 1;
                        }
                    }
                    "int8" | "i8" => repr.int = Some(IntegerType::Fixed(Integer::I8, true)),
                    "int16" | "i16" => repr.int = Some(IntegerType::Fixed(Integer::I16, true)),
                    "int32" | "i32" => repr.int = Some(IntegerType::Fixed(Integer::I32, true)),
                    "int64" | "i64" => repr.int = Some(IntegerType::Fixed(Integer::I64, true)),
                    "int128" | "i128" => repr.int = Some(IntegerType::Fixed(Integer::I128, true)),
                    "int" | "isize" => repr.int = Some(IntegerType::Pointer(true)),
                    "uint8" | "u8" => repr.int = Some(IntegerType::Fixed(Integer::I8, false)),
                    "uint16" | "u16" => repr.int = Some(IntegerType::Fixed(Integer::I16, false)),
                    "uint32" | "u32" => repr.int = Some(IntegerType::Fixed(Integer::I32, false)),
                    "uint64" | "u64" => repr.int = Some(IntegerType::Fixed(Integer::I64, false)),
                    "uint128" | "u128" => repr.int = Some(IntegerType::Fixed(Integer::I128, false)),
                    "uint" | "usize" => repr.int = Some(IntegerType::Pointer(false)),
                    _ => {}
                }
            }
            _ => {}
        }
        i += 1;
    }
}

pub(crate) fn adt_destructor(tcx: TyCtxt<'_>, def_id: DefId) -> Option<DefId> {
    let drop_trait = tcx.lang_items().get(LangItem::Drop)?;
    for (local, info) in tcx.defs.iter_enumerated() {
        if info.kind != (DefKind::Impl { of_trait: true }) {
            continue;
        }
        let imp_def = local.to_def_id();
        let Some(trait_ref) = tcx.impl_trait_ref(imp_def) else {
            continue;
        };
        if trait_ref.def_id != drop_trait {
            continue;
        }
        if let TyKind::Adt(adt_def, _) = *trait_ref.self_ty().kind()
            && adt_def.did() == def_id
        {
            if let Some(assoc) = tcx
                .associated_items(imp_def)
                .iter()
                .find(|a| matches!(a.kind, AssocKind::Fn { .. }))
            {
                return Some(assoc.def_id);
            }
        }
    }
    None
}

pub(crate) fn codegen_fn_attrs(tcx: TyCtxt<'_>, def_id: DefId) -> CodegenFnAttrs {
    let Some(local) = def_id.as_local() else {
        return CodegenFnAttrs::EMPTY;
    };
    let mut attrs = CodegenFnAttrs::EMPTY;

    let is_foreign = matches!(tcx.hir_node(local), Some(Node::ForeignItem(_)));
    if is_foreign {
        attrs.flags.insert(CodegenFnAttrFlags::FOREIGN_ITEM);
        attrs.symbol_name = Some(tcx.item_name(def_id));
    }

    for attr in tcx.owner_attrs(local) {
        parse_codegen_attr(tcx, def_id, attr, &mut attrs);
    }

    attrs
}

fn parse_codegen_attr(
    tcx: TyCtxt<'_>,
    def_id: DefId,
    attr: &Attribute,
    out: &mut CodegenFnAttrs,
) {
    let AttrKind::Normal(normal) = &attr.kind else {
        return;
    };
    let Some(seg) = normal.path.segments.last() else {
        return;
    };
    let name = seg.ident.name;

    if name == sym::inline {
        out.inline = match &normal.args {
            AttrArgs::Empty => InlineAttr::Hint,
            AttrArgs::Delimited(delim) => match delim.tokens.as_slice() {
                [TokenTree::Token(tok)] => match tok.kind {
                    TokenKind::Ident(s, _) if s == sym::always => InlineAttr::Always,
                    TokenKind::Ident(s, _) if s == sym::never => InlineAttr::Never,
                    _ => InlineAttr::Hint,
                },
                _ => InlineAttr::Hint,
            },
            AttrArgs::Eq { .. } => InlineAttr::Hint,
        };
    } else if name == sym::cold {
        out.flags.insert(CodegenFnAttrFlags::COLD);
    } else if name == sym::used {
        out.flags.insert(CodegenFnAttrFlags::USED_LINKER);
    } else if name == sym::trackCaller {
        out.flags.insert(CodegenFnAttrFlags::TRACK_CALLER);
    } else if name == sym::noMangle {
        out.flags.insert(CodegenFnAttrFlags::NO_MANGLE);
        if out.symbol_name.is_none() {
            out.symbol_name = Some(tcx.item_name(def_id));
        }
    } else if name == sym::exportName {
        if let Some(s) = parse_str_from_args(&normal.args) {
            out.symbol_name = Some(s);
        }
    } else if name == sym::linkName {
        if let Some(s) = parse_str_from_args(&normal.args) {
            out.symbol_name = Some(s);
        }
    } else if name == sym::linkSection {
        if let Some(s) = parse_str_from_args(&normal.args) {
            out.link_section = Some(s);
        }
    } else if name == sym::threadLocal || name == sym::thread_local {
        out.flags.insert(CodegenFnAttrFlags::THREAD_LOCAL);
    }
}
