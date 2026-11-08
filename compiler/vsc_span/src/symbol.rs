//! Interned strings.
//!
//! A [`Symbol`] is a `u32` that names a string stored once for the whole
//! process. Keywords and other names the compiler needs to recognize are
//! interned up front in a fixed order, so they are constants: `kw::Func`,
//! `sym::println`.

use std::fmt;
use std::sync::{Mutex, OnceLock};

use vsc_data_structures::FxHashMap;

use crate::{DUMMY_SP, Span};

macro_rules! symbols {
    (
        Keywords { $($kw:ident: $kw_str:literal,)* }
        Symbols { $($sym:ident $(: $sym_str:literal)?,)* }
    ) => {
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[repr(u32)]
        enum KwIndex { $($kw,)* }

        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[repr(u32)]
        enum SymIndex { $($sym,)* }

        const KEYWORD_COUNT: u32 = [$($kw_str,)*].len() as u32;

        /// Keywords, in the order of the grammar's keyword lists.
        #[allow(non_upper_case_globals)]
        pub mod kw {
            use super::{KwIndex, Symbol};
            $(pub const $kw: Symbol = Symbol(KwIndex::$kw as u32);)*
        }

        /// Other names the compiler recognizes.
        #[allow(non_upper_case_globals)]
        pub mod sym {
            use super::{KEYWORD_COUNT, SymIndex, Symbol};
            $(pub const $sym: Symbol = Symbol(KEYWORD_COUNT + SymIndex::$sym as u32);)*
        }

        const PREINTERNED: &[&str] = &[$($kw_str,)* $(symbols!(@str $sym $($sym_str)?),)*];
    };
    (@str $sym:ident $s:literal) => { $s };
    (@str $sym:ident) => { stringify!($sym) };
}

symbols! {
    // The order matters: each group is a contiguous range, which the
    // `is_*_keyword` checks rely on.
    Keywords {
        Empty: "",
        // Strict keywords (grammar: STRICT_KEYWORDS).
        Underscore: "_",
        As: "as",
        Async: "async",
        Await: "await",
        Break: "break",
        Class: "class",
        Const: "const",
        Continue: "continue",
        Dyn: "dyn",
        Else: "else",
        Enum: "enum",
        Extern: "extern",
        False: "false",
        For: "for",
        Func: "func",
        If: "if",
        Impl: "impl",
        Implement: "implement",
        Import: "import",
        In: "in",
        Let: "let",
        Loop: "loop",
        Match: "match",
        Mut: "mut",
        Package: "package",
        Public: "public",
        Ref: "ref",
        Return: "return",
        SelfLower: "self",
        SelfUpper: "Self",
        Static: "static",
        Struct: "struct",
        Trait: "trait",
        True: "true",
        Type: "type",
        Unsafe: "unsafe",
        Use: "use",
        Var: "var",
        Where: "where",
        While: "while",
        // Reserved keywords (grammar: RESERVED_KEYWORDS).
        Abstract: "abstract",
        Become: "become",
        Box: "box",
        Crate: "crate",
        Do: "do",
        Final: "final",
        Fn: "fn",
        Gen: "gen",
        Macro: "macro",
        Mod: "mod",
        Move: "move",
        Override: "override",
        Priv: "priv",
        Pub: "pub",
        Super: "super",
        Try: "try",
        Typeof: "typeof",
        Unsized: "unsized",
        Virtual: "virtual",
        Yield: "yield",
        // Weak keywords (grammar: WEAK_KEYWORDS).
        From: "from",
        Raw: "raw",
        Safe: "safe",
        Union: "union",
    }

    Symbols {
        // Built-in names (grammar: BUILTIN_NAMES). Keep this group first.
        print,
        println,
        eprint,
        eprintln,
        format,
        vec,
        assert,
        assertEq,
        panic,
        todo,
        unreachable,
        asm,
        nakedAsm,
        globalAsm,
        never,
        cstring,
        c_char,
        c_schar,
        c_uchar,
        c_short,
        c_ushort,
        c_int,
        c_uint,
        c_long,
        c_ulong,
        c_longlong,
        c_ulonglong,
        c_float,
        c_double,
        c_void,
        // End of built-in names.
        Origin,

        // Primitive type names (exception 2).
        bool,
        char,
        int,
        int8,
        int16,
        int32,
        int64,
        int128,
        uint,
        uint8,
        uint16,
        uint32,
        uint64,
        uint128,
        float32,
        float64,
        string,

        // Inline assembly.
        att_syntax: "attSyntax",
        clobberAbi,
        inlateout,
        inout,
        label,
        lateout,
        nomem,
        noreturn,
        nostack,
        options,
        out,
        preservesFlags,
        pure,
        readonly,
        sym,

        // Configuration.
        all,
        any,
        cfg,
        cfgAttr,
        not,
        target_os,
        feature,

        // Attributes.
        allow,
        deny,
        deprecated,
        derive,
        doc,
        exportName,
        inline,
        link,
        mustUse,
        repr,
        test,
        warn,

        // Crates and entry points.
        alloc,
        core,
        main,
        std,

        // ABIs.
        C: "C",
        Rust: "Rust",
    }
}

