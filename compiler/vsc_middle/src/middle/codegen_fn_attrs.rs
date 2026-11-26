//! Codegen-relevant function and static attributes (`rustc_middle::middle::codegen_fn_attrs`).

use std::borrow::Cow;

use vsc_span::Symbol;

use crate::ty::{InstanceKind, TyCtxt};

/// How inlining was requested on a function (`@inline(...)`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum InlineAttr {
    #[default]
    None,
    /// `@inline` or `@inline(hint)`
    Hint,
    /// `@inline(always)`
    Always,
    /// `@inline(never)`
    Never,
}

/// Optimization preference (`@optimize(...)`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum OptimizeAttr {
    #[default]
    Default,
    Speed,
    Size,
    DoNotOptimize,
}

/// Symbol linkage (`rustc_attr_ir::Linkage`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Linkage {
    External,
    AvailableExternally,
    LinkOnceAny,
    LinkOnceODR,
    WeakAny,
    WeakODR,
    Internal,
    Common,
    ExternalWeak,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct CodegenFnAttrFlags(u32);

impl CodegenFnAttrFlags {
    pub const EMPTY: Self = Self(0);
    pub const COLD: Self = Self(1 << 0);
    pub const ALLOCATOR: Self = Self(1 << 1);
    pub const FFI_RETURNS_TWICE: Self = Self(1 << 2);
    pub const FFI_PURE: Self = Self(1 << 3);
    pub const FFI_CONST: Self = Self(1 << 4);
    pub const NAKED: Self = Self(1 << 5);
    pub const NO_MANGLE: Self = Self(1 << 6);
    pub const RUSTC_STD_INTERNAL_SYMBOL: Self = Self(1 << 7);
    pub const THREAD_LOCAL: Self = Self(1 << 8);
    pub const USED_COMPILER: Self = Self(1 << 9);
    pub const USED_LINKER: Self = Self(1 << 10);
    pub const TRACK_CALLER: Self = Self(1 << 11);
    pub const NO_BUILTINS: Self = Self(1 << 12);
    pub const FOREIGN_ITEM: Self = Self(1 << 13);

    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    #[inline]
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct CodegenFnAttrs {
    pub flags: CodegenFnAttrFlags,
    pub inline: InlineAttr,
    pub optimize: OptimizeAttr,
    /// Custom symbol name specified via `@unsafe(exportName = "...")` or `@linkName = "..."`.
    pub symbol_name: Option<Symbol>,
    pub link_section: Option<Symbol>,
    pub linkage: Option<Linkage>,
    pub import_linkage: Option<Linkage>,
    pub target_features: Vec<Symbol>,
}

impl CodegenFnAttrs {
    pub const EMPTY: Self = Self {
        flags: CodegenFnAttrFlags::EMPTY,
        inline: InlineAttr::None,
        optimize: OptimizeAttr::Default,
        symbol_name: None,
        link_section: None,
        linkage: None,
        import_linkage: None,
        target_features: Vec::new(),
    };

    pub fn contains_extern_indicator(&self) -> bool {
        self.flags.contains(CodegenFnAttrFlags::NO_MANGLE)
            || self.symbol_name.is_some()
            || matches!(self.linkage, Some(Linkage::External | Linkage::WeakAny | Linkage::WeakODR))
    }

    pub fn used(&self) -> bool {
        self.flags.contains(CodegenFnAttrFlags::USED_COMPILER)
            || self.flags.contains(CodegenFnAttrFlags::USED_LINKER)
    }
}

impl<'tcx> TyCtxt<'tcx> {
    pub fn codegen_instance_attrs(
        self,
        instance_kind: InstanceKind<'tcx>,
    ) -> Cow<'tcx, CodegenFnAttrs> {
        let mut attrs = Cow::Borrowed(self.codegen_fn_attrs(instance_kind.def_id()));

        if !matches!(instance_kind, InstanceKind::Item(_))
            && attrs.flags.contains(CodegenFnAttrFlags::NAKED)
        {
            attrs.to_mut().flags.remove(CodegenFnAttrFlags::NAKED);
        }

        if let InstanceKind::ReifyShim(..) = instance_kind
            && attrs.flags.contains(CodegenFnAttrFlags::TRACK_CALLER)
        {
            if attrs.symbol_name.is_some() {
                attrs.to_mut().symbol_name = None;
            }
        }

        if let InstanceKind::ClosureOnceShim { closure, .. } = instance_kind {
            let closure_attrs = self.codegen_fn_attrs(closure);
            attrs.to_mut().optimize = closure_attrs.optimize;
        }

        attrs
    }
}
