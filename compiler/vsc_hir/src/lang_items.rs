//! Lang items: library definitions the compiler refers to by role (rustc's
//! `rustc_hir::lang_items`). The library marks them `@lang(name)`; until
//! `library/` exists, HIR names them and type checking will look them up.

use vsc_span::Symbol;

macro_rules! lang_items {
    ($($variant:ident: $name:literal, $target:literal;)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum LangItem { $($variant,)* }

        impl LangItem {
            pub const ALL: &'static [LangItem] = &[$(LangItem::$variant,)*];

            /// The name in `@lang(name)`.
            pub fn name(self) -> &'static str {
                match self { $(LangItem::$variant => $name,)* }
            }

            /// What kind of definition it must be.
            pub fn target(self) -> &'static str {
                match self { $(LangItem::$variant => $target,)* }
            }

            pub fn from_name(name: Symbol) -> Option<LangItem> {
                let s = name.as_str();
                LangItem::ALL.iter().copied().find(|l| l.name() == s)
            }
        }
    };
}

lang_items! {
    // `for` loops.
    IntoIterIntoIter: "intoIter", "method";
    IteratorNext: "next", "method";
    OptionSome: "Some", "variant";
    OptionNone: "None", "variant";

    // `?`
    TryTraitBranch: "branch", "method";
    TryTraitFromResidual: "fromResidual", "method";
    ControlFlowContinue: "Continue", "variant";
    ControlFlowBreak: "Break", "variant";

    // Ranges.
    Range: "Range", "struct";
    RangeFrom: "RangeFrom", "struct";
    RangeTo: "RangeTo", "struct";
    RangeFull: "RangeFull", "struct";
    RangeInclusiveNew: "rangeInclusiveNew", "method";
    RangeToInclusive: "RangeToInclusive", "struct";

    // Calls of values that aren't functions: closures, and parameters
    // bounded by the function traits.
    FnOnce: "fnOnce", "trait";
    FnMut: "fnMut", "trait";
    Fn: "fn", "trait";

    // Exception 2, 3 and 14 types.
    String: "String", "struct";
    CString: "CString", "struct";
    CStr: "CStr", "struct";
    Class: "Class", "struct";
    Func: "Func", "struct";
    CVoid: "c_void", "enum";

    // `vec[…]`
    VecNew: "vecNew", "fn";
    VecFromArray: "vecFromArray", "fn";
    VecFromElem: "vecFromElem", "fn";

    // Format strings and the built-ins that use them.
    FormatArguments: "formatArguments", "struct";
    FormatArgument: "formatArgument", "struct";
    FormatPlaceholder: "formatPlaceholder", "struct";
    FormatCount: "formatCount", "enum";
    FormatAlignment: "formatAlignment", "enum";
    Print: "print", "fn";
    Eprint: "eprint", "fn";
    Format: "format", "fn";
    PanicFmt: "panicFmt", "fn";
    Panic: "panic", "fn";
    AssertFailed: "assertFailed", "fn";
    AssertKind: "assertKind", "enum";
}