/// An interned string.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Symbol(u32);

struct Interner {
    names: FxHashMap<&'static str, Symbol>,
    strings: Vec<&'static str>,
}

fn interner() -> &'static Mutex<Interner> {
    static INTERNER: OnceLock<Mutex<Interner>> = OnceLock::new();
    INTERNER.get_or_init(|| {
        let mut names = FxHashMap::default();
        for (i, &s) in PREINTERNED.iter().enumerate() {
            let prev = names.insert(s, Symbol(i as u32));
            assert!(prev.is_none(), "symbol {s:?} is listed twice");
        }
        Mutex::new(Interner { names, strings: PREINTERNED.to_vec() })
    })
}

impl Symbol {
    pub fn intern(s: &str) -> Symbol {
        let mut inner = interner().lock().unwrap();
        if let Some(&sym) = inner.names.get(s) {
            return sym;
        }
        let s: &'static str = Box::leak(s.to_string().into_boxed_str());
        let sym = Symbol(inner.strings.len() as u32);
        inner.strings.push(s);
        inner.names.insert(s, sym);
        sym
    }

    /// The interned string. Interned strings live for the whole process.
    pub fn as_str(self) -> &'static str {
        interner().lock().unwrap().strings[self.0 as usize]
    }

    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self == kw::Empty
    }

    /// A keyword that can never be an identifier.
    #[inline]
    pub fn is_strict_keyword(self) -> bool {
        kw::Underscore.0 <= self.0 && self.0 <= kw::While.0
    }

    /// A Rust keyword Vertex doesn't use but keeps reserved (`fn`, `mod`, …).
    #[inline]
    pub fn is_reserved_keyword(self) -> bool {
        kw::Abstract.0 <= self.0 && self.0 <= kw::Yield.0
    }

    /// A keyword only in some positions (`union`, `safe`, `raw`).
    #[inline]
    pub fn is_weak_keyword(self) -> bool {
        kw::From.0 <= self.0 && self.0 <= kw::Union.0
    }

    /// Strict or reserved: not usable as a plain identifier.
    #[inline]
    pub fn is_used_or_reserved_keyword(self) -> bool {
        self.is_strict_keyword() || self.is_reserved_keyword()
    }

    /// A built-in name (`println`, `vec`, `c_int`, …), which can't be redeclared.
    #[inline]
    pub fn is_builtin_name(self) -> bool {
        sym::print.0 <= self.0 && self.0 <= sym::c_void.0
    }

    /// Keywords that can start a path segment: `self` and `Self`.
    #[inline]
    pub fn is_path_segment_keyword(self) -> bool {
        self == kw::SelfLower || self == kw::SelfUpper
    }
}

impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A name and where it was written.
#[derive(Clone, Copy, Eq, Hash)]
pub struct Ident {
    pub name: Symbol,
    pub span: Span,
}

impl Ident {
    #[inline]
    pub const fn new(name: Symbol, span: Span) -> Ident {
        Ident { name, span }
    }

    #[inline]
    pub const fn with_dummy_span(name: Symbol) -> Ident {
        Ident::new(name, DUMMY_SP)
    }

    pub fn from_str(s: &str) -> Ident {
        Ident::with_dummy_span(Symbol::intern(s))
    }

    pub fn empty() -> Ident {
        Ident::with_dummy_span(kw::Empty)
    }

    pub fn as_str(&self) -> &'static str {
        self.name.as_str()
    }

    /// Whether a raw identifier (`r#name`) is needed to write this name.
    pub fn is_reserved(self) -> bool {
        self.name.is_used_or_reserved_keyword()
    }
}

/// Identifiers compare by name only; the span is where it was written.
impl PartialEq for Ident {
    fn eq(&self, other: &Ident) -> bool {
        self.name == other.name
    }
}

impl fmt::Debug for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{:?}", self.name, self.span)
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.name, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preinterned() {
        assert_eq!(kw::Func.as_str(), "func");
        assert_eq!(Symbol::intern("func"), kw::Func);
        assert_eq!(sym::println.as_str(), "println");
        assert_eq!(sym::att_syntax.as_str(), "attSyntax");
        assert_eq!(Symbol::intern("println"), sym::println);
    }

    #[test]
    fn keyword_classes() {
        assert!(kw::Func.is_strict_keyword());
        assert!(kw::While.is_strict_keyword());
        assert!(!kw::Fn.is_strict_keyword());
        assert!(kw::Fn.is_reserved_keyword());
        assert!(kw::Union.is_weak_keyword());
        assert!(sym::vec.is_builtin_name());
        assert!(sym::c_void.is_builtin_name());
        assert!(!sym::int64.is_builtin_name());
        assert!(!Symbol::intern("hello").is_used_or_reserved_keyword());
    }

    #[test]
    fn interning_is_stable() {
        let a = Symbol::intern("someName");
        let b = Symbol::intern("someName");
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "someName");
    }
}
