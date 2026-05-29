# Architecture

> Drafted by AI assistant under human oversight. Last updated 2026-05-29.

This document describes the overall structure of `opbdiff` and how data
flows from input files to a diff result.

## Pipeline

```
file A ─▶ Parser ─▶ AST_A ─▶ Normaliser ─▶ Canonical model A ─┐
                                                              ├─▶ Compare ─▶ Diff result ─▶ Reporter ─▶ stdout
file B ─▶ Parser ─▶ AST_B ─▶ Normaliser ─▶ Canonical model B ─┘
```

1. **Parser** reads an OPB file into an AST that preserves source
   information (line/column, original text) for diagnostics and human
   output. See [0002](0002-opb-format.md) for the dialect we accept.
2. **Normaliser** transforms each AST entry into the canonical model
   that defines semantic equivalence. See [0003](0003-normalization.md).
3. **Comparison engine** consumes the two canonical models and
   produces a structured diff result. See
   [0004](0004-comparison-algorithm.md).
4. **Reporter** formats the diff result for output (plain text in v1,
   then colour, JSON, side-by-side). See
   [0005](0005-output-formats.md).

## Crate layout

```
src/
├── main.rs        # CLI entry point: argv parsing, file I/O, exit codes
├── lib.rs         # public API; re-exports
├── cli.rs         # clap definitions (kept apart for testability)
├── parser/
│   ├── mod.rs     # parse() entry point and Parser type
│   ├── lexer.rs   # token stream
│   └── ast.rs     # AST types (constraint, objective, preserved, label)
├── model/
│   ├── mod.rs     # canonical-model types
│   └── normal.rs  # AST → canonical normalisation
├── compare.rs     # ordered + unordered comparison, label rules
└── report/
    ├── mod.rs     # Reporter trait
    ├── plain.rs   # v1
    └── ...        # color/json/side_by_side land later
```

The split between AST (`parser/ast.rs`) and canonical model
(`model/`) is deliberate. The AST is faithful to source: it keeps the
original literal forms (positive or negated), the operator (`<=` /
`>=`), the original RHS sign, and span information so we can show the
user the original text when reporting. The canonical model is
post-normalisation and defines equality; it is what the comparison
engine actually compares.

## Library / binary split

`src/lib.rs` exposes parser, model, and reporters as a library API.
`src/main.rs` is a thin shell that does argv parsing, file I/O, and
exit-code mapping. This split keeps integration tests honest (they
exercise the library, not subprocess output) and leaves the door open
for embedding `opbdiff` in other tools.

## VeriPB integration posture

We start with our own parser, but the AST types in `parser/ast.rs`
are designed so that swapping in VeriPB's parser later is a matter of
writing a translation layer, not a rewrite. See
[0006](0006-veripb-integration-survey.md) for the survey to be done.
