# Rust grammar

The complete grammar of the Rust language in one file: every lexical and syntactic production from **The Rust Reference**, the Rust project's reference for the language, in the Reference's own order and notation.

| | |
| --- | --- |
| Source | [rust-lang/reference](https://github.com/rust-lang/reference), `src/**/*.md`, every ```` ```grammar ```` block |
| Commit | [`a2ae66d7037c`](https://github.com/rust-lang/reference/tree/a2ae66d7037cbedac68746b9510b24cc34b96d69) (2026-11-04) |
| Productions | 352 |
| Online | [doc.rust-lang.org/reference](https://doc.rust-lang.org/reference/), and its [grammar summary](https://doc.rust-lang.org/reference/grammar.html) |
| License | The Rust Reference is dual-licensed under the MIT and Apache-2.0 licenses. The productions below are copied from it unchanged. Copyright The Rust Project Developers. |

**Scope.** This is the grammar only: what Rust programs may look like. What they mean (types, borrowing, evaluation order) is in the prose of the Reference, which each chapter below links to. The grammar covers the current edition (2024). Earlier-edition differences appear as notes under the productions they affect.

**Other specifications.** The Rust project also hosts the Ferrocene Language Specification ([rust-lang/fls](https://github.com/rust-lang/fls)), a second, independently written description of the language. This file follows the Reference, whose repository includes a tool (`tools/grammar-check`) that compares its grammar with rustc's own lexer and parser.

**How it was made.** Every ```` ```grammar ```` block in the Reference's `src/**/*.md`, in the order of its `SUMMARY.md`, copied with the notes its productions reference. Only the notation table and this header are written by hand. To update, re-run the extraction against a newer commit of the Reference.

## Contents

- [Notation](#notation)
- [Lexical structure](#lexical-structure)
  - [Input format](#input-format)
  - [Shebang](#shebang)
  - [Keywords](#keywords)
  - [Identifiers](#identifiers)
  - [Comments](#comments)
  - [Whitespace](#whitespace)
  - [Tokens](#tokens)
- [Macros](#macros)
  - [Macros by example](#macros-by-example)
  - [Procedural macros](#procedural-macros)
- [Crates and source files](#crates-and-source-files)
- [Conditional compilation](#conditional-compilation)
- [Items](#items)
  - [Modules](#modules)
  - [Extern crates](#extern-crates)
  - [Use declarations](#use-declarations)
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
    - [Closure expressions](#closure-expressions)
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
    - [Function pointer types](#function-pointer-types)
    - [Trait object types](#trait-object-types)
    - [Impl trait type](#impl-trait-type)
    - [Inferred type](#inferred-type)
  - [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- [Names](#names)
  - [Paths](#paths)
  - [Visibility and privacy](#visibility-and-privacy)
- [Inline assembly](#inline-assembly)
- [Index of productions](#index-of-productions)

## Notation

Productions are written in the Reference's grammar source notation:

| Notation | Meaning |
| --- | --- |
| `Name -> Expression` | A production: *Name* is defined as *Expression*. Continuation lines are indented. |
| `@root Name -> …` | A root production: not used by any other production (an entry point such as *Crate* or a token class). |
| `UPPER_CASE` | A token produced by the lexer, such as `IDENTIFIER` or `INTEGER_LITERAL`. |
| `CamelCase` | A syntactic production, such as `LetStatement` or `Item`. |
| `` `text` `` | Exactly these characters. |
| `A B` | Sequence: *A* then *B*. Sequence binds tighter than `\|`. |
| `A \| B` | Ordered choice: the first alternative that matches is taken. |
| `( … )` | Grouping. |
| `X?`, `X*`, `X+` | Optional, zero or more, one or more. |
| `X{a..b}`, `X{a..=b}` | Between *a* and *b* repetitions, exclusive or inclusive of *b*. |
| `X{n:a..=b}`, `X{n}` | A counted repetition bound to *n*, and a later repetition of exactly *n* (used by raw strings). |
| `!X` | Negative lookahead: matches only if *X* does not follow, consuming nothing. |
| `` [`a` `b`] ``, `` [`a`-`z`] `` | Any one of the listed characters, or any character in the range. |
| `` ~[ … ] ``, `` ~`text` `` | Any character except those listed, or except this sequence. |
| `A ^ B` | Hard cut: once *A* has matched, *B* must match or parsing fails with no backtracking. |
| `U+0060` | A single Unicode code point. |
| `<text>` | An English description of what matches. |
| `X _except y_` | A modification of the preceding rule, written in italics between underscores. |
| `X[^name]` | A note on *X*; its text is listed under the block as `[^name]`. |
| `// text` | A comment. |

Keywords, operators and punctuation that appear in backticks inside productions refer to the token tables in *Keywords* and *Tokens* below.

## Lexical structure

### Input format

Reference: [input-format.md](https://doc.rust-lang.org/reference/input-format.html)

<sub>category: lexer · `input.syntax`</sub>

```
CHAR -> [U+0000-U+D7FF U+E000-U+10FFFF] // a Unicode scalar value

ASCII -> [U+0000-U+007F]

NUL -> U+0000

EOF -> !CHAR  // End of file or input
```

### Shebang

Reference: [shebang.md](https://doc.rust-lang.org/reference/shebang.html)

<sub>category: lexer · `shebang.syntax`</sub>

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

Reference: [keywords.md](https://doc.rust-lang.org/reference/keywords.html)

*Strict keywords*

<sub>category: lexer · `lex.keywords.strict.syntax`</sub>

```
@root STRICT_KEYWORDS ->
      `_`
    | `as`
    | `async`
    | `await`
    | `break`
    | `const`
    | `continue`
    | `crate`
    | `dyn`
    | `else`
    | `enum`
    | `extern`
    | `false`
    | `fn`
    | `for`
    | `if`
    | `impl`
    | `in`
    | `let`
    | `loop`
    | `match`
    | `mod`
    | `move`
    | `mut`
    | `pub`
    | `ref`
    | `return`
    | `self`
    | `Self`
    | `static`
    | `struct`
    | `super`
    | `trait`
    | `true`
    | `type`
    | `unsafe`
    | `use`
    | `where`
    | `while`
```

*Reserved keywords*

<sub>category: lexer · `lex.keywords.reserved.syntax`</sub>

```
@root RESERVED_KEYWORDS ->
      `abstract`
    | `become`
    | `box`
    | `do`
    | `final`
    | `gen`
    | `macro`
    | `override`
    | `priv`
    | `try`
    | `typeof`
    | `unsized`
    | `virtual`
    | `yield`
```

*Weak keywords*

<sub>category: lexer · `lex.keywords.weak.syntax`</sub>

```
@root WEAK_KEYWORDS ->
      `'static`
    | `macro_rules`
    | `raw`
    | `safe`
    | `union`
```

### Identifiers

Reference: [identifiers.md](https://doc.rust-lang.org/reference/identifiers.html)

<sub>category: lexer · `ident.syntax`</sub>

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

Reference: [comments.md](https://doc.rust-lang.org/reference/comments.html)

<sub>category: lexer · `comments.syntax`</sub>

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

Reference: [whitespace.md](https://doc.rust-lang.org/reference/whitespace.html)

<sub>category: lexer · `whitespace.syntax`</sub>

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
```

### Tokens

Reference: [tokens.md](https://doc.rust-lang.org/reference/tokens.html)

<sub>category: lexer · `lex.token.syntax`</sub>

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

<sub>category: lexer · `lex.token.literal.suffix.syntax`</sub>

```
SUFFIX ->
      `_` ^ XID_Continue+
    | XID_Start XID_Continue*
```

*Character literals*

<sub>category: lexer · `lex.token.literal.char.syntax`</sub>

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

<sub>category: lexer · `lex.token.literal.str.syntax`</sub>

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

<sub>category: lexer · `lex.token.literal.str-raw.syntax`</sub>

```
RAW_STRING_LITERAL ->
      `r` `"` ^ RAW_STRING_CONTENT `"` SUFFIX?
    | `r` `#`{n:1..=255} ^ `"` RAW_STRING_CONTENT_HASHED `"` `#`{n} SUFFIX?

RAW_STRING_CONTENT -> (!`"` ~CR )*

RAW_STRING_CONTENT_HASHED -> (!(`"` `#`{n}) ~CR )*
```

*Byte literals*

<sub>category: lexer · `lex.token.byte.syntax`</sub>

```
BYTE_LITERAL ->
    `b'` ^ ( ASCII_FOR_CHAR | BYTE_ESCAPE )  `'` SUFFIX?

ASCII_FOR_CHAR -> ![`'` `\` LF CR TAB] ASCII

BYTE_ESCAPE ->
      `\x` HEX_DIGIT HEX_DIGIT
    | `\n` | `\r` | `\t` | `\\` | `\0` | `\'` | `\"`
```

*Byte string literals*

<sub>category: lexer · `lex.token.str-byte.syntax`</sub>

```
BYTE_STRING_LITERAL ->
    `b"` ^ ( ASCII_FOR_STRING | BYTE_ESCAPE | STRING_CONTINUE )* `"` SUFFIX?

ASCII_FOR_STRING -> ![`"` `\` CR] ASCII
```

*Raw byte string literals*

<sub>category: lexer · `lex.token.str-byte-raw.syntax`</sub>

```
RAW_BYTE_STRING_LITERAL ->
      `br` `"` ^ RAW_BYTE_STRING_CONTENT `"` SUFFIX?
    | `br` `#`{n:1..=255} ^ `"` RAW_BYTE_STRING_CONTENT_HASHED `"` `#`{n} SUFFIX?

RAW_BYTE_STRING_CONTENT -> (!`"` ASCII_FOR_RAW )*

RAW_BYTE_STRING_CONTENT_HASHED -> (!(`"` `#`{n}) ASCII_FOR_RAW )*

ASCII_FOR_RAW -> !CR ASCII
```

*C string literals*

<sub>category: lexer · `lex.token.str-c.syntax`</sub>

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

<sub>category: lexer · `lex.token.str-c-raw.syntax`</sub>

```
RAW_C_STRING_LITERAL ->
      `cr` `"` ^ RAW_C_STRING_CONTENT `"` SUFFIX?
    | `cr` `#`{n:1..=255} ^ `"` RAW_C_STRING_CONTENT_HASHED `"` `#`{n} SUFFIX?

RAW_C_STRING_CONTENT -> (!`"` ~[CR NUL] )*

RAW_C_STRING_CONTENT_HASHED -> (!(`"` `#`{n}) ~[CR NUL] )*
```

*Integer literals*

<sub>category: lexer · `lex.token.literal.int.syntax`</sub>

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

<sub>category: lexer · `lex.token.literal.int.tuple-field.syntax`</sub>

```
TUPLE_INDEX -> DEC_LITERAL | BIN_LITERAL | OCT_LITERAL | HEX_LITERAL
```

*Floating-point literals*

<sub>category: lexer · `lex.token.literal.float.syntax`</sub>

```
FLOAT_LITERAL ->
      DEC_LITERAL (`.` DEC_LITERAL)? FLOAT_EXPONENT SUFFIX?
    | DEC_LITERAL `.` DEC_LITERAL SUFFIX?
    | DEC_LITERAL `.` !(`.` | `_` | XID_Start)

FLOAT_EXPONENT ->
    (`e`|`E`) ^ (`+`|`-`)? `_`* DEC_DIGIT (DEC_DIGIT|`_`)*
```

*Lifetimes and loop labels*

<sub>category: lexer · `lex.token.life.syntax`</sub>

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

<sub>category: lexer · `lex.token.punct.syntax`</sub>

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
    | `::`
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
    | `#`
    | `$`
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

*Reserved tokens*

<sub>category: lexer · `lex.token.reserved.syntax`</sub>

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

<sub>category: lexer · `lex.token.reserved-prefix.syntax`</sub>

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

<sub>category: lexer · `lex.token.reserved-guards.syntax`</sub>

```
RESERVED_GUARDED_STRING_LITERAL -> `#`+ STRING_LITERAL

RESERVED_POUNDS -> `#`{2..}
```

## Macros

Reference: [macros.md](https://doc.rust-lang.org/reference/macros.html)

*Macro invocation*

<sub>category: macros · `macro.invocation.syntax`</sub>

```
MacroInvocation ->
    SimplePath `!` DelimTokenTree

DelimTokenTree ->
      `(` TokenTree* `)`
    | `[` TokenTree* `]`
    | `{` TokenTree* `}`

TokenTree ->
    Token _except [delimiters][lex.token.delim]_ | DelimTokenTree

MacroInvocationSemi ->
      SimplePath `!` `(` TokenTree* `)` `;`
    | SimplePath `!` `[` TokenTree* `]` `;`
    | SimplePath `!` `{` TokenTree* `}`
```

### Macros by example

Reference: [macros-by-example.md](https://doc.rust-lang.org/reference/macros-by-example.html)

<sub>category: macros · `macro.decl.syntax`</sub>

```
MacroRulesDefinition ->
    `macro_rules` `!` IDENTIFIER MacroRulesDef

MacroRulesDef ->
      `(` MacroRules `)` `;`
    | `[` MacroRules `]` `;`
    | `{` MacroRules `}`

MacroRules ->
    MacroRule ( `;` MacroRule )* `;`?

MacroRule ->
    MacroMatcher `=>` MacroTranscriber

MacroMatcher ->
      `(` MacroMatch* `)`
    | `[` MacroMatch* `]`
    | `{` MacroMatch* `}`

MacroMatch ->
      Token _except `$` and [delimiters][lex.token.delim]_
    | MacroMatcher
    | `$` ( IDENTIFIER_OR_KEYWORD _except `crate`_ | RAW_IDENTIFIER ) `:` MacroFragSpec
    | `$` `(` MacroMatch+ `)` MacroRepSep? MacroRepOp

MacroFragSpec ->
      `block` | `expr` | `expr_2021` | `ident` | `item` | `lifetime` | `literal`
    | `meta` | `pat` | `pat_param` | `path` | `stmt` | `tt` | `ty` | `vis`

MacroRepSep -> Token _except [delimiters][lex.token.delim] and [MacroRepOp]_

MacroRepOp -> `*` | `+` | `?`

MacroTranscriber -> DelimTokenTree
```

### Procedural macros

Reference: [procedural-macros.md](https://doc.rust-lang.org/reference/procedural-macros.html)

*The `proc_macro_derive` attribute*

<sub>category: attributes · `macro.proc.derive.syntax`</sub>

```
@root ProcMacroDeriveAttribute ->
    `proc_macro_derive` `(` DeriveMacroName ( `,` DeriveMacroAttributes )? `,`? `)`

DeriveMacroName -> IDENTIFIER

DeriveMacroAttributes ->
    `attributes` `(` ( IDENTIFIER (`,` IDENTIFIER)* `,`?)? `)`
```

## Crates and source files

Reference: [crates-and-source-files.md](https://doc.rust-lang.org/reference/crates-and-source-files.html)

<sub>category: items · `crate.syntax`</sub>

```
@root Crate ->
    InnerAttribute*
    Item*
```

## Conditional compilation

Reference: [conditional-compilation.md](https://doc.rust-lang.org/reference/conditional-compilation.html)

<sub>category: configuration · `cfg.syntax`</sub>

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

<sub>category: configuration · `cfg.attr.syntax`</sub>

```
@root CfgAttribute -> `cfg` `(` ConfigurationPredicate `)`
```

*The `cfg_attr` attribute*

<sub>category: configuration · `cfg.cfg_attr.syntax`</sub>

```
@root CfgAttrAttribute -> `cfg_attr` `(` ConfigurationPredicate `,` CfgAttrs? `)`

CfgAttrs -> Attr (`,` Attr)* `,`?
```

*The `cfg_select` macro*

<sub>category: configuration · `cfg.cfg_select.syntax`</sub>

```
@root CfgSelect -> CfgSelectArms?

CfgSelectArms ->
    CfgSelectConfigurationPredicate `=>`
    (
        `{` ^ TokenTree `}` `,`? CfgSelectArms?
      | ExpressionWithBlockNoAttrs `,`? CfgSelectArms?
      | ExpressionWithoutBlockNoAttrs ( `,` CfgSelectArms? )?
    )

CfgSelectConfigurationPredicate ->
    ConfigurationPredicate | `_`
```

## Items

Reference: [items.md](https://doc.rust-lang.org/reference/items.html)

<sub>category: items · `items.syntax`</sub>

```
Item ->
    OuterAttribute* ( VisItem | MacroItem )

VisItem ->
    Visibility?
    (
        Module
      | ExternCrate
      | UseDeclaration
      | Function
      | TypeAlias
      | Struct
      | Enumeration
      | Union
      | ConstantItem
      | StaticItem
      | Trait
      | Implementation
      | ExternBlock
    )

MacroItem ->
      MacroInvocationSemi
    | MacroRulesDefinition
```

### Modules

Reference: [items/modules.md](https://doc.rust-lang.org/reference/items/modules.html)

<sub>category: items · `items.mod.syntax`</sub>

```
Module ->
      `unsafe`? `mod` IDENTIFIER `;`
    | `unsafe`? `mod` IDENTIFIER `{`
        InnerAttribute*
        Item*
      `}`
```

### Extern crates

Reference: [items/extern-crates.md](https://doc.rust-lang.org/reference/items/extern-crates.html)

*Extern crate declarations*

<sub>category: items · `items.extern-crate.syntax`</sub>

```
ExternCrate -> `extern` `crate` CrateRef AsClause? `;`

CrateRef -> IDENTIFIER | `self`

AsClause -> `as` ( IDENTIFIER | `_` )
```

### Use declarations

Reference: [items/use-declarations.md](https://doc.rust-lang.org/reference/items/use-declarations.html)

<sub>category: items · `items.use.syntax`</sub>

```
UseDeclaration -> `use` UseTree `;`

UseTree ->
      (SimplePath? `::`)? `*`
    | (SimplePath? `::`)? `{` (UseTree ( `,`  UseTree )* `,`?)? `}`
    | SimplePath ( `as` ( IDENTIFIER | `_` ) )?
```

### Functions

Reference: [items/functions.md](https://doc.rust-lang.org/reference/items/functions.html)

<sub>category: items · `items.fn.syntax`</sub>

```
Function ->
    FunctionQualifiers `fn` IDENTIFIER GenericParams?
        `(` FunctionParameters? `)`
        FunctionReturnType? WhereClause?
        ( BlockExpression | `;` )

FunctionQualifiers -> `const`? `async`?[^async-edition] ItemSafety?[^extern-qualifiers] (`extern` Abi?)?

ItemSafety -> `safe`[^extern-safe] | `unsafe`

Abi -> STRING_LITERAL | RAW_STRING_LITERAL

FunctionParameters ->
      SelfParam `,`?
    | (SelfParam `,`)? FunctionParam (`,` FunctionParam)* `,`?

SelfParam -> OuterAttribute* ( ShorthandSelf | TypedSelf )

ShorthandSelf -> (`&` | `&` Lifetime)? `mut`? `self`

TypedSelf -> `mut`? `self` `:` Type

FunctionParam -> OuterAttribute* ( FunctionParamPattern | `...` | Type[^fn-param-2015] )

FunctionParamPattern -> PatternNoTopAlt `:` ( Type | `...` )

FunctionReturnType -> `->` Type
```

- `[^async-edition]`: The `async` qualifier is not allowed in the 2015 edition.
- `[^extern-qualifiers]`: *Relevant to editions earlier than Rust 2024*: Within `extern` blocks, the `safe` or `unsafe` function qualifier is only allowed when the `extern` is qualified as `unsafe`.
- `[^extern-safe]`: The `safe` function qualifier is only allowed semantically within `extern` blocks.
- `[^fn-param-2015]`: Function parameters with only a type are only allowed in an associated function of a trait item in the 2015 edition.

### Type aliases

Reference: [items/type-aliases.md](https://doc.rust-lang.org/reference/items/type-aliases.html)

<sub>category: items · `items.type.syntax`</sub>

```
TypeAlias ->
    `type` IDENTIFIER GenericParams? ( `:` Bounds? )?
        WhereClause?
        ( `=` Type WhereClause?)? `;`
```

### Structs

Reference: [items/structs.md](https://doc.rust-lang.org/reference/items/structs.html)

<sub>category: items · `items.struct.syntax`</sub>

```
Struct ->
      StructStruct
    | TupleStruct

StructStruct ->
    `struct` IDENTIFIER GenericParams? WhereClause? ( `{` StructFields? `}` | `;` )

TupleStruct ->
    `struct` IDENTIFIER GenericParams? `(` TupleFields? `)` WhereClause? `;`

StructFields -> StructField (`,` StructField)* `,`?

StructField -> OuterAttribute* Visibility? IDENTIFIER `:` Type

TupleFields -> TupleField (`,` TupleField)* `,`?

TupleField -> OuterAttribute* Visibility? Type
```

### Enumerations

Reference: [items/enumerations.md](https://doc.rust-lang.org/reference/items/enumerations.html)

<sub>category: items · `items.enum.syntax`</sub>

```
Enumeration ->
    `enum` IDENTIFIER GenericParams? WhereClause? `{` EnumVariants? `}`

EnumVariants -> EnumVariant ( `,` EnumVariant )* `,`?

EnumVariant ->
    OuterAttribute* Visibility?
    IDENTIFIER ( EnumVariantTuple | EnumVariantStruct )? EnumVariantDiscriminant?

EnumVariantTuple -> `(` TupleFields? `)`

EnumVariantStruct -> `{` StructFields? `}`

EnumVariantDiscriminant -> `=` Expression
```

### Unions

Reference: [items/unions.md](https://doc.rust-lang.org/reference/items/unions.html)

<sub>category: items · `items.union.syntax`</sub>

```
Union ->
    `union` IDENTIFIER GenericParams? WhereClause? `{` StructFields? `}`
```

### Constant items

Reference: [items/constant-items.md](https://doc.rust-lang.org/reference/items/constant-items.html)

<sub>category: items · `items.const.syntax`</sub>

```
ConstantItem ->
    `const` ( IDENTIFIER | `_` ) `:` Type ( `=` Expression )? `;`
```

### Static items

Reference: [items/static-items.md](https://doc.rust-lang.org/reference/items/static-items.html)

<sub>category: items · `items.static.syntax`</sub>

```
StaticItem ->
    ItemSafety?[^extern-safety] `static` `mut`? IDENTIFIER `:` Type ( `=` Expression )? `;`
```

- `[^extern-safety]`: The `safe` and `unsafe` function qualifiers are only allowed semantically within `extern` blocks.

### Traits

Reference: [items/traits.md](https://doc.rust-lang.org/reference/items/traits.html)

<sub>category: items · `items.traits.syntax`</sub>

```
Trait ->
    `unsafe`? `trait` IDENTIFIER GenericParams? ( `:` Bounds? )? WhereClause?
    `{`
        InnerAttribute*
        AssociatedItem*
    `}`
```

### Implementations

Reference: [items/implementations.md](https://doc.rust-lang.org/reference/items/implementations.html)

<sub>category: items · `items.impl.syntax`</sub>

```
Implementation -> InherentImpl | TraitImpl

InherentImpl ->
    `impl` GenericParams? Type WhereClause? `{`
        InnerAttribute*
        AssociatedItem*
    `}`

TraitImpl ->
    `unsafe`? `impl` GenericParams? `!`? TypePath `for` Type
    WhereClause?
    `{`
        InnerAttribute*
        AssociatedItem*
    `}`
```

### External blocks

Reference: [items/external-blocks.md](https://doc.rust-lang.org/reference/items/external-blocks.html)

<sub>category: items · `items.extern.syntax`</sub>

```
ExternBlock ->
    `unsafe`?[^unsafe-2024] `extern` Abi? `{`
        InnerAttribute*
        ExternalItem*
    `}`

ExternalItem ->
    OuterAttribute* (
        MacroInvocationSemi
      | Visibility? StaticItem
      | Visibility? Function
    )
```

- `[^unsafe-2024]`: Starting with the 2024 Edition, the `unsafe` keyword is required semantically.

### Generic parameters

Reference: [items/generics.md](https://doc.rust-lang.org/reference/items/generics.html)

<sub>category: items · `items.generics.syntax`</sub>

```
GenericParams -> `<` ( GenericParam (`,` GenericParam)* `,`? )? `>`

GenericParam -> OuterAttribute* ( LifetimeParam | TypeParam | ConstParam )

LifetimeParam -> Lifetime ( `:` LifetimeBounds? )?

TypeParam -> IDENTIFIER ( `:` Bounds? )? ( `=` Type )?

ConstParam ->
    `const` IDENTIFIER `:` Type
    ( `=` ( BlockExpression | IDENTIFIER | `-`?LiteralExpression ) )?
```

*Where clauses*

<sub>category: items · `items.generics.where.syntax`</sub>

```
WhereClause -> `where` ( WhereClauseItem `,` )* WhereClauseItem?

WhereClauseItem ->
      LifetimeWhereClauseItem
    | TypeBoundWhereClauseItem

LifetimeWhereClauseItem -> Lifetime `:` LifetimeBounds?

TypeBoundWhereClauseItem -> ForLifetimes? Type `:` Bounds?
```

### Associated items

Reference: [items/associated-items.md](https://doc.rust-lang.org/reference/items/associated-items.html)

<sub>category: items · `items.associated.syntax`</sub>

```
AssociatedItem ->
    OuterAttribute* (
        MacroInvocationSemi
      | ( Visibility? ( TypeAlias | ConstantItem | Function ) )
    )
```

## Attributes

Reference: [attributes.md](https://doc.rust-lang.org/reference/attributes.html)

<sub>category: attributes · `attributes.syntax`</sub>

```
InnerAttribute -> `#` `!` `[` Attr `]`

OuterAttribute -> `#` `[` Attr `]`

Attr ->
      SimplePath AttrInput?
    | `unsafe` `(` SimplePath AttrInput? `)`

AttrInput ->
      DelimTokenTree
    | `=` Expression
```

*Meta item attribute syntax*

<sub>category: attributes · `attributes.meta.syntax`</sub>

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

<sub>category: attributes · `attributes.meta.builtin-syntax`</sub>

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

Reference: [attributes/codegen.md](https://doc.rust-lang.org/reference/attributes/codegen.html)

*The `inline` attribute*

<sub>category: attributes · `attributes.codegen.inline.syntax`</sub>

```
@root InlineAttribute ->
      `inline` `(` `always` `)`
    | `inline` `(` `never` `)`
    | `inline`
```

*The `optimize` attribute*

<sub>category: attributes · `attributes.codegen.optimize.syntax`</sub>

```
@root OptimizeAttribute ->
    `optimize` `(` ( `size` | `speed` | `none` ) `)`
```

### Debugger

Reference: [attributes/debugger.md](https://doc.rust-lang.org/reference/attributes/debugger.html)

*The `collapse_debuginfo` attribute*

<sub>category: attributes · `attributes.debugger.collapse_debuginfo.syntax`</sub>

```
@root CollapseDebuginfoAttribute -> `collapse_debuginfo` `(` CollapseDebuginfoOption `)`

CollapseDebuginfoOption ->
      `yes`
    | `no`
    | `external`
```

## Statements and expressions

### Statements

Reference: [statements.md](https://doc.rust-lang.org/reference/statements.html)

<sub>category: statements · `statement.syntax`</sub>

```
Statement ->
      `;`
    | Item
    | LetStatement
    | ExpressionStatement
    | OuterAttribute* MacroInvocationSemi
```

*`let` statements*

<sub>category: statements · `statement.let.syntax`</sub>

```
LetStatement ->
    OuterAttribute* `let` PatternNoTopAlt ( `:` Type )?
    (
          `=` Expression
        | `=` Expression _except [LazyBooleanExpression] or end with a `}`_
              `else` BlockExpressionNoInnerAttributes
    )? `;`
```

*Expression statements*

<sub>category: statements · `statement.expr.syntax`</sub>

```
ExpressionStatement ->
      ExpressionWithoutBlock `;`
    | ExpressionWithBlock `;`?
```

### Expressions

Reference: [expressions.md](https://doc.rust-lang.org/reference/expressions.html)

<sub>category: expressions · `expr.syntax`</sub>

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
    | ClosureExpression
    | AsyncBlockExpression
    | ContinueExpression
    | BreakExpression
    | RangeExpression
    | ReturnExpression
    | UnderscoreExpression
    | MacroInvocation

ExpressionWithBlock ->
    OuterAttribute* ExpressionWithBlockNoAttrs

ExpressionWithBlockNoAttrs ->
      BlockExpression
    | ConstBlockExpression
    | UnsafeBlockExpression
    | LoopExpression
    | IfExpression
    | MatchExpression
```

*Expression precedence* (strongest first)

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
| `return` `break` closures  | |

#### Literal expressions

Reference: [expressions/literal-expr.md](https://doc.rust-lang.org/reference/expressions/literal-expr.html)

<sub>category: expressions · `expr.literal.syntax`</sub>

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

Reference: [expressions/path-expr.md](https://doc.rust-lang.org/reference/expressions/path-expr.html)

<sub>category: expressions · `expr.path.syntax`</sub>

```
PathExpression ->
      PathInExpression
    | QualifiedPathInExpression
```

#### Block expressions

Reference: [expressions/block-expr.md](https://doc.rust-lang.org/reference/expressions/block-expr.html)

<sub>category: expressions · `expr.block.syntax`</sub>

```
BlockExpression ->
    `{`
        InnerAttribute*
        Statements?
    `}`

BlockExpressionNoInnerAttributes ->
    `{`
        Statements?
    `}`

Statements ->
      Statement+
    | Statement+ ExpressionWithoutBlock
    | ExpressionWithoutBlock
```

*`async` blocks*

<sub>category: expressions · `expr.block.async.syntax`</sub>

```
AsyncBlockExpression -> `async` `move`? BlockExpression
```

*`const` blocks*

<sub>category: expressions · `expr.block.const.syntax`</sub>

```
ConstBlockExpression -> `const` BlockExpression
```

*`unsafe` blocks*

<sub>category: expressions · `expr.block.unsafe.syntax`</sub>

```
UnsafeBlockExpression -> `unsafe` BlockExpression
```

#### Operator expressions

Reference: [expressions/operator-expr.md](https://doc.rust-lang.org/reference/expressions/operator-expr.html)

<sub>category: expressions · `expr.operator.syntax`</sub>

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

<sub>category: expressions · `expr.operator.borrow.syntax`</sub>

```
BorrowExpression ->
      (`&`|`&&`) Expression
    | (`&`|`&&`) `mut` Expression
    | (`&`|`&&`) `raw` `const` Expression
    | (`&`|`&&`) `raw` `mut` Expression
```

*The dereference operator*

<sub>category: expressions · `expr.deref.syntax`</sub>

```
DereferenceExpression -> `*` Expression
```

*The try propagation expression*

<sub>category: expressions · `expr.try.syntax`</sub>

```
TryPropagationExpression -> Expression `?`
```

*Negation operators*

<sub>category: expressions · `expr.negate.syntax`</sub>

```
NegationExpression ->
      `-` Expression
    | `!` Expression
```

*Arithmetic and logical binary operators*

<sub>category: expressions · `expr.arith-logic.syntax`</sub>

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

<sub>category: expressions · `expr.cmp.syntax`</sub>

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

<sub>category: expressions · `expr.bool-logic.syntax`</sub>

```
LazyBooleanExpression ->
      Expression `||` Expression
    | Expression `&&` Expression
```

*Type cast expressions*

<sub>category: expressions · `expr.as.syntax`</sub>

```
TypeCastExpression -> Expression `as` TypeNoBounds
```

*Assignment expressions*

<sub>category: expressions · `expr.assign.syntax`</sub>

```
AssignmentExpression -> Expression `=` Expression
```

*Compound assignment expressions*

<sub>category: expressions · `expr.compound-assign.syntax`</sub>

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

Reference: [expressions/grouped-expr.md](https://doc.rust-lang.org/reference/expressions/grouped-expr.html)

<sub>category: expressions · `expr.paren.syntax`</sub>

```
GroupedExpression -> `(` Expression `)`
```

#### Array and index expressions

Reference: [expressions/array-expr.md](https://doc.rust-lang.org/reference/expressions/array-expr.html)

*Array expressions*

<sub>category: expressions · `expr.array.syntax`</sub>

```
ArrayExpression -> `[` ArrayElements? `]`

ArrayElements ->
      Expression ( `,` Expression )* `,`?
    | Expression `;` Expression
```

*Array and slice indexing expressions*

<sub>category: expressions · `expr.array.index.syntax`</sub>

```
IndexExpression -> Expression `[` Expression `]`
```

#### Tuple and index expressions

Reference: [expressions/tuple-expr.md](https://doc.rust-lang.org/reference/expressions/tuple-expr.html)

*Tuple expressions*

<sub>category: expressions · `expr.tuple.syntax`</sub>

```
TupleExpression -> `(` TupleElements? `)`

TupleElements -> ( Expression `,` )+ Expression?
```

*Tuple indexing expressions*

<sub>category: expressions · `expr.tuple-index.syntax`</sub>

```
TupleIndexingExpression -> Expression `.` TUPLE_INDEX
```

#### Struct expressions

Reference: [expressions/struct-expr.md](https://doc.rust-lang.org/reference/expressions/struct-expr.html)

<sub>category: expressions · `expr.struct.syntax`</sub>

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

Reference: [expressions/call-expr.md](https://doc.rust-lang.org/reference/expressions/call-expr.html)

<sub>category: expressions · `expr.call.syntax`</sub>

```
CallExpression -> Expression `(` CallParams? `)`

CallParams -> Expression ( `,` Expression )* `,`?
```

#### Method call expressions

Reference: [expressions/method-call-expr.md](https://doc.rust-lang.org/reference/expressions/method-call-expr.html)

*Method-call expressions*

<sub>category: expressions · `expr.method.syntax`</sub>

```
MethodCallExpression -> Expression `.` PathExprSegment `(`CallParams? `)`
```

#### Field access expressions

Reference: [expressions/field-expr.md](https://doc.rust-lang.org/reference/expressions/field-expr.html)

<sub>category: expressions · `expr.field.syntax`</sub>

```
FieldExpression -> Expression `.` IDENTIFIER
```

#### Closure expressions

Reference: [expressions/closure-expr.md](https://doc.rust-lang.org/reference/expressions/closure-expr.html)

<sub>category: expressions · `expr.closure.syntax`</sub>

```
ClosureExpression ->
    `async`?[^cl-async-edition]
    `move`?
    ( `||` | `|` ClosureParameters? `|` )
    (Expression | `->` TypeNoBounds BlockExpression)

ClosureParameters -> ClosureParam (`,` ClosureParam)* `,`?

ClosureParam -> OuterAttribute* PatternNoTopAlt ( `:` Type )?
```

- `[^cl-async-edition]`: The `async` qualifier is not allowed in the 2015 edition.

#### Loop expressions

Reference: [expressions/loop-expr.md](https://doc.rust-lang.org/reference/expressions/loop-expr.html)

*Loops and other breakable expressions*

<sub>category: expressions · `expr.loop.syntax`</sub>

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

<sub>category: expressions · `expr.loop.infinite.syntax`</sub>

```
InfiniteLoopExpression -> `loop` BlockExpression
```

*Predicate loops*

<sub>category: expressions · `expr.loop.while.syntax`</sub>

```
PredicateLoopExpression -> `while` Conditions BlockExpression
```

*Iterator loops*

<sub>category: expressions · `expr.loop.for.syntax`</sub>

```
IteratorLoopExpression ->
    `for` Pattern `in` Expression _except [StructExpression]_ BlockExpression
```

*Loop labels*

<sub>category: expressions · `expr.loop.label.syntax`</sub>

```
LoopLabel -> LIFETIME_OR_LABEL `:`
```

*`break` expressions*

<sub>category: expressions · `expr.loop.break.syntax`</sub>

```
BreakExpression -> `break` LIFETIME_OR_LABEL? Expression?
```

*Labeled block expressions*

<sub>category: expressions · `expr.loop.block-labels.syntax`</sub>

```
LabelBlockExpression -> BlockExpression
```

*`continue` expressions*

<sub>category: expressions · `expr.loop.continue.syntax`</sub>

```
ContinueExpression -> `continue` LIFETIME_OR_LABEL?
```

#### Range expressions

Reference: [expressions/range-expr.md](https://doc.rust-lang.org/reference/expressions/range-expr.html)

<sub>category: expressions · `expr.range.syntax`</sub>

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

Reference: [expressions/if-expr.md](https://doc.rust-lang.org/reference/expressions/if-expr.html)

*`if` expressions*

<sub>category: expressions · `expr.if.syntax`</sub>

```
IfExpression ->
    `if` Conditions BlockExpressionNoInnerAttributes
    (`else` ( BlockExpressionNoInnerAttributes | IfExpression ) )?

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

#### Match expressions

Reference: [expressions/match-expr.md](https://doc.rust-lang.org/reference/expressions/match-expr.html)

*`match` expressions*

<sub>category: expressions · `expr.match.syntax`</sub>

```
MatchExpression ->
    `match` Scrutinee `{`
        InnerAttribute*
        MatchArms?
    `}`

Scrutinee -> Expression _except [StructExpression]_

MatchArms ->
    ( MatchArm `=>` ( ExpressionWithoutBlock `,` | ExpressionWithBlock `,`? ) )*
    MatchArm `=>` Expression `,`?

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

#### Return expressions

Reference: [expressions/return-expr.md](https://doc.rust-lang.org/reference/expressions/return-expr.html)

*`return` expressions*

<sub>category: expressions · `expr.return.syntax`</sub>

```
ReturnExpression -> `return` Expression?
```

#### Await expressions

Reference: [expressions/await-expr.md](https://doc.rust-lang.org/reference/expressions/await-expr.html)

<sub>category: expressions · `expr.await.syntax`</sub>

```
AwaitExpression -> Expression `.` `await`
```

#### Underscore expressions

Reference: [expressions/underscore-expr.md](https://doc.rust-lang.org/reference/expressions/underscore-expr.html)

*`_` expressions*

<sub>category: expressions · `expr.placeholder.syntax`</sub>

```
UnderscoreExpression -> `_`
```

## Patterns

Reference: [patterns.md](https://doc.rust-lang.org/reference/patterns.html)

<sub>category: patterns · `patterns.syntax`</sub>

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
    | MacroInvocation
    | ObsoleteRangePattern[^obsolete-range-edition]
```

- `[^obsolete-range-edition]`: The ObsoleteRangePattern syntax is semantically invalid in the 2021 edition and beyond.

*Literal patterns*

<sub>category: patterns · `patterns.literal.syntax`</sub>

```
LiteralPattern -> `-`? LiteralExpression
```

*Identifier patterns*

<sub>category: patterns · `patterns.ident.syntax`</sub>

```
IdentifierPattern -> `ref`? `mut`? IDENTIFIER ( `@` PatternNoTopAlt )?
```

*Wildcard pattern*

<sub>category: patterns · `patterns.wildcard.syntax`</sub>

```
WildcardPattern -> `_`
```

*Rest pattern*

<sub>category: patterns · `patterns.rest.syntax`</sub>

```
RestPattern -> `..`
```

*Range patterns*

<sub>category: patterns · `patterns.range.syntax`</sub>

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

ObsoleteRangePattern ->
    RangePatternBound `...` RangePatternBound

RangePatternBound ->
      LiteralPattern
    | PathExpression
```

*Reference patterns*

<sub>category: patterns · `patterns.ref.syntax`</sub>

```
ReferencePattern -> (`&`|`&&`) `mut`? PatternWithoutModernRange
```

*Struct patterns*

<sub>category: patterns · `patterns.struct.syntax`</sub>

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
      | `ref`? `mut`? IDENTIFIER
    )

StructPatternEtCetera -> `..`
```

*Tuple struct patterns*

<sub>category: patterns · `patterns.tuple-struct.syntax`</sub>

```
TupleStructPattern -> PathInExpression `(` TupleStructItems? `)`

TupleStructItems -> Pattern ( `,` Pattern )* `,`?
```

*Tuple patterns*

<sub>category: patterns · `patterns.tuple.syntax`</sub>

```
TuplePattern -> `(` TuplePatternItems? `)`

TuplePatternItems ->
      Pattern `,`
    | RestPattern
    | Pattern (`,` Pattern)+ `,`?
```

*Grouped patterns*

<sub>category: patterns · `patterns.paren.syntax`</sub>

```
GroupedPattern -> `(` Pattern `)`
```

*Slice patterns*

<sub>category: patterns · `patterns.slice.syntax`</sub>

```
SlicePattern -> `[` SlicePatternItems? `]`

SlicePatternItems -> Pattern (`,` Pattern)* `,`?
```

*Path patterns*

<sub>category: patterns · `patterns.path.syntax`</sub>

```
PathPattern -> PathExpression
```

## Type system

### Types

Reference: [types.md](https://doc.rust-lang.org/reference/types.html)

*Type expressions*

<sub>category: types · `type.name.syntax`</sub>

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
    | MacroInvocation
```

*Parenthesized types*

<sub>category: types · `type.name.parenthesized.syntax`</sub>

```
ParenthesizedType -> `(` Type `)`
```

#### Never type

Reference: [types/never.md](https://doc.rust-lang.org/reference/types/never.html)

<sub>category: types · `type.never.syntax`</sub>

```
NeverType -> `!`
```

#### Tuple types

Reference: [types/tuple.md](https://doc.rust-lang.org/reference/types/tuple.html)

<sub>category: types · `type.tuple.syntax`</sub>

```
TupleType ->
      `(` `)`
    | `(` ( Type `,` )+ Type? `)`
```

#### Array types

Reference: [types/array.md](https://doc.rust-lang.org/reference/types/array.html)

<sub>category: types · `type.array.syntax`</sub>

```
ArrayType -> `[` Type `;` Expression `]`
```

#### Slice types

Reference: [types/slice.md](https://doc.rust-lang.org/reference/types/slice.html)

<sub>category: types · `type.slice.syntax`</sub>

```
SliceType -> `[` Type `]`
```

#### Pointer types

Reference: [types/pointer.md](https://doc.rust-lang.org/reference/types/pointer.html)

*References (`&` and `&mut`)*

<sub>category: types · `type.pointer.reference.syntax`</sub>

```
ReferenceType -> `&` Lifetime? `mut`? TypeNoBounds
```

*Raw pointers (`*const` and `*mut`)*

<sub>category: types · `type.pointer.raw.syntax`</sub>

```
RawPointerType -> `*` ( `mut` | `const` ) TypeNoBounds
```

#### Function pointer types

Reference: [types/function-pointer.md](https://doc.rust-lang.org/reference/types/function-pointer.html)

<sub>category: types · `type.fn-pointer.syntax`</sub>

```
BareFunctionType ->
    ForLifetimes? FunctionTypeQualifiers `fn`
       `(` FunctionParametersMaybeNamedVariadic? `)` BareFunctionReturnType?

FunctionTypeQualifiers -> `unsafe`? (`extern` Abi?)?

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

#### Trait object types

Reference: [types/trait-object.md](https://doc.rust-lang.org/reference/types/trait-object.html)

*Trait objects*

<sub>category: types · `type.trait-object.syntax`</sub>

```
TraitObjectType -> Bounds[^bare-2021] | `dyn`[^dyn-2018] Bounds?

TraitObjectTypeOneBound -> TraitBound[^bare-2021] | `dyn`[^dyn-2018] TraitBound?
```

- `[^bare-2021]`: See type.trait-object.syntax-edition2021.
- `[^dyn-2018]`: See type.trait-object.syntax-edition2018.

#### Impl trait type

Reference: [types/impl-trait.md](https://doc.rust-lang.org/reference/types/impl-trait.html)

*Impl trait*

<sub>category: types · `type.impl-trait.syntax`</sub>

```
ImplTraitType -> `impl` Bounds?

ImplTraitTypeOneBound -> `impl` TraitBound?
```

#### Inferred type

Reference: [types/inferred.md](https://doc.rust-lang.org/reference/types/inferred.html)

<sub>category: types · `type.inferred.syntax`</sub>

```
InferredType -> `_`
```

### Trait and lifetime bounds

Reference: [trait-bounds.md](https://doc.rust-lang.org/reference/trait-bounds.html)

<sub>category: miscellaneous · `bound.syntax`</sub>

```
Bounds -> Bound ( `+` Bound )* `+`?

Bound -> Lifetime | TraitBound | UseBound

TraitBound ->
      ( `?` | ForLifetimes )? TypePath
    | `(` ( `?` | ForLifetimes )? TypePath `)`

LifetimeBounds -> ( Lifetime `+` )* Lifetime?

Lifetime ->
      LIFETIME_OR_LABEL
    | `'static`
    | `'_`

UseBound -> `use` UseBoundGenericArgs

UseBoundGenericArgs ->
      `<` `>`
    | `<` ( UseBoundGenericArg `,`)* UseBoundGenericArg `,`? `>`

UseBoundGenericArg ->
      Lifetime
    | IDENTIFIER
    | `Self`
```

*Higher-ranked trait bounds*

<sub>category: miscellaneous · `bound.higher-ranked.syntax`</sub>

```
ForLifetimes -> `for` GenericParams
```

## Names

### Paths

Reference: [paths.md](https://doc.rust-lang.org/reference/paths.html)

*Simple paths*

<sub>category: paths · `paths.simple.syntax`</sub>

```
SimplePath ->
    `::`? SimplePathSegment (`::` SimplePathSegment)*

SimplePathSegment ->
    IDENTIFIER | `super` | `self` | `crate` | `$crate`
```

*Paths in expressions*

<sub>category: paths · `paths.expr.syntax`</sub>

```
PathInExpression ->
    `::`? PathExprSegment (`::` PathExprSegment)*

PathExprSegment ->
    PathIdentSegment (`::` GenericArgs)?

PathIdentSegment ->
    IDENTIFIER | `super` | `self` | `Self` | `crate` | `$crate`

GenericArgs ->
      `<` GenericArgList? `>`
    | `(` TypeList? `)` (`->` TypeNoBounds)?

GenericArgList ->
    ( GenericArg `,` )* GenericArg `,`?

TypeList ->
    ( Type `,` )* Type `,`?

GenericArg ->
    Lifetime | Type | GenericArgsConst | GenericArgsBinding | GenericArgsBounds

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

*Qualified paths*

<sub>category: paths · `paths.qualified.syntax`</sub>

```
QualifiedPathInExpression -> QualifiedPathType (`::` PathExprSegment)+

QualifiedPathType -> `<` Type (`as` TypePath)? `>`

QualifiedPathInType -> QualifiedPathType (`::` TypePathSegment)+
```

*Paths in types*

<sub>category: paths · `paths.type.syntax`</sub>

```
TypePath -> `::`? TypePathSegment (`::` TypePathSegment)*

TypePathSegment -> PathIdentSegment (`::`? GenericArgs)?
```

### Visibility and privacy

Reference: [visibility-and-privacy.md](https://doc.rust-lang.org/reference/visibility-and-privacy.html)

<sub>category: items · `vis.syntax`</sub>

```
Visibility ->
      `pub`
    | `pub` `(` `crate` `)`
    | `pub` `(` `self` `)`
    | `pub` `(` `super` `)`
    | `pub` `(` `in` SimplePath `)`
```

## Inline assembly

Reference: [inline-assembly.md](https://doc.rust-lang.org/reference/inline-assembly.html)

*Syntax*

<sub>category: assembly · `asm.syntax`</sub>

```
@root AsmArgs -> AsmAttrFormatString (`,` AsmAttrFormatString)* (`,` AsmAttrOperand)* `,`?

FormatString -> STRING_LITERAL | RAW_STRING_LITERAL | MacroInvocation

AsmAttrFormatString -> (OuterAttribute)* FormatString

AsmOperand ->
      ClobberAbi
    | AsmOptions
    | RegOperand

AsmAttrOperand -> (OuterAttribute)* AsmOperand

ClobberAbi -> `clobber_abi` `(` Abi (`,` Abi)* `,`? `)`

AsmOptions ->
    `options` `(` ( AsmOption (`,` AsmOption)* `,`? )? `)`

AsmOption ->
      `pure`
    | `nomem`
    | `readonly`
    | `preserves_flags`
    | `noreturn`
    | `nostack`
    | `att_syntax`
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

## Index of productions

Every production name, alphabetically, with the chapter that defines it.

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
- `AsClause`: [Extern crates](#extern-crates)
- `AsmArgs`: [Inline assembly](#inline-assembly)
- `AsmAttrFormatString`: [Inline assembly](#inline-assembly)
- `AsmAttrOperand`: [Inline assembly](#inline-assembly)
- `AsmOperand`: [Inline assembly](#inline-assembly)
- `AsmOption`: [Inline assembly](#inline-assembly)
- `AsmOptions`: [Inline assembly](#inline-assembly)
- `AssignmentExpression`: [Operator expressions](#operator-expressions)
- `AssociatedItem`: [Associated items](#associated-items)
- `AsyncBlockExpression`: [Block expressions](#block-expressions)
- `Attr`: [Attributes](#attributes)
- `AttrInput`: [Attributes](#attributes)
- `AwaitExpression`: [Await expressions](#await-expressions)
- `BareFunctionReturnType`: [Function pointer types](#function-pointer-types)
- `BareFunctionType`: [Function pointer types](#function-pointer-types)
- `BIN_DIGIT`: [Tokens](#tokens)
- `BIN_LITERAL`: [Tokens](#tokens)
- `BLOCK_CHAR`: [Comments](#comments)
- `BLOCK_COMMENT`: [Comments](#comments)
- `BlockExpression`: [Block expressions](#block-expressions)
- `BlockExpressionNoInnerAttributes`: [Block expressions](#block-expressions)
- `BorrowExpression`: [Operator expressions](#operator-expressions)
- `Bound`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `Bounds`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `BreakExpression`: [Loop expressions](#loop-expressions)
- `BYTE_ESCAPE`: [Tokens](#tokens)
- `BYTE_LITERAL`: [Tokens](#tokens)
- `BYTE_STRING_LITERAL`: [Tokens](#tokens)
- `C_STRING_LITERAL`: [Tokens](#tokens)
- `CallExpression`: [Call expressions](#call-expressions)
- `CallParams`: [Call expressions](#call-expressions)
- `CfgAttrAttribute`: [Conditional compilation](#conditional-compilation)
- `CfgAttribute`: [Conditional compilation](#conditional-compilation)
- `CfgAttrs`: [Conditional compilation](#conditional-compilation)
- `CfgSelect`: [Conditional compilation](#conditional-compilation)
- `CfgSelectArms`: [Conditional compilation](#conditional-compilation)
- `CfgSelectConfigurationPredicate`: [Conditional compilation](#conditional-compilation)
- `CHAR`: [Input format](#input-format)
- `CHAR_LITERAL`: [Tokens](#tokens)
- `ClobberAbi`: [Inline assembly](#inline-assembly)
- `ClosureExpression`: [Closure expressions](#closure-expressions)
- `ClosureParam`: [Closure expressions](#closure-expressions)
- `ClosureParameters`: [Closure expressions](#closure-expressions)
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
- `Crate`: [Crates and source files](#crates-and-source-files)
- `CrateRef`: [Extern crates](#extern-crates)
- `DEC_DIGIT`: [Tokens](#tokens)
- `DEC_LITERAL`: [Tokens](#tokens)
- `DelimTokenTree`: [Macros](#macros)
- `DereferenceExpression`: [Operator expressions](#operator-expressions)
- `DeriveMacroAttributes`: [Procedural macros](#procedural-macros)
- `DeriveMacroName`: [Procedural macros](#procedural-macros)
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
- `ExternBlock`: [External blocks](#external-blocks)
- `ExternCrate`: [Extern crates](#extern-crates)
- `FieldExpression`: [Field access expressions](#field-access-expressions)
- `FLOAT_EXPONENT`: [Tokens](#tokens)
- `FLOAT_LITERAL`: [Tokens](#tokens)
- `ForLifetimes`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `FormatString`: [Inline assembly](#inline-assembly)
- `Function`: [Functions](#functions)
- `FunctionParam`: [Functions](#functions)
- `FunctionParameters`: [Functions](#functions)
- `FunctionParametersMaybeNamedVariadic`: [Function pointer types](#function-pointer-types)
- `FunctionParamPattern`: [Functions](#functions)
- `FunctionQualifiers`: [Functions](#functions)
- `FunctionReturnType`: [Functions](#functions)
- `FunctionTypeQualifiers`: [Function pointer types](#function-pointer-types)
- `GenericArg`: [Paths](#paths)
- `GenericArgList`: [Paths](#paths)
- `GenericArgs`: [Paths](#paths)
- `GenericArgsBinding`: [Paths](#paths)
- `GenericArgsBounds`: [Paths](#paths)
- `GenericArgsConst`: [Paths](#paths)
- `GenericParam`: [Generic parameters](#generic-parameters)
- `GenericParams`: [Generic parameters](#generic-parameters)
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
- `IndexExpression`: [Array and index expressions](#array-and-index-expressions)
- `InferredType`: [Inferred type](#inferred-type)
- `InfiniteLoopExpression`: [Loop expressions](#loop-expressions)
- `InherentImpl`: [Implementations](#implementations)
- `InlineAttribute`: [Code generation](#code-generation)
- `INNER_BLOCK_DOC`: [Comments](#comments)
- `INNER_LINE_DOC`: [Comments](#comments)
- `InnerAttribute`: [Attributes](#attributes)
- `INTEGER_LITERAL`: [Tokens](#tokens)
- `Item`: [Items](#items)
- `ItemSafety`: [Functions](#functions)
- `IteratorLoopExpression`: [Loop expressions](#loop-expressions)
- `LabelBlockExpression`: [Loop expressions](#loop-expressions)
- `LazyBooleanExpression`: [Operator expressions](#operator-expressions)
- `LetChain`: [If expressions](#if-expressions)
- `LetChainCondition`: [If expressions](#if-expressions)
- `LetStatement`: [Statements](#statements)
- `LF`: [Whitespace](#whitespace)
- `Lifetime`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `LIFETIME_OR_LABEL`: [Tokens](#tokens)
- `LIFETIME_TOKEN`: [Tokens](#tokens)
- `LifetimeBounds`: [Trait and lifetime bounds](#trait-and-lifetime-bounds)
- `LifetimeParam`: [Generic parameters](#generic-parameters)
- `LifetimeWhereClauseItem`: [Generic parameters](#generic-parameters)
- `LINE_COMMENT`: [Comments](#comments)
- `LINE_DOC_COMMENT_CONTENT`: [Comments](#comments)
- `LiteralExpression`: [Literal expressions](#literal-expressions)
- `LiteralPattern`: [Patterns](#patterns)
- `LoopExpression`: [Loop expressions](#loop-expressions)
- `LoopLabel`: [Loop expressions](#loop-expressions)
- `MacroFragSpec`: [Macros by example](#macros-by-example)
- `MacroInvocation`: [Macros](#macros)
- `MacroInvocationSemi`: [Macros](#macros)
- `MacroItem`: [Items](#items)
- `MacroMatch`: [Macros by example](#macros-by-example)
- `MacroMatcher`: [Macros by example](#macros-by-example)
- `MacroRepOp`: [Macros by example](#macros-by-example)
- `MacroRepSep`: [Macros by example](#macros-by-example)
- `MacroRule`: [Macros by example](#macros-by-example)
- `MacroRules`: [Macros by example](#macros-by-example)
- `MacroRulesDef`: [Macros by example](#macros-by-example)
- `MacroRulesDefinition`: [Macros by example](#macros-by-example)
- `MacroTranscriber`: [Macros by example](#macros-by-example)
- `MatchArm`: [Match expressions](#match-expressions)
- `MatchArmGuard`: [Match expressions](#match-expressions)
- `MatchArms`: [Match expressions](#match-expressions)
- `MatchConditions`: [Match expressions](#match-expressions)
- `MatchExpression`: [Match expressions](#match-expressions)
- `MatchGuardChain`: [Match expressions](#match-expressions)
- `MatchGuardCondition`: [Match expressions](#match-expressions)
- `MatchGuardScrutinee`: [Match expressions](#match-expressions)
- `MaybeNamedFunctionParameters`: [Function pointer types](#function-pointer-types)
- `MaybeNamedFunctionParametersVariadic`: [Function pointer types](#function-pointer-types)
- `MaybeNamedParam`: [Function pointer types](#function-pointer-types)
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
- `Module`: [Modules](#modules)
- `NegationExpression`: [Operator expressions](#operator-expressions)
- `NESTED_BLOCK_DOC_COMMENT`: [Comments](#comments)
- `NeverType`: [Never type](#never-type)
- `NON_KEYWORD_IDENTIFIER`: [Identifiers](#identifiers)
- `NUL`: [Input format](#input-format)
- `ObsoleteRangePattern`: [Patterns](#patterns)
- `OCT_DIGIT`: [Tokens](#tokens)
- `OCT_LITERAL`: [Tokens](#tokens)
- `OperatorExpression`: [Operator expressions](#operator-expressions)
- `OptimizeAttribute`: [Code generation](#code-generation)
- `OUTER_BLOCK_DOC`: [Comments](#comments)
- `OUTER_LINE_DOC`: [Comments](#comments)
- `OuterAttribute`: [Attributes](#attributes)
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
- `ProcMacroDeriveAttribute`: [Procedural macros](#procedural-macros)
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
- `SHEBANG`: [Shebang](#shebang)
- `SHEBANG_BLOCK_COMMENT`: [Shebang](#shebang)
- `SHEBANG_NESTED_BLOCK_COMMENT`: [Shebang](#shebang)
- `ShorthandSelf`: [Functions](#functions)
- `SimplePath`: [Paths](#paths)
- `SimplePathSegment`: [Paths](#paths)
- `SlicePattern`: [Patterns](#patterns)
- `SlicePatternItems`: [Patterns](#patterns)
- `SliceType`: [Slice types](#slice-types)
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
- `Token`: [Tokens](#tokens)
- `TokenTree`: [Macros](#macros)
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
- `UseDeclaration`: [Use declarations](#use-declarations)
- `UseTree`: [Use declarations](#use-declarations)
- `Visibility`: [Visibility and privacy](#visibility-and-privacy)
- `VisItem`: [Items](#items)
- `WEAK_KEYWORDS`: [Keywords](#keywords)
- `WhereClause`: [Generic parameters](#generic-parameters)
- `WhereClauseItem`: [Generic parameters](#generic-parameters)
- `WHITESPACE`: [Whitespace](#whitespace)
- `WildcardPattern`: [Patterns](#patterns)
- `XID_Continue`: [Identifiers](#identifiers)
- `XID_Start`: [Identifiers](#identifiers)

