# Vertex v2 grammar

The grammar of Vertex v2 `.vs` files. It is [rust_grammar.md](rust_grammar.md), the grammar of The Rust Reference, with the fourteen exceptions from [proposed_vsc_v2.md](proposed_vsc_v2.md) applied production by production. `.vsx` markup is not covered.

| | |
| --- | --- |
| Base | The Rust Reference at [`a2ae66d7037c`](https://github.com/rust-lang/reference/tree/a2ae66d7037cbedac68746b9510b24cc34b96d69) (2026-11-04), the same commit as rust_grammar.md |
| Productions | 343: 68 changed from Rust, 26 added, 35 removed |
| Rule | Anything not changed here is Rust's production, unchanged, and means what it means in Rust |
| License | The Rust productions come from The Rust Reference, dual-licensed MIT and Apache-2.0, copyright The Rust Project Developers |

Under every block, **Changes from Rust** lists what was added, changed or removed, and which exception it comes from. A block with no such list is identical to Rust's.

## Contents

- [Exceptions at a glance](#exceptions-at-a-glance)
- [Notation](#notation)
- [Line breaks](#line-breaks)
- [Decisions this grammar makes](#decisions-this-grammar-makes)
- [Lexical structure](#lexical-structure)
  - [Input format](#input-format)
  - [Shebang](#shebang)
  - [Keywords](#keywords)
  - [Identifiers](#identifiers)
  - [Comments](#comments)
  - [Whitespace](#whitespace)
  - [Tokens](#tokens)
- [Token trees](#token-trees)
- [Source files and packages](#source-files-and-packages)
- [Conditional compilation](#conditional-compilation)
- [Items](#items)
  - [Functions](#functions)
  - [Type aliases](#type-aliases)
  - [Structs](#structs)
  - [Enumerations](#enumerations)
  - [Unions](#unions)
  - [Constant items](#constant-items)
  - [Static items](#static-items)
  - [Traits](#traits)
  - [Implementations](#implementations)
  - [External blocks](#external-blocks)
  - [Generic parameters](#generic-parameters)
  - [Associated items](#associated-items)
- [Attributes](#attributes)
  - [Code generation](#code-generation)
  - [Debugger](#debugger)
- [Statements and expressions](#statements-and-expressions)
  - [Statements](#statements)
  - [Expressions](#expressions)
    - [Literal expressions](#literal-expressions)
    - [Path expressions](#path-expressions)
    - [Block expressions](#block-expressions)
    - [Operator expressions](#operator-expressions)
    - [Grouped expressions](#grouped-expressions)
    - [Array and index expressions](#array-and-index-expressions)
    - [Tuple and index expressions](#tuple-and-index-expressions)
    - [Struct expressions](#struct-expressions)
    - [Call expressions](#call-expressions)
    - [Method call expressions](#method-call-expressions)
    - [Field access expressions](#field-access-expressions)
    - [Function literals](#function-literals)
    - [Loop expressions](#loop-expressions)
    - [Range expressions](#range-expressions)
    - [If expressions](#if-expressions)
    - [Match expressions](#match-expressions)
    - [Return expressions](#return-expressions)
    - [Await expressions](#await-expressions)
    - [Underscore expressions](#underscore-expressions)
- [Patterns](#patterns)
- [Type system](#type-system)
  - [Types](#types)
    - [Never type](#never-type)
    - [Tuple types](#tuple-types)
    - [Array types](#array-types)
    - [Slice types](#slice-types)
    - [Pointer types](#pointer-types)
    - [Function types](#function-types)
    - [Trait object types](#trait-object-types)
    - [Impl trait type](#impl-trait-type)
    - [Inferred type](#inferred-type)
  - [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- [Names](#names)
  - [Paths](#paths)
  - [Visibility and privacy](#visibility-and-privacy)
- [Inline assembly](#inline-assembly)
- [Removed from Rust](#removed-from-rust)
- [Index of productions](#index-of-productions)

## Exceptions at a glance

| # | Exception | Where it shows in the grammar |
| --- | --- | --- |
| 1 | `package` + string imports | `SourceFile`, `PackageDeclaration`, `ImportDeclaration`, `ImportSpec`; `Module`, `ExternCrate`, `UseDeclaration` removed |
| 2 | Long-form types | `NeverType` is `never`; type names like `int64` are names, not grammar (see `BUILTIN_NAMES`) |
| 3 | `class` | `Class` |
| 4 | `func` | `Function`, `BareFunctionType`, keywords |
| 5 | `var` | `LetStatement`, `IdentifierPattern`, `StructPatternField`, `ShorthandSelf`, `TypedSelf`, `StaticItem` |
| 6 | `public` / `package` | `Visibility` |
| 7 | `.` for paths | `SimplePath`, `PathInExpression`, `TypePath` and the qualified paths; `::` removed from `PUNCTUATION` |
| 8 | `implement`, flat receivers | `InherentImpl`, `TraitImpl`, `Receiver` |
| 9 | `@` attributes | `OuterAttribute`; `InnerAttribute` removed |
| 10 | No `;`, newline separation | `LINE_END`, `Terminator`, `Separation`, `Statements`, `StructFields`, `EnumVariants`, `MatchArms`, the item lists |
| 11 | Go naming | Names are a lint, not grammar; Rust names inside built-in syntax use Go casing (`cfgAttr`, `clobberAbi`) |
| 12 | Built-ins without `!` | `BuiltinExpression`, `VecExpression`, `AsmExpression`, `GlobalAsm`, `BUILTIN_NAMES`; all macro productions removed |
| 13 | Origins | `OriginRef`, `Origin`, `OriginParam`, `OriginWhereClauseItem`, `ForOrigins`; `Lifetime` removed |
| 14 | Function literals, `func(T)` values | `FunctionLiteral`, `FunctionValueType`, `AsyncBlockExpression`; `ClosureExpression` removed |

## Notation

The same notation as rust_grammar.md, which is the Reference's grammar notation:

| Notation | Meaning |
| --- | --- |
| `Name -> Expression` | A production. Continuation lines are indented. |
| `@root Name -> …` | A root production: not used by any other production. |
| `UPPER_CASE` | A lexer token, such as `IDENTIFIER`. |
| `CamelCase` | A syntactic production, such as `LetStatement`. |
| `` `text` `` | Exactly these characters. |
| `A B`, `A \| B`, `( … )` | Sequence, ordered choice (first match wins), grouping. Sequence binds tighter than `\|`. |
| `X?`, `X*`, `X+` | Optional, zero or more, one or more. |
| `X{a..b}`, `X{a..=b}`, `X{n:a..=b}`, `X{n}` | Bounded and counted repetitions. |
| `!X` | Negative lookahead. |
| `` [`a`-`z`] ``, `` ~[ … ] `` | Character sets and their complements. |
| `A ^ B` | Hard cut: once *A* matches, *B* must match. |
| `U+0060`, `<text>`, `X _except y_` | A code point, an English description, a modification of the rule before. |
| `X[^name]` | A note, listed under the block. |

## Line breaks

Exception 10 replaces Rust's `;` with line breaks. The productions use `Terminator` (`;` or `LINE_END`) wherever Rust used `;`, and wherever Rust separated fields, variants and arms with commas. Which line feeds become `LINE_END` is decided by these rules, after Swift's:

1. **Only where a terminator is allowed.** A line feed becomes `LINE_END` only at a point where the grammar accepts a `Terminator`, that is, where the statement, declaration, field, variant or arm before it is complete. Everywhere else it is whitespace. Inside `( )` and `[ ]`, and in comma-separated lists such as struct literals, parameters and generic arguments, line feeds are therefore always whitespace.
2. **Unless the next line continues.** Even where a terminator is allowed, a line feed is whitespace if the next line starts with:
   - `.`, so method chains can break before each call
   - a binary operator followed by whitespace, or `as` (`+ x`, `&& y`). A prefix operator written against its operand (`-x`, `!done`, `*p`, `&v`) starts a new statement, as in Swift
   - `where` or `else`
   - `{`, when the line before ended a declaration header (`func`, `struct`, `class`, `enum`, `union`, `trait`, `implement`, `extern`) that may take a body
3. **Comments don't count.** A line that ends in a comment is treated as ending at its last token, and blank lines are like a single line feed.
4. **`;` still works** as a separator between statements, fields, variants or arms on one line. The formatter never writes it at the end of a line.

## Decisions this grammar makes

Writing the exceptions as productions forced some choices that the proposal doesn't spell out. Each follows rule 1 of the proposal ("Rust is the base") unless noted.

- **One edition.** Vertex follows Rust 2024 only. Syntax Rust keeps for older editions is removed: bare trait objects without `dyn`, `a...b` range patterns, type-only parameters, and `extern` blocks without `unsafe`. Edition notes are dropped.
- **Paths and fields share `.`.** The parser reads `a.b.c` as a path. Name resolution then reads the segments after a value as field accesses or a method call, the way Go tells `pkg.Name` from `x.field`.
- **`&[a] T` versus `&[T]`.** After `&`, a bracketed list followed by a type is an origin (`&[a] string`); otherwise it is a slice type (`&[uint8]`). As a generic argument, `[x]` is an origin when *x* names a variable, parameter or origin, and a slice type when it names a type (`Lines<[doc]>` versus `Box<[uint8]>`).
- **`func(T)` versus raw function pointers.** A function type with no qualifier, `func(uint)`, is a function value (Rust's `Rc<dyn Fn>`). With `unsafe` or `extern`, it is a raw function pointer (`unsafe extern "C" func(…)`). Rust's safe, Rust-ABI pointer `fn(i32) -> i32` has no spelling of its own; named functions convert to `func(T)` instead.
- **A literal versus a flat receiver.** At item level, `func (t: &Task) IsOpen()` has a name after the parentheses; a function literal `func(t) { … }` is followed by `{` or `->`.
- **Generics on flat receivers.** Type parameters in a flat receiver's type are introduced by naming them, as in Go: `func (s: &Stack<T>) Push(v: T)`. Bounds on them need an `implement` block.
- **Loop labels keep Rust's `'outer:`.** Exception 13 removes the tick from lifetimes only. Labels (`'outer: loop { break 'outer }`) are not covered by any exception, so they stay Rust's. Go and Swift write `outer:` and `break outer` if you'd rather change them.
- **Literal suffixes stay Rust's.** `1u8`, `2.5f32` and other suffixes still lex. With Vertex's type names they would read `1uint8`; whether to keep suffixes at all is open.
- **Removed Rust keywords stay reserved.** `fn`, `mod`, `move`, `pub`, `crate` and `super` can't be used as names, so Rust code never parses with a different meaning.
- **Struct-like enum variants use newlines too.** `Parse { line: int64; message: string }` follows the struct field rule. The proposal's exception 10 example writes this variant with commas, which this grammar doesn't accept.
- **`extern type` lives inside an extern block.** As in Rust (RFC 1861): `extern "C" { type FILE }`. The proposal writes `extern type FILE` as a standalone item.
- **`impl Trait` keeps `impl`.** In type position only, pending open question 8 in the proposal.
- **`cfg_select!` is dropped.** It is not one of v2's built-ins.

## Lexical structure

### Input format

Rust: [input-format.md](https://doc.rust-lang.org/reference/input-format.html)

<sub>category: lexer · Rust rule `input.syntax`</sub>

```
CHAR -> [U+0000-U+D7FF U+E000-U+10FFFF] // a Unicode scalar value

ASCII -> [U+0000-U+007F]

NUL -> U+0000

EOF -> !CHAR  // End of file or input
```

### Shebang

Rust: [shebang.md](https://doc.rust-lang.org/reference/shebang.html)

<sub>category: lexer · Rust rule `shebang.syntax`</sub>

```
@root SHEBANG ->
    `#!` !((WHITESPACE | LINE_COMMENT | SHEBANG_BLOCK_COMMENT)* `[`)
    ~LF* (LF | EOF)

SHEBANG_BLOCK_COMMENT ->
    `/*` !(`!` | `*` ![`*` `/`])
      ( SHEBANG_NESTED_BLOCK_COMMENT | (!(`*/` | `/*`) CHAR) )*
    `*/`

SHEBANG_NESTED_BLOCK_COMMENT ->
    `/*`
      ( SHEBANG_NESTED_BLOCK_COMMENT | (!(`*/` | `/*`) CHAR) )*
    `*/`
```

### Keywords

Rust: [keywords.md](https://doc.rust-lang.org/reference/keywords.html)

*Strict keywords*

<sub>category: lexer · Rust rule `lex.keywords.strict.syntax`</sub>

```
@root STRICT_KEYWORDS ->
      `_`
    | `as`
    | `async`
    | `await`
    | `break`
    | `class`
    | `const`
    | `continue`
    | `dyn`
    | `else`
    | `enum`
    | `extern`
    | `false`
    | `for`
    | `func`
    | `if`
    | `impl`
    | `implement`
    | `import`
    | `in`
    | `let`
    | `loop`
    | `match`
    | `mut`
    | `package`
    | `public`
    | `ref`
    | `return`
    | `self`
    | `Self`
    | `static`
    | `struct`
    | `trait`
    | `true`
    | `type`
    | `unsafe`
    | `use`
    | `var`
    | `where`
    | `while`
```

**Changes from Rust:**

- Changed `STRICT_KEYWORDS` (exceptions 1, 3, 4, 5, 6, 8, 14): Adds `class`, `func`, `implement`, `import`, `package`, `public`, `var`. Rust's `fn`, `mod`, `move`, `pub`, `crate` and `super` move to the reserved list. `impl` stays for `impl Trait` types (open question 8 in the proposal).

*Reserved keywords*

<sub>category: lexer · Rust rule `lex.keywords.reserved.syntax`</sub>

```
@root RESERVED_KEYWORDS ->
      `abstract`
    | `become`
    | `box`
    | `crate`
    | `do`
    | `final`
    | `fn`
    | `gen`
    | `macro`
    | `mod`
    | `move`
    | `override`
    | `priv`
    | `pub`
    | `super`
    | `try`
    | `typeof`
    | `unsized`
    | `virtual`
    | `yield`
```

**Changes from Rust:**

- Changed `RESERVED_KEYWORDS` (exceptions 1, 4, 6, 14): Rust keywords that Vertex no longer uses stay reserved, so Rust code never parses with a different meaning.

*Weak keywords*

<sub>category: lexer · Rust rule `lex.keywords.weak.syntax`</sub>

```
@root WEAK_KEYWORDS ->
      `raw`
    | `safe`
    | `union`

@root BUILTIN_NAMES ->
      `print` | `println` | `eprint` | `eprintln` | `format` | `vec`
    | `assert` | `assertEq` | `panic` | `todo` | `unreachable`
    | `asm` | `nakedAsm` | `globalAsm`
    | `never` | `Origin` | `cstring`
    | `c_char` | `c_schar` | `c_uchar` | `c_short` | `c_ushort`
    | `c_int` | `c_uint` | `c_long` | `c_ulong` | `c_longlong` | `c_ulonglong`
    | `c_float` | `c_double` | `c_void`
```

**Changes from Rust:**

- Changed `WEAK_KEYWORDS` (exceptions 12, 13): `'static` is replaced by `[static]` (origins), and `macro_rules` goes with user macros.
- Added `BUILTIN_NAMES` (exceptions 2, 12, 13): Built-in names. They are reserved and can't be redeclared, unlike Go's built-ins.

### Identifiers

Rust: [identifiers.md](https://doc.rust-lang.org/reference/identifiers.html)

<sub>category: lexer · Rust rule `ident.syntax`</sub>

```
IDENTIFIER_OR_KEYWORD -> ( XID_Start | `_` ) XID_Continue*

XID_Start -> <`XID_Start` defined by Unicode>

XID_Continue -> <`XID_Continue` defined by Unicode>

RAW_IDENTIFIER -> `r#` IDENTIFIER_OR_KEYWORD

NON_KEYWORD_IDENTIFIER -> IDENTIFIER_OR_KEYWORD _except a [strict][lex.keywords.strict] or [reserved][lex.keywords.reserved] keyword_

IDENTIFIER -> NON_KEYWORD_IDENTIFIER | RAW_IDENTIFIER

RESERVED_RAW_IDENTIFIER ->
    `r#` (`_` | `crate` | `self` | `Self` | `super`) !XID_Continue
```

### Comments

Rust: [comments.md](https://doc.rust-lang.org/reference/comments.html)

<sub>category: lexer · Rust rule `comments.syntax`</sub>

```
@root COMMENT ->
      LINE_COMMENT
    | INNER_LINE_DOC
    | OUTER_LINE_DOC
    | INNER_BLOCK_DOC
    | OUTER_BLOCK_DOC
    | BLOCK_COMMENT

LINE_COMMENT ->
      `//` (~[`/` `!` LF] | `//`) ~LF*
    | `//` EOF
    | `//` _immediately followed by LF_

BLOCK_COMMENT ->
    `/*` ^
      ( BLOCK_COMMENT | BLOCK_CHAR )*
    `*/`

INNER_LINE_DOC ->
    `//!` ^ LINE_DOC_COMMENT_CONTENT (LF | EOF)

LINE_DOC_COMMENT_CONTENT -> (!CR ~LF)*

INNER_BLOCK_DOC ->
    `/*!` ^ ( NESTED_BLOCK_DOC_COMMENT | DOC_BLOCK_CHAR )* `*/`

OUTER_LINE_DOC ->
    `///` ^ LINE_DOC_COMMENT_CONTENT (LF | EOF)

OUTER_BLOCK_DOC ->
    `/**` ![`*` `/`]
      ^
      ~[`*` CR]
      ( NESTED_BLOCK_DOC_COMMENT | DOC_BLOCK_CHAR )*
    `*/`

BLOCK_CHAR -> !`*/` CHAR

DOC_BLOCK_CHAR -> (!(`*/` | CR) CHAR)

NESTED_BLOCK_DOC_COMMENT ->
    `/*` ( NESTED_BLOCK_DOC_COMMENT | DOC_BLOCK_CHAR )* `*/`
```

### Whitespace

Rust: [whitespace.md](https://doc.rust-lang.org/reference/whitespace.html)

<sub>category: lexer · Rust rule `whitespace.syntax`</sub>

```
WHITESPACE ->
      U+0009 // Horizontal tab, `'\t'`
    | U+000A // Line feed, `'\n'`
    | U+000B // Vertical tab
    | U+000C // Form feed
    | U+000D // Carriage return, `'\r'`
    | U+0020 // Space, `' '`
    | U+0085 // Next line
    | U+200E // Left-to-right mark
    | U+200F // Right-to-left mark
    | U+2028 // Line separator
    | U+2029 // Paragraph separator

TAB -> U+0009 // Horizontal tab, `'\t'`

LF -> U+000A  // Line feed, `'\n'`

CR -> U+000D  // Carriage return, `'\r'`

SP -> U+0020  // Space, `' '`

LINE_END ->
    LF _when it ends a statement, declaration, field, variant or arm (see Line breaks)_

Terminator -> `;` | LINE_END

Separation ->
      Terminator+
    | <nothing, after an item, statement or arm that ends with `}`>
```

**Changes from Rust:**

- Added `LINE_END` (exception 10): A line feed that acts as a terminator.
- Added `Terminator` (exception 10): What Rust spells `;`.
- Added `Separation` (exception 10): Between two items, statements or arms. Rust likewise needs no `;` after a block-like statement.

### Tokens

Rust: [tokens.md](https://doc.rust-lang.org/reference/tokens.html)

<sub>category: lexer · Rust rule `lex.token.syntax`</sub>

```
Token ->
      RESERVED_TOKEN
    | RAW_IDENTIFIER
    | CHAR_LITERAL
    | STRING_LITERAL
    | RAW_STRING_LITERAL
    | BYTE_LITERAL
    | BYTE_STRING_LITERAL
    | RAW_BYTE_STRING_LITERAL
    | C_STRING_LITERAL
    | RAW_C_STRING_LITERAL
    | FLOAT_LITERAL
    | INTEGER_LITERAL
    | LIFETIME_TOKEN
    | PUNCTUATION
    | IDENTIFIER_OR_KEYWORD
```

*Suffixes*

<sub>category: lexer · Rust rule `lex.token.literal.suffix.syntax`</sub>

```
SUFFIX ->
      `_` ^ XID_Continue+
    | XID_Start XID_Continue*
```

*Character literals*

<sub>category: lexer · Rust rule `lex.token.literal.char.syntax`</sub>

```
CHAR_LITERAL ->
    `'`
        ( ~[`'` `\` LF CR TAB] | QUOTE_ESCAPE | ASCII_ESCAPE | UNICODE_ESCAPE )
    `'` SUFFIX?

QUOTE_ESCAPE -> `\'` | `\"`

ASCII_ESCAPE ->
      `\x` OCT_DIGIT HEX_DIGIT
    | `\n` | `\r` | `\t` | `\\` | `\0`

UNICODE_ESCAPE ->
    `\u{` ( HEX_DIGIT `_`* ){1..=6} _valid hex char value_ `}`[^valid-hex-char]
```

- `[^valid-hex-char]`: See lex.token.literal.char-escape.unicode.

*String literals*

<sub>category: lexer · Rust rule `lex.token.literal.str.syntax`</sub>

```
STRING_LITERAL ->
    `"` (
        ~[`"` `\` CR]
      | QUOTE_ESCAPE
      | ASCII_ESCAPE
      | UNICODE_ESCAPE
      | STRING_CONTINUE
    )* `"` SUFFIX?

STRING_CONTINUE -> `\` LF [TAB LF CR SP]*
```

*Raw string literals*

<sub>category: lexer · Rust rule `lex.token.literal.str-raw.syntax`</sub>

```
RAW_STRING_LITERAL ->
      `r` `"` ^ RAW_STRING_CONTENT `"` SUFFIX?
    | `r` `#`{n:1..=255} ^ `"` RAW_STRING_CONTENT_HASHED `"` `#`{n} SUFFIX?

RAW_STRING_CONTENT -> (!`"` ~CR )*

RAW_STRING_CONTENT_HASHED -> (!(`"` `#`{n}) ~CR )*
```

*Byte literals*

<sub>category: lexer · Rust rule `lex.token.byte.syntax`</sub>

```
BYTE_LITERAL ->
    `b'` ^ ( ASCII_FOR_CHAR | BYTE_ESCAPE )  `'` SUFFIX?

ASCII_FOR_CHAR -> ![`'` `\` LF CR TAB] ASCII

BYTE_ESCAPE ->
      `\x` HEX_DIGIT HEX_DIGIT
    | `\n` | `\r` | `\t` | `\\` | `\0` | `\'` | `\"`
```

*Byte string literals*

<sub>category: lexer · Rust rule `lex.token.str-byte.syntax`</sub>

```
BYTE_STRING_LITERAL ->
    `b"` ^ ( ASCII_FOR_STRING | BYTE_ESCAPE | STRING_CONTINUE )* `"` SUFFIX?

ASCII_FOR_STRING -> ![`"` `\` CR] ASCII
```

*Raw byte string literals*

<sub>category: lexer · Rust rule `lex.token.str-byte-raw.syntax`</sub>

```
RAW_BYTE_STRING_LITERAL ->
      `br` `"` ^ RAW_BYTE_STRING_CONTENT `"` SUFFIX?
    | `br` `#`{n:1..=255} ^ `"` RAW_BYTE_STRING_CONTENT_HASHED `"` `#`{n} SUFFIX?

RAW_BYTE_STRING_CONTENT -> (!`"` ASCII_FOR_RAW )*

RAW_BYTE_STRING_CONTENT_HASHED -> (!(`"` `#`{n}) ASCII_FOR_RAW )*

ASCII_FOR_RAW -> !CR ASCII
```

*C string literals*

<sub>category: lexer · Rust rule `lex.token.str-c.syntax`</sub>

```
C_STRING_LITERAL ->
    `c"` ^ (
        ~[`"` `\` CR NUL]
      | !(`\0` | `\x00`) BYTE_ESCAPE
      | !(`\u{` (`0` `_`*){1..=6} `}`) UNICODE_ESCAPE
      | STRING_CONTINUE
    )* `"` SUFFIX?
```

*Raw C string literals*

<sub>category: lexer · Rust rule `lex.token.str-c-raw.syntax`</sub>

```
RAW_C_STRING_LITERAL ->
      `cr` `"` ^ RAW_C_STRING_CONTENT `"` SUFFIX?
    | `cr` `#`{n:1..=255} ^ `"` RAW_C_STRING_CONTENT_HASHED `"` `#`{n} SUFFIX?

RAW_C_STRING_CONTENT -> (!`"` ~[CR NUL] )*

RAW_C_STRING_CONTENT_HASHED -> (!(`"` `#`{n}) ~[CR NUL] )*
```

*Integer literals*

<sub>category: lexer · Rust rule `lex.token.literal.int.syntax`</sub>

```
INTEGER_LITERAL ->
    ( BIN_LITERAL | OCT_LITERAL | HEX_LITERAL | DEC_LITERAL )
    ^ !RESERVED_FLOAT SUFFIX?

DEC_LITERAL -> DEC_DIGIT (DEC_DIGIT|`_`)*

BIN_LITERAL -> `0b` ^ `_`* BIN_DIGIT (BIN_DIGIT|`_`)* ![`e` `E` `2`-`9`]

OCT_LITERAL -> `0o` ^ `_`* OCT_DIGIT (OCT_DIGIT|`_`)* ![`e` `E` `8`-`9`]

HEX_LITERAL -> `0x` ^ `_`* HEX_DIGIT (HEX_DIGIT|`_`)*

BIN_DIGIT -> [`0`-`1`]

OCT_DIGIT -> [`0`-`7`]

DEC_DIGIT -> [`0`-`9`]

HEX_DIGIT -> [`0`-`9` `a`-`f` `A`-`F`]

RESERVED_FLOAT -> `.` !(`.` | `_` | XID_Start)
```

*Tuple index*

<sub>category: lexer · Rust rule `lex.token.literal.int.tuple-field.syntax`</sub>

```
TUPLE_INDEX -> DEC_LITERAL | BIN_LITERAL | OCT_LITERAL | HEX_LITERAL
```

*Floating-point literals*

<sub>category: lexer · Rust rule `lex.token.literal.float.syntax`</sub>

```
FLOAT_LITERAL ->
      DEC_LITERAL (`.` DEC_LITERAL)? FLOAT_EXPONENT SUFFIX?
    | DEC_LITERAL `.` DEC_LITERAL SUFFIX?
    | DEC_LITERAL `.` !(`.` | `_` | XID_Start)

FLOAT_EXPONENT ->
    (`e`|`E`) ^ (`+`|`-`)? `_`* DEC_DIGIT (DEC_DIGIT|`_`)*
```

*Lifetimes and loop labels*

<sub>category: lexer · Rust rule `lex.token.life.syntax`</sub>

```
LIFETIME_TOKEN ->
      RAW_LIFETIME
    | `'` IDENTIFIER_OR_KEYWORD !`'`

LIFETIME_OR_LABEL ->
      RAW_LIFETIME
    | `'` NON_KEYWORD_IDENTIFIER !`'`

RAW_LIFETIME ->
    `'r#` ^ IDENTIFIER_OR_KEYWORD !`'`

RESERVED_RAW_LIFETIME -> `'r#` (`_` | `crate` | `self` | `Self` | `super`) !(`'` | XID_Continue)
```

*Punctuation*

<sub>category: lexer · Rust rule `lex.token.punct.syntax`</sub>

```
PUNCTUATION ->
      `...`
    | `..=`
    | `<<=`
    | `>>=`
    | `!=`
    | `%=`
    | `&&`
    | `&=`
    | `*=`
    | `+=`
    | `-=`
    | `->`
    | `..`
    | `/=`
    | `<-`
    | `<<`
    | `<=`
    | `==`
    | `=>`
    | `>=`
    | `>>`
    | `^=`
    | `|=`
    | `||`
    | `!`
    | `%`
    | `&`
    | `(`
    | `)`
    | `*`
    | `+`
    | `,`
    | `-`
    | `.`
    | `/`
    | `:`
    | `;`
    | `<`
    | `=`
    | `>`
    | `?`
    | `@`
    | `[`
    | `]`
    | `^`
    | `{`
    | `|`
    | `}`
    | `~`
```

**Changes from Rust:**

- Changed `PUNCTUATION` (exceptions 7, 9, 12): Removes `::` (paths use `.`), `#` (attributes use `@`) and `$` (macro metavariables).

*Reserved tokens*

<sub>category: lexer · Rust rule `lex.token.reserved.syntax`</sub>

```
RESERVED_TOKEN ->
      RESERVED_GUARDED_STRING_LITERAL
    | RESERVED_POUNDS
    | RESERVED_RAW_IDENTIFIER
    | RESERVED_RAW_LIFETIME
    | RESERVED_TOKEN_DOUBLE_QUOTE
    | RESERVED_TOKEN_LIFETIME
    | RESERVED_TOKEN_POUND
    | RESERVED_TOKEN_SINGLE_QUOTE
```

*Reserved prefixes*

<sub>category: lexer · Rust rule `lex.token.reserved-prefix.syntax`</sub>

```
RESERVED_TOKEN_DOUBLE_QUOTE ->
    IDENTIFIER_OR_KEYWORD _except `b` or `c` or `r` or `br` or `cr`_ `"`

RESERVED_TOKEN_SINGLE_QUOTE ->
    IDENTIFIER_OR_KEYWORD _except `b`_ `'`

RESERVED_TOKEN_POUND ->
    IDENTIFIER_OR_KEYWORD _except `r` or `br` or `cr`_ `#`

RESERVED_TOKEN_LIFETIME ->
    `'` IDENTIFIER_OR_KEYWORD _except `r`_ `#`
```

*Reserved guards*

<sub>category: lexer · Rust rule `lex.token.reserved-guards.syntax`</sub>

```
RESERVED_GUARDED_STRING_LITERAL -> `#`+ STRING_LITERAL

RESERVED_POUNDS -> `#`{2..}
```

## Token trees

Rust: [macros.md](https://doc.rust-lang.org/reference/macros.html)

*Macro invocation*

<sub>category: macros · Rust rule `macro.invocation.syntax`</sub>

```
DelimTokenTree ->
      `(` TokenTree* `)`
    | `[` TokenTree* `]`
    | `{` TokenTree* `}`

TokenTree ->
    Token _except [delimiters][lex.token.delim]_ | DelimTokenTree
```

**Changes from Rust:**

- Removed `MacroInvocation` (exception 12): No user macros. Built-ins are ordinary calls (`println(…)`) or `vec[…]`.
- Removed `MacroInvocationSemi` (exception 12): No user macros.

## Source files and packages

Rust: [crates-and-source-files.md](https://doc.rust-lang.org/reference/crates-and-source-files.html)

<sub>category: items · Rust rule `crate.syntax`</sub>

```
@root SourceFile ->
    Terminator*
    ( PackageDeclaration Separation )?
    ( ImportDeclaration Separation )*
    ItemList?

PackageDeclaration -> OuterAttribute* `package` IDENTIFIER

ImportDeclaration ->
      `import` ImportSpec
    | `import` `(` ImportSpec* `)`

ImportSpec -> IDENTIFIER? STRING_LITERAL

ItemList -> Terminator* Item ( Separation Item )* Terminator*
```

**Changes from Rust:**

- Removed `Crate` (exception 1): Replaced by `SourceFile`: a file belongs to its folder's package.
- Added `SourceFile` (exception 1): A source file: an optional package declaration, imports, then items.
- Added `PackageDeclaration` (exceptions 1, 9): Go's package clause. Attributes on it apply to the whole package (Rust's inner attributes `#![…]`).
- Added `ImportDeclaration` (exception 1): Go's import declaration, single or grouped.
- Added `ImportSpec` (exception 1): An import path, optionally bound to another name: `geo "./geometry"`.
- Added `ItemList` (exception 10): Items separated by line breaks or `;`.

## Conditional compilation

Rust: [conditional-compilation.md](https://doc.rust-lang.org/reference/conditional-compilation.html)

<sub>category: configuration · Rust rule `cfg.syntax`</sub>

```
ConfigurationPredicate ->
      ConfigurationOption
    | ConfigurationAll
    | ConfigurationAny
    | ConfigurationNot
    | `true`
    | `false`

ConfigurationOption ->
    IDENTIFIER ( `=` ( STRING_LITERAL | RAW_STRING_LITERAL ) )?

ConfigurationAll ->
    `all` `(` ConfigurationPredicateList? `)`

ConfigurationAny ->
    `any` `(` ConfigurationPredicateList? `)`

ConfigurationNot ->
    `not` `(` ConfigurationPredicate `)`

ConfigurationPredicateList ->
    ConfigurationPredicate (`,` ConfigurationPredicate)* `,`?
```

*The `cfg` attribute*

<sub>category: configuration · Rust rule `cfg.attr.syntax`</sub>

```
@root CfgAttribute -> `cfg` `(` ConfigurationPredicate `)`
```

*The `cfg_attr` attribute*

<sub>category: configuration · Rust rule `cfg.cfg_attr.syntax`</sub>

```
@root CfgAttrAttribute -> `cfgAttr` `(` ConfigurationPredicate `,` CfgAttrs? `)`

CfgAttrs -> Attr (`,` Attr)* `,`?
```

**Changes from Rust:**

- Changed `CfgAttrAttribute` (exception 11): Attribute names use Go casing. Configuration keys (`target_os`) keep Rust's spelling, as in the proposal.

## Items

Rust: [items.md](https://doc.rust-lang.org/reference/items.html)

<sub>category: items · Rust rule `items.syntax`</sub>

```
Item ->
    OuterAttribute* VisItem

VisItem ->
    Visibility?
    (
        Function
      | TypeAlias
      | Struct
      | Class
      | Enumeration
      | Union
      | ConstantItem
      | StaticItem
      | Trait
      | Implementation
      | ExternBlock
      | GlobalAsm
    )
```

**Changes from Rust:**

- Changed `Item` (exception 12): No `MacroItem`.
- Changed `VisItem` (exceptions 1, 3, 12): No `Module`, `ExternCrate` or `UseDeclaration`. Adds `Class` and `GlobalAsm`.
- Removed `MacroItem` (exception 12): No user macros.

### Functions

Rust: [items/functions.md](https://doc.rust-lang.org/reference/items/functions.html)

<sub>category: items · Rust rule `items.fn.syntax`</sub>

```
Function ->
    FunctionQualifiers `func` Receiver? IDENTIFIER GenericParams?
        `(` FunctionParameters? `)`
        FunctionReturnType? WhereClause?
        BlockExpression?

Receiver -> `(` OuterAttribute* IDENTIFIER `:` Type `)`

FunctionQualifiers -> `const`? `async`? ItemSafety?[^extern-safe] (`extern` Abi?)?

ItemSafety -> `safe`[^extern-safe] | `unsafe`

Abi -> STRING_LITERAL | RAW_STRING_LITERAL

FunctionParameters ->
      SelfParam `,`?
    | (SelfParam `,`)? FunctionParam (`,` FunctionParam)* `,`?

SelfParam -> OuterAttribute* ( ShorthandSelf | TypedSelf )

ShorthandSelf -> ( `&` OriginRef? `mut`? | `var` )? `self`

TypedSelf -> `var`? `self` `:` Type

FunctionParam -> OuterAttribute* ( FunctionParamPattern | `...` )

FunctionParamPattern -> PatternNoTopAlt `:` ( Type | `...` )

FunctionReturnType -> `->` Type
```

- `[^extern-safe]`: The `safe` function qualifier is only allowed semantically within `extern` blocks.

**Changes from Rust:**

- Changed `Function` (exceptions 4, 8, 10): `fn` → `func`. Adds the flat `Receiver`. A missing body ends the declaration instead of `;`.
- Added `Receiver` (exception 8): Go's method receiver, with Rust's `&`, `&mut` or by-value types: `func (t: &Task) IsOpen()`.
- Changed `FunctionQualifiers`: Vertex has one edition (Rust 2024), so the edition notes go.
- Changed `ShorthandSelf` (exceptions 5, 13): `mut self` → `var self`; `&'a self` → `&[a] self`.
- Changed `TypedSelf` (exception 5): `mut self: T` → `var self: T`.
- Changed `FunctionParam`: Parameters with only a type (2015 edition) go.

### Type aliases

Rust: [items/type-aliases.md](https://doc.rust-lang.org/reference/items/type-aliases.html)

<sub>category: items · Rust rule `items.type.syntax`</sub>

```
TypeAlias ->
    `type` IDENTIFIER GenericParams? ( `:` Bounds? )?
        WhereClause?
        ( `=` Type WhereClause?)?
```

**Changes from Rust:**

- Changed `TypeAlias` (exception 10): No trailing `;`.

### Structs

Rust: [items/structs.md](https://doc.rust-lang.org/reference/items/structs.html)

<sub>category: items · Rust rule `items.struct.syntax`</sub>

```
Struct ->
      StructStruct
    | TupleStruct

StructStruct ->
    `struct` IDENTIFIER GenericParams? WhereClause? ( `{` StructFields? `}` )?

TupleStruct ->
    `struct` IDENTIFIER GenericParams? `(` TupleFields? `)` WhereClause?

StructFields -> Terminator* StructField ( Terminator+ StructField )* Terminator*

StructField -> OuterAttribute* Visibility? IDENTIFIER `:` Type

TupleFields -> TupleField (`,` TupleField)* `,`?

TupleField -> OuterAttribute* Visibility? Type

Class ->
    `class` IDENTIFIER GenericParams? WhereClause? `{` StructFields? `}`
```

**Changes from Rust:**

- Changed `StructStruct` (exception 10): A unit struct is `struct Unit`, with no `;`.
- Changed `TupleStruct` (exception 10): No trailing `;`.
- Changed `StructFields` (exception 10): Fields are separated by line breaks, or `;` on one line, not commas.
- Added `Class` (exception 3): A shared, reference-counted object: Rust's `Rc<RefCell<T>>`. Fields as in a struct. No inheritance.

### Enumerations

Rust: [items/enumerations.md](https://doc.rust-lang.org/reference/items/enumerations.html)

<sub>category: items · Rust rule `items.enum.syntax`</sub>

```
Enumeration ->
    `enum` IDENTIFIER GenericParams? WhereClause? `{` EnumVariants? `}`

EnumVariants -> Terminator* EnumVariant ( Terminator+ EnumVariant )* Terminator*

EnumVariant ->
    OuterAttribute* Visibility?
    IDENTIFIER ( EnumVariantTuple | EnumVariantStruct )? EnumVariantDiscriminant?

EnumVariantTuple -> `(` TupleFields? `)`

EnumVariantStruct -> `{` StructFields? `}`

EnumVariantDiscriminant -> `=` Expression
```

**Changes from Rust:**

- Changed `EnumVariants` (exception 10): Variants are separated by line breaks, or `;` on one line.

### Unions

Rust: [items/unions.md](https://doc.rust-lang.org/reference/items/unions.html)

<sub>category: items · Rust rule `items.union.syntax`</sub>

```
Union ->
    `union` IDENTIFIER GenericParams? WhereClause? `{` StructFields? `}`
```

### Constant items

Rust: [items/constant-items.md](https://doc.rust-lang.org/reference/items/constant-items.html)

<sub>category: items · Rust rule `items.const.syntax`</sub>

```
ConstantItem ->
    `const` ( IDENTIFIER | `_` ) `:` Type ( `=` Expression )?
```

**Changes from Rust:**

- Changed `ConstantItem` (exception 10): No trailing `;`.

### Static items

Rust: [items/static-items.md](https://doc.rust-lang.org/reference/items/static-items.html)

<sub>category: items · Rust rule `items.static.syntax`</sub>

```
StaticItem ->
    ItemSafety?[^extern-safety] `static` `var`? IDENTIFIER `:` Type ( `=` Expression )?
```

- `[^extern-safety]`: The `safe` and `unsafe` function qualifiers are only allowed semantically within `extern` blocks.

**Changes from Rust:**

- Changed `StaticItem` (exceptions 5, 10): `static mut` → `static var`. No trailing `;`.

### Traits

Rust: [items/traits.md](https://doc.rust-lang.org/reference/items/traits.html)

<sub>category: items · Rust rule `items.traits.syntax`</sub>

```
Trait ->
    `unsafe`? `trait` IDENTIFIER GenericParams? ( `:` Bounds? )? WhereClause?
    `{` AssociatedItems? `}`
```

**Changes from Rust:**

- Changed `Trait` (exceptions 9, 10): No inner attributes. Items are separated by line breaks.

### Implementations

Rust: [items/implementations.md](https://doc.rust-lang.org/reference/items/implementations.html)

<sub>category: items · Rust rule `items.impl.syntax`</sub>

```
Implementation -> InherentImpl | TraitImpl

InherentImpl ->
    `implement` GenericParams? Type WhereClause? `{` AssociatedItems? `}`

TraitImpl ->
    `unsafe`? `implement` GenericParams? `!`? TypePath `for` Type
    WhereClause?
    `{` AssociatedItems? `}`
```

**Changes from Rust:**

- Changed `InherentImpl` (exceptions 8, 9, 10): `impl` → `implement`. No inner attributes.
- Changed `TraitImpl` (exceptions 8, 9, 10): `impl` → `implement`. No inner attributes.

### External blocks

Rust: [items/external-blocks.md](https://doc.rust-lang.org/reference/items/external-blocks.html)

<sub>category: items · Rust rule `items.extern.syntax`</sub>

```
ExternBlock ->
    `unsafe` `extern` Abi? `{` ExternalItems? `}`

ExternalItem ->
    OuterAttribute* (
        Visibility? StaticItem
      | Visibility? Function
      | ExternType
    )

ExternalItems -> Terminator* ExternalItem ( Separation ExternalItem )* Terminator*

ExternType -> `type` IDENTIFIER
```

**Changes from Rust:**

- Changed `ExternBlock` (exceptions 9, 10): `unsafe` is required, as in Rust 2024. No inner attributes. Items are separated by line breaks.
- Changed `ExternalItem` (exception 12): No macro invocations. Adds `ExternType`.
- Added `ExternalItems` (exception 10): Items separated by line breaks or `;`.
- Added `ExternType`: An opaque foreign type, used only behind a pointer: Rust's unstable `extern { type FILE; }` (RFC 1861), which the proposal uses.

### Generic parameters

Rust: [items/generics.md](https://doc.rust-lang.org/reference/items/generics.html)

<sub>category: items · Rust rule `items.generics.syntax`</sub>

```
GenericParams -> `<` ( GenericParam (`,` GenericParam)* `,`? )? `>`

GenericParam -> OuterAttribute* ( OriginParam | TypeParam | ConstParam )

OriginParam -> IDENTIFIER `:` `Origin` ( `+` OriginRef )*

TypeParam -> IDENTIFIER ( `:` Bounds? )? ( `=` Type )?

ConstParam ->
    `const` IDENTIFIER `:` Type
    ( `=` ( BlockExpression | IDENTIFIER | `-`?LiteralExpression ) )?
```

**Changes from Rust:**

- Changed `GenericParam` (exception 13): `LifetimeParam` → `OriginParam`.
- Removed `LifetimeParam` (exception 13): Replaced by `OriginParam`.
- Added `OriginParam` (exception 13): Rust's `'a: 'b` is `a: Origin + [b]`. Mojo declares origins as `origin: Origin`.

*Where clauses*

<sub>category: items · Rust rule `items.generics.where.syntax`</sub>

```
WhereClause -> `where` ( WhereClauseItem `,` )* WhereClauseItem?

WhereClauseItem ->
      OriginWhereClauseItem
    | TypeBoundWhereClauseItem

OriginWhereClauseItem -> IDENTIFIER `:` OriginRef ( `+` OriginRef )*

TypeBoundWhereClauseItem -> ForOrigins? Type `:` Bounds?
```

**Changes from Rust:**

- Changed `WhereClauseItem` (exception 13): `LifetimeWhereClauseItem` → `OriginWhereClauseItem`.
- Removed `LifetimeWhereClauseItem` (exception 13): Replaced by `OriginWhereClauseItem`.
- Added `OriginWhereClauseItem` (exception 13): Rust's `where 'a: 'b` is `where a: [b]`.
- Changed `TypeBoundWhereClauseItem` (exception 13): `ForLifetimes` → `ForOrigins`.

### Associated items

Rust: [items/associated-items.md](https://doc.rust-lang.org/reference/items/associated-items.html)

<sub>category: items · Rust rule `items.associated.syntax`</sub>

```
AssociatedItem ->
    OuterAttribute* Visibility? ( TypeAlias | ConstantItem | Function )

AssociatedItems -> Terminator* AssociatedItem ( Separation AssociatedItem )* Terminator*
```

**Changes from Rust:**

- Changed `AssociatedItem` (exception 12): No macro invocations.
- Added `AssociatedItems` (exception 10): Items separated by line breaks or `;`.

## Attributes

Rust: [attributes.md](https://doc.rust-lang.org/reference/attributes.html)

<sub>category: attributes · Rust rule `attributes.syntax`</sub>

```
OuterAttribute -> `@` Attr

Attr ->
      SimplePath AttrInput?
    | `unsafe` `(` SimplePath AttrInput? `)`

AttrInput ->
      DelimTokenTree
    | `=` Expression
```

**Changes from Rust:**

- Removed `InnerAttribute` (exception 9): No inner attributes. Attributes on the `package` declaration apply to the whole package.
- Changed `OuterAttribute` (exception 9): Rust's `#[…]` is `@…`: `@derive(Debug)`, `@inline`, `@unsafe(exportName = "vx_add")`.

*Meta item attribute syntax*

<sub>category: attributes · Rust rule `attributes.meta.syntax`</sub>

```
@root MetaItem ->
      SimplePath
    | SimplePath `=` Expression
    | SimplePath `(` MetaSeq? `)`

MetaSeq ->
    MetaItemInner ( `,` MetaItemInner )* `,`?

MetaItemInner ->
      MetaItem
    | Expression
```

<sub>category: attributes · Rust rule `attributes.meta.builtin-syntax`</sub>

```
@root MetaWord ->
    IDENTIFIER

MetaNameValueStr ->
    IDENTIFIER `=` (STRING_LITERAL | RAW_STRING_LITERAL)

@root MetaListPaths ->
    IDENTIFIER `(` ( SimplePath (`,` SimplePath)* `,`? )? `)`

@root MetaListIdents ->
    IDENTIFIER `(` ( IDENTIFIER (`,` IDENTIFIER)* `,`? )? `)`

@root MetaListNameValueStr ->
    IDENTIFIER `(` ( MetaNameValueStr (`,` MetaNameValueStr)* `,`? )? `)`
```

### Code generation

Rust: [attributes/codegen.md](https://doc.rust-lang.org/reference/attributes/codegen.html)

*The `inline` attribute*

<sub>category: attributes · Rust rule `attributes.codegen.inline.syntax`</sub>

```
@root InlineAttribute ->
      `inline` `(` `always` `)`
    | `inline` `(` `never` `)`
    | `inline`
```

*The `optimize` attribute*

<sub>category: attributes · Rust rule `attributes.codegen.optimize.syntax`</sub>

```
@root OptimizeAttribute ->
    `optimize` `(` ( `size` | `speed` | `none` ) `)`
```

### Debugger

Rust: [attributes/debugger.md](https://doc.rust-lang.org/reference/attributes/debugger.html)

*The `collapse_debuginfo` attribute*

<sub>category: attributes · Rust rule `attributes.debugger.collapse_debuginfo.syntax`</sub>

```
@root CollapseDebuginfoAttribute -> `collapseDebuginfo` `(` CollapseDebuginfoOption `)`

CollapseDebuginfoOption ->
      `yes`
    | `no`
    | `external`
```

**Changes from Rust:**

- Changed `CollapseDebuginfoAttribute` (exception 11): Attribute names use Go casing.

## Statements and expressions

### Statements

Rust: [statements.md](https://doc.rust-lang.org/reference/statements.html)

<sub>category: statements · Rust rule `statement.syntax`</sub>

```
Statement ->
      Item
    | LetStatement
    | ExpressionStatement
```

**Changes from Rust:**

- Changed `Statement` (exceptions 10, 12): No empty `;` statement and no macro statements.

*`let` statements*

<sub>category: statements · Rust rule `statement.let.syntax`</sub>

```
LetStatement ->
    OuterAttribute* ( `let` | `var` ) PatternNoTopAlt ( `:` Type )?
    (
          `=` Expression
        | `=` Expression _except [LazyBooleanExpression] or end with a `}`_
              `else` BlockExpression
    )?
```

**Changes from Rust:**

- Changed `LetStatement` (exceptions 5, 10): `var` declares mutable bindings, Rust's `let mut`. No trailing `;`.

*Expression statements*

<sub>category: statements · Rust rule `statement.expr.syntax`</sub>

```
ExpressionStatement ->
      ExpressionWithoutBlock
    | ExpressionWithBlock
```

**Changes from Rust:**

- Changed `ExpressionStatement` (exception 10): No `;`. See `Statements` for how statements are separated.

### Expressions

Rust: [expressions.md](https://doc.rust-lang.org/reference/expressions.html)

<sub>category: expressions · Rust rule `expr.syntax`</sub>

```
Expression ->
      ExpressionWithoutBlock
    | ExpressionWithBlock

ExpressionWithoutBlock ->
    OuterAttribute* ExpressionWithoutBlockNoAttrs

ExpressionWithoutBlockNoAttrs ->
      LiteralExpression
    | PathExpression
    | OperatorExpression
    | GroupedExpression
    | ArrayExpression
    | AwaitExpression
    | IndexExpression
    | TupleExpression
    | TupleIndexingExpression
    | StructExpression
    | CallExpression
    | MethodCallExpression
    | FieldExpression
    | FunctionLiteral
    | AsyncBlockExpression
    | ContinueExpression
    | BreakExpression
    | RangeExpression
    | ReturnExpression
    | UnderscoreExpression
    | BuiltinExpression

ExpressionWithBlock ->
    OuterAttribute* ExpressionWithBlockNoAttrs

ExpressionWithBlockNoAttrs ->
      BlockExpression
    | ConstBlockExpression
    | UnsafeBlockExpression
    | LoopExpression
    | IfExpression
    | MatchExpression

BuiltinExpression ->
      VecExpression
    | AsmExpression

VecExpression -> `vec` `[` ArrayElements? `]`
```

**Changes from Rust:**

- Changed `ExpressionWithoutBlockNoAttrs` (exceptions 12, 14): `ClosureExpression` → `FunctionLiteral`. `MacroInvocation` → `BuiltinExpression`.
- Added `BuiltinExpression` (exception 12): Built-ins with their own argument syntax. The others (`println`, `format`, `assert`, `panic`, …) are ordinary calls.
- Added `VecExpression` (exception 12): Rust's `vec![…]` without the `!`, keeping its brackets.

*Expression precedence* (strongest first, unchanged from Rust)

| Operator/Expression         | Associativity       |
|-----------------------------|---------------------|
| Paths          |                     |
| Method calls |                     |
| Field expressions | left to right   |
| Function calls, array indexing | |
| `?`             |                     |
| Unary `-` `!` `*` borrow | |
| `as`             | left to right       |
| `*` `/` `%` | left to right       |
| `+` `-` | left to right       |
| `<<` `>>` | left to right     |
| `&`     | left to right       |
| `^`     | left to right       |
| `\|` | left to right       |
| `==` `!=` `<` `>` `<=` `>=` | Require parentheses |
| `&&`     | left to right       |
| `\|\|` | left to right       |
| `..` `..=` | Require parentheses |
| `=` `+=` `-=` `*=` `/=` `%=` `&=` `\|=` `^=` `<<=` `>>=` | right to left |
| `return` `break` function literals  | |

#### Literal expressions

Rust: [expressions/literal-expr.md](https://doc.rust-lang.org/reference/expressions/literal-expr.html)

<sub>category: expressions · Rust rule `expr.literal.syntax`</sub>

```
LiteralExpression ->
      CHAR_LITERAL
    | STRING_LITERAL
    | RAW_STRING_LITERAL
    | BYTE_LITERAL
    | BYTE_STRING_LITERAL
    | RAW_BYTE_STRING_LITERAL
    | C_STRING_LITERAL
    | RAW_C_STRING_LITERAL
    | INTEGER_LITERAL
    | FLOAT_LITERAL
    | `true`
    | `false`
```

#### Path expressions

Rust: [expressions/path-expr.md](https://doc.rust-lang.org/reference/expressions/path-expr.html)

<sub>category: expressions · Rust rule `expr.path.syntax`</sub>

```
PathExpression ->
      PathInExpression
    | QualifiedPathInExpression
```

#### Block expressions

Rust: [expressions/block-expr.md](https://doc.rust-lang.org/reference/expressions/block-expr.html)

<sub>category: expressions · Rust rule `expr.block.syntax`</sub>

```
BlockExpression ->
    `{`
        Statements?
    `}`

Statements ->
    Terminator* Statement ( Separation Statement )* Terminator*
```

**Changes from Rust:**

- Changed `BlockExpression` (exception 9): No inner attributes.
- Removed `BlockExpressionNoInnerAttributes` (exception 9): Every block has no inner attributes, so this is `BlockExpression`.
- Changed `Statements` (exception 10): Statements are separated by line breaks or `;`. The block's value is its last statement when that is an expression statement (Scala's rule). If the block's type is `()`, the value is discarded.

*`async` blocks*

<sub>category: expressions · Rust rule `expr.block.async.syntax`</sub>

```
AsyncBlockExpression -> `async` BlockExpression
```

**Changes from Rust:**

- Changed `AsyncBlockExpression` (exception 14): No `move`: ownership of captures is inferred.

*`const` blocks*

<sub>category: expressions · Rust rule `expr.block.const.syntax`</sub>

```
ConstBlockExpression -> `const` BlockExpression
```

*`unsafe` blocks*

<sub>category: expressions · Rust rule `expr.block.unsafe.syntax`</sub>

```
UnsafeBlockExpression -> `unsafe` BlockExpression
```

#### Operator expressions

Rust: [expressions/operator-expr.md](https://doc.rust-lang.org/reference/expressions/operator-expr.html)

<sub>category: expressions · Rust rule `expr.operator.syntax`</sub>

```
OperatorExpression ->
      BorrowExpression
    | DereferenceExpression
    | TryPropagationExpression
    | NegationExpression
    | ArithmeticOrLogicalExpression
    | ComparisonExpression
    | LazyBooleanExpression
    | TypeCastExpression
    | AssignmentExpression
    | CompoundAssignmentExpression
```

*Borrow operators*

<sub>category: expressions · Rust rule `expr.operator.borrow.syntax`</sub>

```
BorrowExpression ->
      (`&`|`&&`) Expression
    | (`&`|`&&`) `mut` Expression
    | (`&`|`&&`) `raw` `const` Expression
    | (`&`|`&&`) `raw` `mut` Expression
```

*The dereference operator*

<sub>category: expressions · Rust rule `expr.deref.syntax`</sub>

```
DereferenceExpression -> `*` Expression
```

*The try propagation expression*

<sub>category: expressions · Rust rule `expr.try.syntax`</sub>

```
TryPropagationExpression -> Expression `?`
```

*Negation operators*

<sub>category: expressions · Rust rule `expr.negate.syntax`</sub>

```
NegationExpression ->
      `-` Expression
    | `!` Expression
```

*Arithmetic and logical binary operators*

<sub>category: expressions · Rust rule `expr.arith-logic.syntax`</sub>

```
ArithmeticOrLogicalExpression ->
      Expression `+` Expression
    | Expression `-` Expression
    | Expression `*` Expression
    | Expression `/` Expression
    | Expression `%` Expression
    | Expression `&` Expression
    | Expression `|` Expression
    | Expression `^` Expression
    | Expression `<<` Expression
    | Expression `>>` Expression
```

*Comparison operators*

<sub>category: expressions · Rust rule `expr.cmp.syntax`</sub>

```
ComparisonExpression ->
      Expression `==` Expression
    | Expression `!=` Expression
    | Expression `>` Expression
    | Expression `<` Expression
    | Expression `>=` Expression
    | Expression `<=` Expression
```

*Lazy boolean operators*

<sub>category: expressions · Rust rule `expr.bool-logic.syntax`</sub>

```
LazyBooleanExpression ->
      Expression `||` Expression
    | Expression `&&` Expression
```

*Type cast expressions*

<sub>category: expressions · Rust rule `expr.as.syntax`</sub>

```
TypeCastExpression -> Expression `as` TypeNoBounds
```

*Assignment expressions*

<sub>category: expressions · Rust rule `expr.assign.syntax`</sub>

```
AssignmentExpression -> Expression `=` Expression
```

*Compound assignment expressions*

<sub>category: expressions · Rust rule `expr.compound-assign.syntax`</sub>

```
CompoundAssignmentExpression ->
      Expression `+=` Expression
    | Expression `-=` Expression
    | Expression `*=` Expression
    | Expression `/=` Expression
    | Expression `%=` Expression
    | Expression `&=` Expression
    | Expression `|=` Expression
    | Expression `^=` Expression
    | Expression `<<=` Expression
    | Expression `>>=` Expression
```

#### Grouped expressions

Rust: [expressions/grouped-expr.md](https://doc.rust-lang.org/reference/expressions/grouped-expr.html)

<sub>category: expressions · Rust rule `expr.paren.syntax`</sub>

```
GroupedExpression -> `(` Expression `)`
```

#### Array and index expressions

Rust: [expressions/array-expr.md](https://doc.rust-lang.org/reference/expressions/array-expr.html)

*Array expressions*

<sub>category: expressions · Rust rule `expr.array.syntax`</sub>

```
ArrayExpression -> `[` ArrayElements? `]`

ArrayElements ->
      Expression ( `,` Expression )* `,`?
    | Expression `;` Expression
```

*Array and slice indexing expressions*

<sub>category: expressions · Rust rule `expr.array.index.syntax`</sub>

```
IndexExpression -> Expression `[` Expression `]`
```

#### Tuple and index expressions

Rust: [expressions/tuple-expr.md](https://doc.rust-lang.org/reference/expressions/tuple-expr.html)

*Tuple expressions*

<sub>category: expressions · Rust rule `expr.tuple.syntax`</sub>

```
TupleExpression -> `(` TupleElements? `)`

TupleElements -> ( Expression `,` )+ Expression?
```

*Tuple indexing expressions*

<sub>category: expressions · Rust rule `expr.tuple-index.syntax`</sub>

```
TupleIndexingExpression -> Expression `.` TUPLE_INDEX
```

#### Struct expressions

Rust: [expressions/struct-expr.md](https://doc.rust-lang.org/reference/expressions/struct-expr.html)

<sub>category: expressions · Rust rule `expr.struct.syntax`</sub>

```
StructExpression ->
    PathInExpression `{` (StructExprFields | StructBase)? `}`

StructExprFields ->
    StructExprField (`,` StructExprField)* (`,` StructBase | `,`?)

StructExprField ->
    OuterAttribute*
    (
        IDENTIFIER
      | (IDENTIFIER | TUPLE_INDEX) `:` Expression
    )

StructBase -> `..` Expression
```

#### Call expressions

Rust: [expressions/call-expr.md](https://doc.rust-lang.org/reference/expressions/call-expr.html)

<sub>category: expressions · Rust rule `expr.call.syntax`</sub>

```
CallExpression -> Expression `(` CallParams? `)`

CallParams -> Expression ( `,` Expression )* `,`?
```

#### Method call expressions

Rust: [expressions/method-call-expr.md](https://doc.rust-lang.org/reference/expressions/method-call-expr.html)

*Method-call expressions*

<sub>category: expressions · Rust rule `expr.method.syntax`</sub>

```
MethodCallExpression -> Expression `.` PathExprSegment `(`CallParams? `)`
```

#### Field access expressions

Rust: [expressions/field-expr.md](https://doc.rust-lang.org/reference/expressions/field-expr.html)

<sub>category: expressions · Rust rule `expr.field.syntax`</sub>

```
FieldExpression -> Expression `.` IDENTIFIER
```

#### Function literals

Rust: [expressions/closure-expr.md](https://doc.rust-lang.org/reference/expressions/closure-expr.html)

<sub>category: expressions · Rust rule `expr.closure.syntax`</sub>

```
FunctionLiteral ->
    `async`?
    `func` `(` ClosureParameters? `)` ( `->` TypeNoBounds )?
    BlockExpression

ClosureParameters -> ClosureParam (`,` ClosureParam)* `,`?

ClosureParam -> OuterAttribute* PatternNoTopAlt ( `:` Type )?
```

**Changes from Rust:**

- Removed `ClosureExpression` (exception 14): Replaced by `FunctionLiteral`.
- Added `FunctionLiteral` (exception 14): Go's function literal with Rust's closure meaning: `func(t) { t.IsOpen() }`. No `move`; a literal that escapes takes ownership of its captures.

#### Loop expressions

Rust: [expressions/loop-expr.md](https://doc.rust-lang.org/reference/expressions/loop-expr.html)

*Loops and other breakable expressions*

<sub>category: expressions · Rust rule `expr.loop.syntax`</sub>

```
LoopExpression ->
    LoopLabel? (
        InfiniteLoopExpression
      | PredicateLoopExpression
      | IteratorLoopExpression
      | LabelBlockExpression
    )
```

*Infinite loops*

<sub>category: expressions · Rust rule `expr.loop.infinite.syntax`</sub>

```
InfiniteLoopExpression -> `loop` BlockExpression
```

*Predicate loops*

<sub>category: expressions · Rust rule `expr.loop.while.syntax`</sub>

```
PredicateLoopExpression -> `while` Conditions BlockExpression
```

*Iterator loops*

<sub>category: expressions · Rust rule `expr.loop.for.syntax`</sub>

```
IteratorLoopExpression ->
    `for` Pattern `in` Expression _except [StructExpression]_ BlockExpression
```

*Loop labels*

<sub>category: expressions · Rust rule `expr.loop.label.syntax`</sub>

```
LoopLabel -> LIFETIME_OR_LABEL `:`
```

*`break` expressions*

<sub>category: expressions · Rust rule `expr.loop.break.syntax`</sub>

```
BreakExpression -> `break` LIFETIME_OR_LABEL? Expression?
```

*Labeled block expressions*

<sub>category: expressions · Rust rule `expr.loop.block-labels.syntax`</sub>

```
LabelBlockExpression -> BlockExpression
```

*`continue` expressions*

<sub>category: expressions · Rust rule `expr.loop.continue.syntax`</sub>

```
ContinueExpression -> `continue` LIFETIME_OR_LABEL?
```

#### Range expressions

Rust: [expressions/range-expr.md](https://doc.rust-lang.org/reference/expressions/range-expr.html)

<sub>category: expressions · Rust rule `expr.range.syntax`</sub>

```
RangeExpression ->
      RangeExpr
    | RangeFromExpr
    | RangeToExpr
    | RangeFullExpr
    | RangeInclusiveExpr
    | RangeToInclusiveExpr

RangeExpr -> Expression `..` Expression

RangeFromExpr -> Expression `..`

RangeToExpr -> `..` Expression

RangeFullExpr -> `..`

RangeInclusiveExpr -> Expression `..=` Expression

RangeToInclusiveExpr -> `..=` Expression
```

#### If expressions

Rust: [expressions/if-expr.md](https://doc.rust-lang.org/reference/expressions/if-expr.html)

*`if` expressions*

<sub>category: expressions · Rust rule `expr.if.syntax`</sub>

```
IfExpression ->
    `if` Conditions BlockExpression
    (`else` ( BlockExpression | IfExpression ) )?

Conditions ->
      Expression _except [StructExpression]_
    | LetChain

LetChain -> LetChainCondition ( `&&` LetChainCondition )*

LetChainCondition ->
      Expression _except [ExcludedConditions]_
    | OuterAttribute* `let` Pattern `=` Scrutinee _except [ExcludedConditions]_

@root ExcludedConditions ->
      StructExpression
    | LazyBooleanExpression
    | RangeExpr
    | RangeFromExpr
    | RangeInclusiveExpr
    | AssignmentExpression
    | CompoundAssignmentExpression
```

**Changes from Rust:**

- Changed `IfExpression` (exception 9): `BlockExpressionNoInnerAttributes` → `BlockExpression`.

#### Match expressions

Rust: [expressions/match-expr.md](https://doc.rust-lang.org/reference/expressions/match-expr.html)

*`match` expressions*

<sub>category: expressions · Rust rule `expr.match.syntax`</sub>

```
MatchExpression ->
    `match` Scrutinee `{`
        MatchArms?
    `}`

Scrutinee -> Expression _except [StructExpression]_

MatchArms ->
    Terminator* MatchArmWithBody ( Separation MatchArmWithBody )* Terminator*

MatchArmWithBody -> MatchArm `=>` Expression

MatchArm -> OuterAttribute* Pattern MatchArmGuard?

MatchArmGuard -> `if` MatchConditions

MatchConditions ->
     MatchGuardChain
   | Expression

MatchGuardChain -> MatchGuardCondition ( `&&` MatchGuardCondition )*

MatchGuardCondition ->
     Expression _except [ExcludedMatchConditions]_
   | OuterAttribute* `let` Pattern `=` MatchGuardScrutinee

MatchGuardScrutinee -> Expression _except [ExcludedMatchConditions]_

@root ExcludedMatchConditions ->
      LazyBooleanExpression
    | RangeExpr
    | RangeFromExpr
    | RangeInclusiveExpr
    | AssignmentExpression
    | CompoundAssignmentExpression
```

**Changes from Rust:**

- Changed `MatchExpression` (exception 9): No inner attributes.
- Changed `MatchArms` (exception 10): Arms are separated by line breaks, or `;` on one line, not commas.
- Added `MatchArmWithBody` (exception 10): One arm and its body.

#### Return expressions

Rust: [expressions/return-expr.md](https://doc.rust-lang.org/reference/expressions/return-expr.html)

*`return` expressions*

<sub>category: expressions · Rust rule `expr.return.syntax`</sub>

```
ReturnExpression -> `return` Expression?
```

#### Await expressions

Rust: [expressions/await-expr.md](https://doc.rust-lang.org/reference/expressions/await-expr.html)

<sub>category: expressions · Rust rule `expr.await.syntax`</sub>

```
AwaitExpression -> Expression `.` `await`
```

#### Underscore expressions

Rust: [expressions/underscore-expr.md](https://doc.rust-lang.org/reference/expressions/underscore-expr.html)

*`_` expressions*

<sub>category: expressions · Rust rule `expr.placeholder.syntax`</sub>

```
UnderscoreExpression -> `_`
```

## Patterns

Rust: [patterns.md](https://doc.rust-lang.org/reference/patterns.html)

<sub>category: patterns · Rust rule `patterns.syntax`</sub>

```
Pattern -> `|`? PatternNoTopAlt  ( `|` PatternNoTopAlt )*

PatternNoTopAlt ->
      PatternWithoutModernRange
    | ModernRangePattern

PatternWithoutModernRange ->
      LiteralPattern
    | IdentifierPattern
    | WildcardPattern
    | RestPattern
    | ReferencePattern
    | StructPattern
    | TupleStructPattern
    | TuplePattern
    | GroupedPattern
    | SlicePattern
    | PathPattern
```

**Changes from Rust:**

- Changed `PatternWithoutModernRange` (exception 12): No macro invocations, and no `ObsoleteRangePattern` (invalid since Rust 2021).

*Literal patterns*

<sub>category: patterns · Rust rule `patterns.literal.syntax`</sub>

```
LiteralPattern -> `-`? LiteralExpression
```

*Identifier patterns*

<sub>category: patterns · Rust rule `patterns.ident.syntax`</sub>

```
IdentifierPattern -> ( `ref` `mut`? | `var` )? IDENTIFIER ( `@` PatternNoTopAlt )?
```

**Changes from Rust:**

- Changed `IdentifierPattern` (exception 5): A mutable binding is `var x` (Rust's `mut x`). `ref mut x` keeps its spelling: it binds a `&mut` reference.

*Wildcard pattern*

<sub>category: patterns · Rust rule `patterns.wildcard.syntax`</sub>

```
WildcardPattern -> `_`
```

*Rest pattern*

<sub>category: patterns · Rust rule `patterns.rest.syntax`</sub>

```
RestPattern -> `..`
```

*Range patterns*

<sub>category: patterns · Rust rule `patterns.range.syntax`</sub>

```
ModernRangePattern ->
      RangeExclusivePattern
    | RangeInclusivePattern
    | RangeFromPattern
    | RangeToExclusivePattern
    | RangeToInclusivePattern

RangeExclusivePattern ->
      RangePatternBound `..` RangePatternBound

RangeInclusivePattern ->
      RangePatternBound `..=` RangePatternBound

RangeFromPattern ->
      RangePatternBound `..`

RangeToExclusivePattern ->
      `..` RangePatternBound

RangeToInclusivePattern ->
      `..=` RangePatternBound

RangePatternBound ->
      LiteralPattern
    | PathExpression
```

**Changes from Rust:**

- Removed `ObsoleteRangePattern`: `a...b` patterns are invalid since Rust 2021.

*Reference patterns*

<sub>category: patterns · Rust rule `patterns.ref.syntax`</sub>

```
ReferencePattern -> (`&`|`&&`) `mut`? PatternWithoutModernRange
```

*Struct patterns*

<sub>category: patterns · Rust rule `patterns.struct.syntax`</sub>

```
StructPattern ->
    PathInExpression `{`
        StructPatternElements?
    `}`

StructPatternElements ->
      StructPatternFields (`,` | `,` StructPatternEtCetera)?
    | StructPatternEtCetera

StructPatternFields ->
    StructPatternField (`,` StructPatternField)*

StructPatternField ->
    OuterAttribute*
    (
        TUPLE_INDEX `:` Pattern
      | IDENTIFIER `:` Pattern
      | ( `ref` `mut`? | `var` )? IDENTIFIER
    )

StructPatternEtCetera -> `..`
```

**Changes from Rust:**

- Changed `StructPatternField` (exception 5): `mut x` → `var x`.

*Tuple struct patterns*

<sub>category: patterns · Rust rule `patterns.tuple-struct.syntax`</sub>

```
TupleStructPattern -> PathInExpression `(` TupleStructItems? `)`

TupleStructItems -> Pattern ( `,` Pattern )* `,`?
```

*Tuple patterns*

<sub>category: patterns · Rust rule `patterns.tuple.syntax`</sub>

```
TuplePattern -> `(` TuplePatternItems? `)`

TuplePatternItems ->
      Pattern `,`
    | RestPattern
    | Pattern (`,` Pattern)+ `,`?
```

*Grouped patterns*

<sub>category: patterns · Rust rule `patterns.paren.syntax`</sub>

```
GroupedPattern -> `(` Pattern `)`
```

*Slice patterns*

<sub>category: patterns · Rust rule `patterns.slice.syntax`</sub>

```
SlicePattern -> `[` SlicePatternItems? `]`

SlicePatternItems -> Pattern (`,` Pattern)* `,`?
```

*Path patterns*

<sub>category: patterns · Rust rule `patterns.path.syntax`</sub>

```
PathPattern -> PathExpression
```

## Type system

### Types

Rust: [types.md](https://doc.rust-lang.org/reference/types.html)

*Type expressions*

<sub>category: types · Rust rule `type.name.syntax`</sub>

```
Type ->
      TypeNoBounds
    | ImplTraitType
    | TraitObjectType

TypeNoBounds ->
      ParenthesizedType
    | ImplTraitTypeOneBound
    | TraitObjectTypeOneBound
    | TypePath
    | TupleType
    | NeverType
    | RawPointerType
    | ReferenceType
    | ArrayType
    | SliceType
    | InferredType
    | QualifiedPathInType
    | BareFunctionType
    | FunctionValueType
```

**Changes from Rust:**

- Changed `TypeNoBounds` (exceptions 12, 14): No macro invocations. Adds `FunctionValueType`.

*Parenthesized types*

<sub>category: types · Rust rule `type.name.parenthesized.syntax`</sub>

```
ParenthesizedType -> `(` Type `)`
```

#### Never type

Rust: [types/never.md](https://doc.rust-lang.org/reference/types/never.html)

<sub>category: types · Rust rule `type.never.syntax`</sub>

```
NeverType -> `never`
```

**Changes from Rust:**

- Changed `NeverType` (exception 2): Rust's `!` type is `never`.

#### Tuple types

Rust: [types/tuple.md](https://doc.rust-lang.org/reference/types/tuple.html)

<sub>category: types · Rust rule `type.tuple.syntax`</sub>

```
TupleType ->
      `(` `)`
    | `(` ( Type `,` )+ Type? `)`
```

#### Array types

Rust: [types/array.md](https://doc.rust-lang.org/reference/types/array.html)

<sub>category: types · Rust rule `type.array.syntax`</sub>

```
ArrayType -> `[` Type `;` Expression `]`
```

#### Slice types

Rust: [types/slice.md](https://doc.rust-lang.org/reference/types/slice.html)

<sub>category: types · Rust rule `type.slice.syntax`</sub>

```
SliceType -> `[` Type `]`
```

#### Pointer types

Rust: [types/pointer.md](https://doc.rust-lang.org/reference/types/pointer.html)

*References (`&` and `&mut`)*

<sub>category: types · Rust rule `type.pointer.reference.syntax`</sub>

```
ReferenceType -> `&` OriginRef? `mut`? TypeNoBounds
```

**Changes from Rust:**

- Changed `ReferenceType` (exception 13): `&'a T` → `&[a] T`. After `&`, a bracketed list followed by a type is an origin; otherwise it's a slice type: `&[a] string` versus `&[uint8]`.

*Raw pointers (`*const` and `*mut`)*

<sub>category: types · Rust rule `type.pointer.raw.syntax`</sub>

```
RawPointerType -> `*` ( `mut` | `const` ) TypeNoBounds
```

#### Function types

Rust: [types/function-pointer.md](https://doc.rust-lang.org/reference/types/function-pointer.html)

<sub>category: types · Rust rule `type.fn-pointer.syntax`</sub>

```
BareFunctionType ->
    ForOrigins? FunctionTypeQualifiers `func`
       `(` FunctionParametersMaybeNamedVariadic? `)` BareFunctionReturnType?

FunctionTypeQualifiers ->
      `unsafe` (`extern` Abi?)?
    | `extern` Abi?

FunctionValueType ->
    ForOrigins? `func` `(` MaybeNamedFunctionParameters? `)` BareFunctionReturnType?

BareFunctionReturnType -> `->` TypeNoBounds

FunctionParametersMaybeNamedVariadic ->
    MaybeNamedFunctionParameters | MaybeNamedFunctionParametersVariadic

MaybeNamedFunctionParameters ->
    MaybeNamedParam ( `,` MaybeNamedParam )* `,`?

MaybeNamedParam ->
    OuterAttribute* ( ( IDENTIFIER | `_` ) `:` )? Type

MaybeNamedFunctionParametersVariadic ->
    ( MaybeNamedParam `,` )* MaybeNamedParam `,` OuterAttribute* `...`
```

**Changes from Rust:**

- Changed `BareFunctionType` (exceptions 4, 13, 14): A raw function pointer, such as a C callback: `unsafe extern "C" func(*const c_void) -> c_int`. At least one qualifier is required; without one, `func(…)` is a `FunctionValueType`.
- Changed `FunctionTypeQualifiers` (exception 14): At least one qualifier, to tell a raw pointer from a function value.
- Added `FunctionValueType` (exception 14): Go's function type, meaning Rust's `Rc<dyn Fn(T) -> U>`: `func(uint)`, `func(&Task) -> bool`. Literals and named functions convert to it.

#### Trait object types

Rust: [types/trait-object.md](https://doc.rust-lang.org/reference/types/trait-object.html)

*Trait objects*

<sub>category: types · Rust rule `type.trait-object.syntax`</sub>

```
TraitObjectType -> `dyn` Bounds?

TraitObjectTypeOneBound -> `dyn` TraitBound?
```

**Changes from Rust:**

- Changed `TraitObjectType`: `dyn` is required, as in Rust 2021 and later. Bare trait objects go.
- Changed `TraitObjectTypeOneBound`: `dyn` is required, as in Rust 2021 and later.

#### Impl trait type

Rust: [types/impl-trait.md](https://doc.rust-lang.org/reference/types/impl-trait.html)

*Impl trait*

<sub>category: types · Rust rule `type.impl-trait.syntax`</sub>

```
ImplTraitType -> `impl` Bounds?

ImplTraitTypeOneBound -> `impl` TraitBound?
```

#### Inferred type

Rust: [types/inferred.md](https://doc.rust-lang.org/reference/types/inferred.html)

<sub>category: types · Rust rule `type.inferred.syntax`</sub>

```
InferredType -> `_`
```

### Trait and lifetime bounds

Rust: [trait-bounds.md](https://doc.rust-lang.org/reference/trait-bounds.html)

<sub>category: miscellaneous · Rust rule `bound.syntax`</sub>

```
Bounds -> Bound ( `+` Bound )* `+`?

Bound -> OriginRef | TraitBound | UseBound

TraitBound ->
      ( `?` | ForOrigins )? TypePath
    | `(` ( `?` | ForOrigins )? TypePath `)`

OriginRef -> `[` Origin ( `,` Origin )* `]`

Origin -> IDENTIFIER | `static`

UseBound -> `use` UseBoundGenericArgs

UseBoundGenericArgs ->
      `<` `>`
    | `<` ( UseBoundGenericArg `,`)* UseBoundGenericArg `,`? `>`

UseBoundGenericArg ->
      IDENTIFIER
    | `Self`
```

**Changes from Rust:**

- Changed `Bound` (exception 13): `Lifetime` → `OriginRef`: `T: [src]`.
- Changed `TraitBound` (exception 13): `ForLifetimes` → `ForOrigins`.
- Removed `LifetimeBounds` (exception 13): Origin bounds are `OriginRef` lists joined by `+`.
- Removed `Lifetime` (exception 13): Replaced by `OriginRef`.
- Added `OriginRef` (exception 13): Names the parameters or origins a reference is borrowed from: `&[a, b] string`. Mojo writes `ref [a] T`.
- Added `Origin` (exception 13): A function parameter, an origin parameter, or `static` (Rust's `'static`). Rust's `'_` is written by leaving the origin out.
- Changed `UseBoundGenericArg` (exception 13): Origin parameters are identifiers, so `Lifetime` goes.

*Higher-ranked trait bounds*

<sub>category: miscellaneous · Rust rule `bound.higher-ranked.syntax`</sub>

```
ForOrigins -> `for` GenericParams
```

**Changes from Rust:**

- Removed `ForLifetimes` (exception 13): Replaced by `ForOrigins`.
- Added `ForOrigins` (exception 13): Rust's `for<'a>` is `for<a: Origin>`.

## Names

### Paths

Rust: [paths.md](https://doc.rust-lang.org/reference/paths.html)

*Simple paths*

<sub>category: paths · Rust rule `paths.simple.syntax`</sub>

```
SimplePath ->
    SimplePathSegment (`.` SimplePathSegment)*

SimplePathSegment -> IDENTIFIER
```

**Changes from Rust:**

- Changed `SimplePath` (exceptions 1, 7): `.` separates segments. No leading `::`.
- Changed `SimplePathSegment` (exceptions 1, 12): No `super`, `self`, `crate` or `$crate`: there is no module tree.

*Paths in expressions*

<sub>category: paths · Rust rule `paths.expr.syntax`</sub>

```
PathInExpression ->
    PathExprSegment (`.` PathExprSegment)*

PathExprSegment -> PathIdentSegment

PathIdentSegment ->
    IDENTIFIER | `self` | `Self`

GenericArgs ->
      `<` GenericArgList? `>`
    | `(` TypeList? `)` (`->` TypeNoBounds)?

GenericArgList ->
    ( GenericArg `,` )* GenericArg `,`?

TypeList ->
    ( Type `,` )* Type `,`?

GenericArg ->
    Type | OriginRef | GenericArgsConst | GenericArgsBinding | GenericArgsBounds

GenericArgsConst ->
      BlockExpression
    | LiteralExpression
    | `-` LiteralExpression
    | SimplePathSegment

GenericArgsBinding ->
    TypePathSegment `=` Type

GenericArgsBounds ->
    TypePathSegment `:` Bounds?
```

**Changes from Rust:**

- Changed `PathInExpression` (exception 7): `.` separates segments. No leading `::`.
- Changed `PathExprSegment` (exception 7): No `::<…>` generic arguments in expressions. Explicit types come from annotations (open question 1 in the proposal).
- Changed `PathIdentSegment` (exceptions 1, 12): No `super`, `crate` or `$crate`.
- Changed `GenericArg` (exception 13): `Lifetime` → `OriginRef`.

*Qualified paths*

<sub>category: paths · Rust rule `paths.qualified.syntax`</sub>

```
QualifiedPathInExpression -> QualifiedPathType (`.` PathExprSegment)+

QualifiedPathType -> `<` Type (`as` TypePath)? `>`

QualifiedPathInType -> QualifiedPathType (`.` TypePathSegment)+
```

**Changes from Rust:**

- Changed `QualifiedPathInExpression` (exception 7): `::` → `.`.
- Changed `QualifiedPathInType` (exception 7): `::` → `.`.

*Paths in types*

<sub>category: paths · Rust rule `paths.type.syntax`</sub>

```
TypePath -> TypePathSegment (`.` TypePathSegment)*

TypePathSegment -> PathIdentSegment GenericArgs?
```

**Changes from Rust:**

- Changed `TypePath` (exception 7): `.` separates segments. No leading `::`.
- Changed `TypePathSegment` (exception 7): No `::` before generic arguments.

### Visibility and privacy

Rust: [visibility-and-privacy.md](https://doc.rust-lang.org/reference/visibility-and-privacy.html)

<sub>category: items · Rust rule `vis.syntax`</sub>

```
Visibility ->
      `public`
    | `package`
```

**Changes from Rust:**

- Changed `Visibility` (exception 6): `pub` → `public`. `pub(crate)` → `package`. There is no module tree, so `pub(self)`, `pub(super)` and `pub(in …)` go.

## Inline assembly

Rust: [inline-assembly.md](https://doc.rust-lang.org/reference/inline-assembly.html)

*Syntax*

<sub>category: assembly · Rust rule `asm.syntax`</sub>

```
AsmArgs -> AsmAttrFormatString (`,` AsmAttrFormatString)* (`,` AsmAttrOperand)* `,`?

AsmExpression -> ( `asm` | `nakedAsm` ) `(` AsmArgs `)`

GlobalAsm -> `globalAsm` `(` AsmArgs `)`

FormatString -> STRING_LITERAL | RAW_STRING_LITERAL

AsmAttrFormatString -> (OuterAttribute)* FormatString

AsmOperand ->
      ClobberAbi
    | AsmOptions
    | RegOperand

AsmAttrOperand -> (OuterAttribute)* AsmOperand

ClobberAbi -> `clobberAbi` `(` Abi (`,` Abi)* `,`? `)`

AsmOptions ->
    `options` `(` ( AsmOption (`,` AsmOption)* `,`? )? `)`

AsmOption ->
      `pure`
    | `nomem`
    | `readonly`
    | `preservesFlags`
    | `noreturn`
    | `nostack`
    | `attSyntax`
    | `raw`

RegOperand -> (ParamName `=`)?
    (
          DirSpec `(` RegSpec `)` Expression
        | DualDirSpec `(` RegSpec `)` DualDirSpecExpression
        | `sym` PathExpression
        | `const` Expression
        | `label` `{` Statements? `}`
    )

ParamName -> IDENTIFIER_OR_KEYWORD | RAW_IDENTIFIER

DualDirSpecExpression ->
      Expression
    | Expression `=>` Expression

RegSpec -> RegisterClass | ExplicitRegister

RegisterClass -> IDENTIFIER_OR_KEYWORD

ExplicitRegister -> STRING_LITERAL

DirSpec ->
      `in`
    | `out`
    | `lateout`

DualDirSpec ->
      `inout`
    | `inlateout`
```

**Changes from Rust:**

- Changed `AsmArgs` (exception 12): No longer a root: used by `AsmExpression` and `GlobalAsm`.
- Added `AsmExpression` (exceptions 11, 12): Rust's `asm!` and `naked_asm!` as built-ins.
- Added `GlobalAsm` (exceptions 11, 12): Rust's `global_asm!` as a built-in item.
- Changed `FormatString` (exception 12): No macro invocations (Rust allows `concat!(…)` here).
- Changed `ClobberAbi` (exception 11): Go casing.
- Changed `AsmOption` (exception 11): Go casing.

## Removed from Rust

Every Rust production with no counterpart here.

| Production | Rust chapter | Why |
| --- | --- | --- |
| `AsClause` | Extern crates | Packages are imported by path; there is no `extern crate` (exception 1). |
| `BlockExpressionNoInnerAttributes` | Block expressions | Every block has no inner attributes, so this is `BlockExpression`. |
| `CfgSelect` | Conditional compilation | The `cfg_select!` macro is not one of v2's built-ins. |
| `CfgSelectArms` | Conditional compilation | The `cfg_select!` macro is not one of v2's built-ins. |
| `CfgSelectConfigurationPredicate` | Conditional compilation | The `cfg_select!` macro is not one of v2's built-ins. |
| `ClosureExpression` | Function literals | Replaced by `FunctionLiteral`. |
| `Crate` | Source files and packages | Replaced by `SourceFile`: a file belongs to its folder's package. |
| `CrateRef` | Extern crates | Packages are imported by path; there is no `extern crate` (exception 1). |
| `DeriveMacroAttributes` | Procedural macros | No user macros (exception 12). |
| `DeriveMacroName` | Procedural macros | No user macros (exception 12). |
| `ExternCrate` | Extern crates | Packages are imported by path; there is no `extern crate` (exception 1). |
| `ForLifetimes` | Trait and lifetime bounds | Replaced by `ForOrigins`. |
| `InnerAttribute` | Attributes | No inner attributes. Attributes on the `package` declaration apply to the whole package. |
| `Lifetime` | Trait and lifetime bounds | Replaced by `OriginRef`. |
| `LifetimeBounds` | Trait and lifetime bounds | Origin bounds are `OriginRef` lists joined by `+`. |
| `LifetimeParam` | Generic parameters | Replaced by `OriginParam`. |
| `LifetimeWhereClauseItem` | Generic parameters | Replaced by `OriginWhereClauseItem`. |
| `MacroFragSpec` | Macros by example | No user macros (exception 12). |
| `MacroInvocation` | Token trees | No user macros. Built-ins are ordinary calls (`println(…)`) or `vec[…]`. |
| `MacroInvocationSemi` | Token trees | No user macros. |
| `MacroItem` | Items | No user macros. |
| `MacroMatch` | Macros by example | No user macros (exception 12). |
| `MacroMatcher` | Macros by example | No user macros (exception 12). |
| `MacroRepOp` | Macros by example | No user macros (exception 12). |
| `MacroRepSep` | Macros by example | No user macros (exception 12). |
| `MacroRule` | Macros by example | No user macros (exception 12). |
| `MacroRules` | Macros by example | No user macros (exception 12). |
| `MacroRulesDef` | Macros by example | No user macros (exception 12). |
| `MacroRulesDefinition` | Macros by example | No user macros (exception 12). |
| `MacroTranscriber` | Macros by example | No user macros (exception 12). |
| `Module` | Modules | Folders are packages; there is no `mod` (exception 1). |
| `ObsoleteRangePattern` | Patterns | `a...b` patterns are invalid since Rust 2021. |
| `ProcMacroDeriveAttribute` | Procedural macros | No user macros (exception 12). |
| `UseDeclaration` | Use declarations | Packages are imported with `import`; there is no `use` (exception 1). |
| `UseTree` | Use declarations | Packages are imported with `import`; there is no `use` (exception 1). |

## Index of productions

- `Abi`: [Functions](#functions)
- `ArithmeticOrLogicalExpression`: [Operator expressions](#operator-expressions)
- `ArrayElements`: [Array and index expressions](#array-and-index-expressions)
- `ArrayExpression`: [Array and index expressions](#array-and-index-expressions)
- `ArrayType`: [Array types](#array-types)
- `ASCII`: [Input format](#input-format)
- `ASCII_ESCAPE`: [Tokens](#tokens)
- `ASCII_FOR_CHAR`: [Tokens](#tokens)
- `ASCII_FOR_RAW`: [Tokens](#tokens)
- `ASCII_FOR_STRING`: [Tokens](#tokens)
- `AsmArgs`: [Inline assembly](#inline-assembly)
- `AsmAttrFormatString`: [Inline assembly](#inline-assembly)
- `AsmAttrOperand`: [Inline assembly](#inline-assembly)
- `AsmExpression`: [Inline assembly](#inline-assembly)
- `AsmOperand`: [Inline assembly](#inline-assembly)
- `AsmOption`: [Inline assembly](#inline-assembly)
- `AsmOptions`: [Inline assembly](#inline-assembly)
- `AssignmentExpression`: [Operator expressions](#operator-expressions)
- `AssociatedItem`: [Associated items](#associated-items)
- `AssociatedItems`: [Associated items](#associated-items)
- `AsyncBlockExpression`: [Block expressions](#block-expressions)
- `Attr`: [Attributes](#attributes)
- `AttrInput`: [Attributes](#attributes)
- `AwaitExpression`: [Await expressions](#await-expressions)
- `BareFunctionReturnType`: [Function types](#function-types)
- `BareFunctionType`: [Function types](#function-types)
- `BIN_DIGIT`: [Tokens](#tokens)
- `BIN_LITERAL`: [Tokens](#tokens)
- `BLOCK_CHAR`: [Comments](#comments)
- `BLOCK_COMMENT`: [Comments](#comments)
- `BlockExpression`: [Block expressions](#block-expressions)
- `BorrowExpression`: [Operator expressions](#operator-expressions)
- `Bound`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `Bounds`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `BreakExpression`: [Loop expressions](#loop-expressions)
- `BUILTIN_NAMES`: [Keywords](#keywords)
- `BuiltinExpression`: [Expressions](#expressions)
- `BYTE_ESCAPE`: [Tokens](#tokens)
- `BYTE_LITERAL`: [Tokens](#tokens)
- `BYTE_STRING_LITERAL`: [Tokens](#tokens)
- `C_STRING_LITERAL`: [Tokens](#tokens)
- `CallExpression`: [Call expressions](#call-expressions)
- `CallParams`: [Call expressions](#call-expressions)
- `CfgAttrAttribute`: [Conditional compilation](#conditional-compilation)
- `CfgAttribute`: [Conditional compilation](#conditional-compilation)
- `CfgAttrs`: [Conditional compilation](#conditional-compilation)
- `CHAR`: [Input format](#input-format)
- `CHAR_LITERAL`: [Tokens](#tokens)
- `Class`: [Structs](#structs)
- `ClobberAbi`: [Inline assembly](#inline-assembly)
- `ClosureParam`: [Function literals](#function-literals)
- `ClosureParameters`: [Function literals](#function-literals)
- `CollapseDebuginfoAttribute`: [Debugger](#debugger)
- `CollapseDebuginfoOption`: [Debugger](#debugger)
- `COMMENT`: [Comments](#comments)
- `ComparisonExpression`: [Operator expressions](#operator-expressions)
- `CompoundAssignmentExpression`: [Operator expressions](#operator-expressions)
- `Conditions`: [If expressions](#if-expressions)
- `ConfigurationAll`: [Conditional compilation](#conditional-compilation)
- `ConfigurationAny`: [Conditional compilation](#conditional-compilation)
- `ConfigurationNot`: [Conditional compilation](#conditional-compilation)
- `ConfigurationOption`: [Conditional compilation](#conditional-compilation)
- `ConfigurationPredicate`: [Conditional compilation](#conditional-compilation)
- `ConfigurationPredicateList`: [Conditional compilation](#conditional-compilation)
- `ConstantItem`: [Constant items](#constant-items)
- `ConstBlockExpression`: [Block expressions](#block-expressions)
- `ConstParam`: [Generic parameters](#generic-parameters)
- `ContinueExpression`: [Loop expressions](#loop-expressions)
- `CR`: [Whitespace](#whitespace)
- `DEC_DIGIT`: [Tokens](#tokens)
- `DEC_LITERAL`: [Tokens](#tokens)
- `DelimTokenTree`: [Token trees](#token-trees)
- `DereferenceExpression`: [Operator expressions](#operator-expressions)
- `DirSpec`: [Inline assembly](#inline-assembly)
- `DOC_BLOCK_CHAR`: [Comments](#comments)
- `DualDirSpec`: [Inline assembly](#inline-assembly)
- `DualDirSpecExpression`: [Inline assembly](#inline-assembly)
- `Enumeration`: [Enumerations](#enumerations)
- `EnumVariant`: [Enumerations](#enumerations)
- `EnumVariantDiscriminant`: [Enumerations](#enumerations)
- `EnumVariants`: [Enumerations](#enumerations)
- `EnumVariantStruct`: [Enumerations](#enumerations)
- `EnumVariantTuple`: [Enumerations](#enumerations)
- `EOF`: [Input format](#input-format)
- `ExcludedConditions`: [If expressions](#if-expressions)
- `ExcludedMatchConditions`: [Match expressions](#match-expressions)
- `ExplicitRegister`: [Inline assembly](#inline-assembly)
- `Expression`: [Expressions](#expressions)
- `ExpressionStatement`: [Statements](#statements)
- `ExpressionWithBlock`: [Expressions](#expressions)
- `ExpressionWithBlockNoAttrs`: [Expressions](#expressions)
- `ExpressionWithoutBlock`: [Expressions](#expressions)
- `ExpressionWithoutBlockNoAttrs`: [Expressions](#expressions)
- `ExternalItem`: [External blocks](#external-blocks)
- `ExternalItems`: [External blocks](#external-blocks)
- `ExternBlock`: [External blocks](#external-blocks)
- `ExternType`: [External blocks](#external-blocks)
- `FieldExpression`: [Field access expressions](#field-access-expressions)
- `FLOAT_EXPONENT`: [Tokens](#tokens)
- `FLOAT_LITERAL`: [Tokens](#tokens)
- `FormatString`: [Inline assembly](#inline-assembly)
- `ForOrigins`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `Function`: [Functions](#functions)
- `FunctionLiteral`: [Function literals](#function-literals)
- `FunctionParam`: [Functions](#functions)
- `FunctionParameters`: [Functions](#functions)
- `FunctionParametersMaybeNamedVariadic`: [Function types](#function-types)
- `FunctionParamPattern`: [Functions](#functions)
- `FunctionQualifiers`: [Functions](#functions)
- `FunctionReturnType`: [Functions](#functions)
- `FunctionTypeQualifiers`: [Function types](#function-types)
- `FunctionValueType`: [Function types](#function-types)
- `GenericArg`: [Paths](#paths)
- `GenericArgList`: [Paths](#paths)
- `GenericArgs`: [Paths](#paths)
- `GenericArgsBinding`: [Paths](#paths)
- `GenericArgsBounds`: [Paths](#paths)
- `GenericArgsConst`: [Paths](#paths)
- `GenericParam`: [Generic parameters](#generic-parameters)
- `GenericParams`: [Generic parameters](#generic-parameters)
- `GlobalAsm`: [Inline assembly](#inline-assembly)
- `GroupedExpression`: [Grouped expressions](#grouped-expressions)
- `GroupedPattern`: [Patterns](#patterns)
- `HEX_DIGIT`: [Tokens](#tokens)
- `HEX_LITERAL`: [Tokens](#tokens)
- `IDENTIFIER`: [Identifiers](#identifiers)
- `IDENTIFIER_OR_KEYWORD`: [Identifiers](#identifiers)
- `IdentifierPattern`: [Patterns](#patterns)
- `IfExpression`: [If expressions](#if-expressions)
- `Implementation`: [Implementations](#implementations)
- `ImplTraitType`: [Impl trait type](#impl-trait-type)
- `ImplTraitTypeOneBound`: [Impl trait type](#impl-trait-type)
- `ImportDeclaration`: [Source files and packages](#source-files-and-packages)
- `ImportSpec`: [Source files and packages](#source-files-and-packages)
- `IndexExpression`: [Array and index expressions](#array-and-index-expressions)
- `InferredType`: [Inferred type](#inferred-type)
- `InfiniteLoopExpression`: [Loop expressions](#loop-expressions)
- `InherentImpl`: [Implementations](#implementations)
- `InlineAttribute`: [Code generation](#code-generation)
- `INNER_BLOCK_DOC`: [Comments](#comments)
- `INNER_LINE_DOC`: [Comments](#comments)
- `INTEGER_LITERAL`: [Tokens](#tokens)
- `Item`: [Items](#items)
- `ItemList`: [Source files and packages](#source-files-and-packages)
- `ItemSafety`: [Functions](#functions)
- `IteratorLoopExpression`: [Loop expressions](#loop-expressions)
- `LabelBlockExpression`: [Loop expressions](#loop-expressions)
- `LazyBooleanExpression`: [Operator expressions](#operator-expressions)
- `LetChain`: [If expressions](#if-expressions)
- `LetChainCondition`: [If expressions](#if-expressions)
- `LetStatement`: [Statements](#statements)
- `LF`: [Whitespace](#whitespace)
- `LIFETIME_OR_LABEL`: [Tokens](#tokens)
- `LIFETIME_TOKEN`: [Tokens](#tokens)
- `LINE_COMMENT`: [Comments](#comments)
- `LINE_DOC_COMMENT_CONTENT`: [Comments](#comments)
- `LINE_END`: [Whitespace](#whitespace)
- `LiteralExpression`: [Literal expressions](#literal-expressions)
- `LiteralPattern`: [Patterns](#patterns)
- `LoopExpression`: [Loop expressions](#loop-expressions)
- `LoopLabel`: [Loop expressions](#loop-expressions)
- `MatchArm`: [Match expressions](#match-expressions)
- `MatchArmGuard`: [Match expressions](#match-expressions)
- `MatchArms`: [Match expressions](#match-expressions)
- `MatchArmWithBody`: [Match expressions](#match-expressions)
- `MatchConditions`: [Match expressions](#match-expressions)
- `MatchExpression`: [Match expressions](#match-expressions)
- `MatchGuardChain`: [Match expressions](#match-expressions)
- `MatchGuardCondition`: [Match expressions](#match-expressions)
- `MatchGuardScrutinee`: [Match expressions](#match-expressions)
- `MaybeNamedFunctionParameters`: [Function types](#function-types)
- `MaybeNamedFunctionParametersVariadic`: [Function types](#function-types)
- `MaybeNamedParam`: [Function types](#function-types)
- `MetaItem`: [Attributes](#attributes)
- `MetaItemInner`: [Attributes](#attributes)
- `MetaListIdents`: [Attributes](#attributes)
- `MetaListNameValueStr`: [Attributes](#attributes)
- `MetaListPaths`: [Attributes](#attributes)
- `MetaNameValueStr`: [Attributes](#attributes)
- `MetaSeq`: [Attributes](#attributes)
- `MetaWord`: [Attributes](#attributes)
- `MethodCallExpression`: [Method call expressions](#method-call-expressions)
- `ModernRangePattern`: [Patterns](#patterns)
- `NegationExpression`: [Operator expressions](#operator-expressions)
- `NESTED_BLOCK_DOC_COMMENT`: [Comments](#comments)
- `NeverType`: [Never type](#never-type)
- `NON_KEYWORD_IDENTIFIER`: [Identifiers](#identifiers)
- `NUL`: [Input format](#input-format)
- `OCT_DIGIT`: [Tokens](#tokens)
- `OCT_LITERAL`: [Tokens](#tokens)
- `OperatorExpression`: [Operator expressions](#operator-expressions)
- `OptimizeAttribute`: [Code generation](#code-generation)
- `Origin`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `OriginParam`: [Generic parameters](#generic-parameters)
- `OriginRef`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `OriginWhereClauseItem`: [Generic parameters](#generic-parameters)
- `OUTER_BLOCK_DOC`: [Comments](#comments)
- `OUTER_LINE_DOC`: [Comments](#comments)
- `OuterAttribute`: [Attributes](#attributes)
- `PackageDeclaration`: [Source files and packages](#source-files-and-packages)
- `ParamName`: [Inline assembly](#inline-assembly)
- `ParenthesizedType`: [Types](#types)
- `PathExpression`: [Path expressions](#path-expressions)
- `PathExprSegment`: [Paths](#paths)
- `PathIdentSegment`: [Paths](#paths)
- `PathInExpression`: [Paths](#paths)
- `PathPattern`: [Patterns](#patterns)
- `Pattern`: [Patterns](#patterns)
- `PatternNoTopAlt`: [Patterns](#patterns)
- `PatternWithoutModernRange`: [Patterns](#patterns)
- `PredicateLoopExpression`: [Loop expressions](#loop-expressions)
- `PUNCTUATION`: [Tokens](#tokens)
- `QualifiedPathInExpression`: [Paths](#paths)
- `QualifiedPathInType`: [Paths](#paths)
- `QualifiedPathType`: [Paths](#paths)
- `QUOTE_ESCAPE`: [Tokens](#tokens)
- `RangeExclusivePattern`: [Patterns](#patterns)
- `RangeExpr`: [Range expressions](#range-expressions)
- `RangeExpression`: [Range expressions](#range-expressions)
- `RangeFromExpr`: [Range expressions](#range-expressions)
- `RangeFromPattern`: [Patterns](#patterns)
- `RangeFullExpr`: [Range expressions](#range-expressions)
- `RangeInclusiveExpr`: [Range expressions](#range-expressions)
- `RangeInclusivePattern`: [Patterns](#patterns)
- `RangePatternBound`: [Patterns](#patterns)
- `RangeToExclusivePattern`: [Patterns](#patterns)
- `RangeToExpr`: [Range expressions](#range-expressions)
- `RangeToInclusiveExpr`: [Range expressions](#range-expressions)
- `RangeToInclusivePattern`: [Patterns](#patterns)
- `RAW_BYTE_STRING_CONTENT`: [Tokens](#tokens)
- `RAW_BYTE_STRING_CONTENT_HASHED`: [Tokens](#tokens)
- `RAW_BYTE_STRING_LITERAL`: [Tokens](#tokens)
- `RAW_C_STRING_CONTENT`: [Tokens](#tokens)
- `RAW_C_STRING_CONTENT_HASHED`: [Tokens](#tokens)
- `RAW_C_STRING_LITERAL`: [Tokens](#tokens)
- `RAW_IDENTIFIER`: [Identifiers](#identifiers)
- `RAW_LIFETIME`: [Tokens](#tokens)
- `RAW_STRING_CONTENT`: [Tokens](#tokens)
- `RAW_STRING_CONTENT_HASHED`: [Tokens](#tokens)
- `RAW_STRING_LITERAL`: [Tokens](#tokens)
- `RawPointerType`: [Pointer types](#pointer-types)
- `Receiver`: [Functions](#functions)
- `ReferencePattern`: [Patterns](#patterns)
- `ReferenceType`: [Pointer types](#pointer-types)
- `RegisterClass`: [Inline assembly](#inline-assembly)
- `RegOperand`: [Inline assembly](#inline-assembly)
- `RegSpec`: [Inline assembly](#inline-assembly)
- `RESERVED_FLOAT`: [Tokens](#tokens)
- `RESERVED_GUARDED_STRING_LITERAL`: [Tokens](#tokens)
- `RESERVED_KEYWORDS`: [Keywords](#keywords)
- `RESERVED_POUNDS`: [Tokens](#tokens)
- `RESERVED_RAW_IDENTIFIER`: [Identifiers](#identifiers)
- `RESERVED_RAW_LIFETIME`: [Tokens](#tokens)
- `RESERVED_TOKEN`: [Tokens](#tokens)
- `RESERVED_TOKEN_DOUBLE_QUOTE`: [Tokens](#tokens)
- `RESERVED_TOKEN_LIFETIME`: [Tokens](#tokens)
- `RESERVED_TOKEN_POUND`: [Tokens](#tokens)
- `RESERVED_TOKEN_SINGLE_QUOTE`: [Tokens](#tokens)
- `RestPattern`: [Patterns](#patterns)
- `ReturnExpression`: [Return expressions](#return-expressions)
- `Scrutinee`: [Match expressions](#match-expressions)
- `SelfParam`: [Functions](#functions)
- `Separation`: [Whitespace](#whitespace)
- `SHEBANG`: [Shebang](#shebang)
- `SHEBANG_BLOCK_COMMENT`: [Shebang](#shebang)
- `SHEBANG_NESTED_BLOCK_COMMENT`: [Shebang](#shebang)
- `ShorthandSelf`: [Functions](#functions)
- `SimplePath`: [Paths](#paths)
- `SimplePathSegment`: [Paths](#paths)
- `SlicePattern`: [Patterns](#patterns)
- `SlicePatternItems`: [Patterns](#patterns)
- `SliceType`: [Slice types](#slice-types)
- `SourceFile`: [Source files and packages](#source-files-and-packages)
- `SP`: [Whitespace](#whitespace)
- `Statement`: [Statements](#statements)
- `Statements`: [Block expressions](#block-expressions)
- `StaticItem`: [Static items](#static-items)
- `STRICT_KEYWORDS`: [Keywords](#keywords)
- `STRING_CONTINUE`: [Tokens](#tokens)
- `STRING_LITERAL`: [Tokens](#tokens)
- `Struct`: [Structs](#structs)
- `StructBase`: [Struct expressions](#struct-expressions)
- `StructExpression`: [Struct expressions](#struct-expressions)
- `StructExprField`: [Struct expressions](#struct-expressions)
- `StructExprFields`: [Struct expressions](#struct-expressions)
- `StructField`: [Structs](#structs)
- `StructFields`: [Structs](#structs)
- `StructPattern`: [Patterns](#patterns)
- `StructPatternElements`: [Patterns](#patterns)
- `StructPatternEtCetera`: [Patterns](#patterns)
- `StructPatternField`: [Patterns](#patterns)
- `StructPatternFields`: [Patterns](#patterns)
- `StructStruct`: [Structs](#structs)
- `SUFFIX`: [Tokens](#tokens)
- `TAB`: [Whitespace](#whitespace)
- `Terminator`: [Whitespace](#whitespace)
- `Token`: [Tokens](#tokens)
- `TokenTree`: [Token trees](#token-trees)
- `Trait`: [Traits](#traits)
- `TraitBound`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `TraitImpl`: [Implementations](#implementations)
- `TraitObjectType`: [Trait object types](#trait-object-types)
- `TraitObjectTypeOneBound`: [Trait object types](#trait-object-types)
- `TryPropagationExpression`: [Operator expressions](#operator-expressions)
- `TUPLE_INDEX`: [Tokens](#tokens)
- `TupleElements`: [Tuple and index expressions](#tuple-and-index-expressions)
- `TupleExpression`: [Tuple and index expressions](#tuple-and-index-expressions)
- `TupleField`: [Structs](#structs)
- `TupleFields`: [Structs](#structs)
- `TupleIndexingExpression`: [Tuple and index expressions](#tuple-and-index-expressions)
- `TuplePattern`: [Patterns](#patterns)
- `TuplePatternItems`: [Patterns](#patterns)
- `TupleStruct`: [Structs](#structs)
- `TupleStructItems`: [Patterns](#patterns)
- `TupleStructPattern`: [Patterns](#patterns)
- `TupleType`: [Tuple types](#tuple-types)
- `Type`: [Types](#types)
- `TypeAlias`: [Type aliases](#type-aliases)
- `TypeBoundWhereClauseItem`: [Generic parameters](#generic-parameters)
- `TypeCastExpression`: [Operator expressions](#operator-expressions)
- `TypedSelf`: [Functions](#functions)
- `TypeList`: [Paths](#paths)
- `TypeNoBounds`: [Types](#types)
- `TypeParam`: [Generic parameters](#generic-parameters)
- `TypePath`: [Paths](#paths)
- `TypePathSegment`: [Paths](#paths)
- `UnderscoreExpression`: [Underscore expressions](#underscore-expressions)
- `UNICODE_ESCAPE`: [Tokens](#tokens)
- `Union`: [Unions](#unions)
- `UnsafeBlockExpression`: [Block expressions](#block-expressions)
- `UseBound`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `UseBoundGenericArg`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `UseBoundGenericArgs`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `VecExpression`: [Expressions](#expressions)
- `Visibility`: [Visibility and privacy](#visibility-and-privacy)
- `VisItem`: [Items](#items)
- `WEAK_KEYWORDS`: [Keywords](#keywords)
- `WhereClause`: [Generic parameters](#generic-parameters)
- `WhereClauseItem`: [Generic parameters](#generic-parameters)
- `WHITESPACE`: [Whitespace](#whitespace)
- `WildcardPattern`: [Patterns](#patterns)
- `XID_Continue`: [Identifiers](#identifiers)
- `XID_Start`: [Identifiers](#identifiers)

