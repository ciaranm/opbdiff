# opbdiff

A semantic diff for [VeriPB][veripb]-extended OPB pseudo-Boolean files.

## Status

Pre-release. Skeleton only; no working comparison yet. APIs and CLI flags are
unstable and will change.

## What it is

`opbdiff` compares two OPB files by what they *mean* rather than by their bytes.
For example, these two lines describe the same constraint and `opbdiff` will
treat them as equivalent:

```
1 x1 1 x2 1 x3 >= 2 ;
+1 ~x3 +1 ~x2 +1 ~x1 <= 1 ;
```

Two intended uses:

- Comparing before-and-after OPB output when a change to a proof-generating
  solver causes unexpected breakage.
- Lining up output from a solver using its own encoder against output from an
  external encoder of the same problem.

Comparison is order-sensitive by default; an `--unordered` flag treats the
constraints as multisets. Constraint labels (`@name ...`) can optionally be
checked against a directional reference file.

## Build

Requires a stable Rust toolchain (edition 2024, MSRV 1.85).

```
cargo build --release
```

## Usage

The CLI is not implemented yet. The intended shape is:

```
opbdiff [OPTIONS] <A> <B>
```

See `dev_docs/` for the design in progress.

## Development notes

Active design and implementation notes live in [`dev_docs/`](dev_docs/),
including the OPB dialect we accept, the normalisation rules, and the
comparison algorithm.

This project is being developed with significant assistance from an AI
coding assistant (Claude, by Anthropic) under human oversight. Each commit
carries a `Co-Authored-By` trailer identifying the AI contributor.

## Licence

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
licence, shall be dual licensed as above, without any additional terms or
conditions.

[veripb]: https://gitlab.com/MIAOresearch/software/VeriPB
