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

    // Operators on values that aren't primitives (rustc's `ops` and `cmp`
    // lang items).
    Add: "add", "trait";
    Sub: "sub", "trait";
    Mul: "mul", "trait";
    Div: "div", "trait";
    Rem: "rem", "trait";
    Neg: "neg", "trait";
    Not: "not", "trait";
    BitAnd: "bitAnd", "trait";
    BitOr: "bitOr", "trait";
    BitXor: "bitXor", "trait";
    Shl: "shl", "trait";
    Shr: "shr", "trait";
    AddAssign: "addAssign", "trait";
    SubAssign: "subAssign", "trait";
    MulAssign: "mulAssign", "trait";
    DivAssign: "divAssign", "trait";
    RemAssign: "remAssign", "trait";
    BitAndAssign: "bitAndAssign", "trait";
    BitOrAssign: "bitOrAssign", "trait";
    BitXorAssign: "bitXorAssign", "trait";
    ShlAssign: "shlAssign", "trait";
    ShrAssign: "shrAssign", "trait";
    PartialEq: "eq", "trait";
    PartialOrd: "partialOrd", "trait";
    Ordering: "Ordering", "enum";
    Index: "index", "trait";
    IndexMut: "indexMut", "trait";

    // Values that are copied rather than moved.
    Copy: "copy", "trait";

    // Auto traits & pinning.
    Send: "send", "trait";
    Sync: "sync", "trait";
    Unpin: "unpin", "trait";
    Pin: "Pin", "struct";
    PinNewUnchecked: "pinNewUnchecked", "method";

    // Async / Future / Coroutine lang items.
    Poll: "Poll", "enum";
    PollReady: "Ready", "variant";
    PollPending: "Pending", "variant";
    Future: "future", "trait";
    FutureOutput: "futureOutput", "assoc_ty";
    FuturePoll: "poll", "method";
    IntoFuture: "intoFuture", "trait";
    IntoFutureIntoFuture: "intoFutureMethod", "method";
    ResumeTy: "ResumeTy", "struct";
    GetContext: "getContext", "fn";
    Coroutine: "coroutine", "trait";
    CoroutineState: "coroutineState", "enum";
    CoroutineYield: "coroutineYield", "assoc_ty";
    CoroutineReturn: "coroutineReturn", "assoc_ty";
    CoroutineResume: "coroutineResume", "method";
    AsyncFnOnce: "asyncFnOnce", "trait";
    AsyncFnMut: "asyncFnMut", "trait";
    AsyncFn: "asyncFn", "trait";
    BlockOn: "blockOn", "fn";

    // Exception 2, 3 and 14 types.
    Sized: "sized", "trait";
    Drop: "drop", "trait";
    DropInPlace: "dropInPlace", "fn";
    Start: "start", "fn";
    PhantomData: "PhantomData", "struct";
    UnsafeCell: "UnsafeCell", "struct";
    ManuallyDrop: "ManuallyDrop", "struct";
    Deref: "deref", "trait";
    DerefMut: "derefMut", "trait";

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
