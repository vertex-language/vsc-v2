# vsc-v2

The compiler for Vertex v2: Rust with fourteen exceptions in its spelling
([grammar](src/doc/reference/grammar.md), [usage guide](src/doc/reference/usage.md)).

vsc-v2 is written in Rust and copies rustc's design: the same crates (named
`vsc_*` for `rustc_*`), the same intermediate representations (AST, HIR, THIR,
MIR, then LLVM IR) and the same command line, so cargo builds Vertex crates
with `build.rustc = "vsc"`. The compiler's own code is written from scratch;
its foundation, front end and middle use Rust's std only, and the few outside
crates allowed at the edges are listed in
[`src/tools/tidy/src/deps.rs`](src/tools/tidy/src/deps.rs).

## Status

The front of the pipeline works: lexing, parsing, package loading, `@cfg`,
built-ins, name resolution, lowering to HIR, item signatures, type checking
of function bodies, THIR with the `unsafe` and pattern checks, and MIR.

| Stage | Crates | State |
| --- | --- | --- |
| Foundation | `vsc_arena`, `vsc_index`, `vsc_data_structures`, `vsc_span`, `vsc_errors`, `vsc_session` | started |
| Lexing | `vsc_lexer` | done |
| Parsing | `vsc_ast`, `vsc_parse`, `vsc_ast_pretty` | done for `.vs`; every Vertex snippet in the usage guide parses |
| Expansion | `vsc_expand`, `vsc_parse_format`, `vsc_builtin_macros` | package folders, `@cfg`, `@cfgAttr`, the format built-ins; `@derive` next |
| Name resolution | `vsc_resolve`, `vsc_hir::def` | definitions, packages, per-file imports, `.` paths, patterns, origins, labels |
| Lowering to HIR | `vsc_ast_lowering`, `vsc_hir`, `vsc_hir_pretty` | done: exception 7's fields and methods, rustc's desugarings, format built-ins |
| Signatures | `vsc_middle` (`ty`, `TyCtxt`, queries), `vsc_passes`, `vsc_hir_analysis` | types, generics, predicates, function signatures (exceptions 2, 3, 13, 14), lang items, `break`/`continue` checks, trait impl completeness, duplicate methods |
| Type checking bodies | `vsc_infer`, `vsc_trait_selection`, `vsc_hir_typeck` | inference, coercions, method lookup, trait selection and normalization, patterns with default binding modes, exceptions 3, 10 and 14; not yet `async`, operator traits, `unsafe` checking |
| THIR | `vsc_middle::thir`, `vsc_mir_build`, `vsc_pattern_analysis` | THIR with explicit adjustments; `unsafe` checking; exhaustiveness (E0004) and refutability (E0005) with witnesses, unreachable arms |
| MIR | `vsc_middle::mir`, `vsc_mir_build` | built from THIR with overflow, division and bounds checks, arm-by-arm matching, closures with captures; validated by `-Z validate-mir` (on in every ui test); no drops or storage markers yet; slicing by ranges not yet |
| Borrow checking, interpreter, code generation | | not started |
| Code generation (LLVM) | | not started |

Until `library/` can be built, imports of `std` and other crates report
`can't find crate`, and standard names (`Option`, `Vec`, …) don't resolve.
Types that need a lang item (`string` needs `String`, a `class` needs
`Class`, `func(T) -> U` needs `Func`) report it missing; a crate can define
the ones it needs with `@lang("String")`, as rustc's `no_core` tests do;
ui tests get a set of stubs with `//@ add-core-stubs`
([`tests/auxiliary/minicore.vs`](tests/auxiliary/minicore.vs)).

## Building

You need a stable Rust toolchain (1.98 or later).

```sh
./x build                        # build vsc
./x test                         # tidy, unit tests, tests/ui
./x test tests/ui [FILTER] --bless
```

```sh
target/debug/vsc -Z unpretty=ast main.vs     # parse and print back
target/debug/vsc -Z parse-only main.vs       # parse and report errors
target/debug/vsc -Z dump-resolutions main.vs # what every path resolved to
target/debug/vsc -Z unpretty=hir main.vs     # the crate after lowering, in Rust syntax
target/debug/vsc -Z dump-signatures main.vs  # every item's type or signature
target/debug/vsc -Z dump-types main.vs       # the types inferred in every body
target/debug/vsc -Z unpretty=thir-tree main.vs  # every body's THIR
target/debug/vsc -Z dump-mir main.vs         # every body's MIR
```

## Layout

```
compiler/     one crate per rustc crate: vsc_lexer, vsc_parse, vsc_ast, …, vsc (the binary)
src/bootstrap ./x
src/tools/    compiletest, tidy
src/doc/      the Vertex reference: grammar.md, usage.md, rust_grammar.md
tests/ui/     .vs files with //~ annotations and expected .stderr / .stdout
```

Inside each crate the files follow rustc's names, so the
[rustc-dev-guide](https://rustc-dev-guide.rust-lang.org/) is a map of vsc.
Where Vertex differs, the file says which of the fourteen exceptions it
implements.

## License

[MIT](LICENSE). See [COPYRIGHT](COPYRIGHT) for borrowed material.
