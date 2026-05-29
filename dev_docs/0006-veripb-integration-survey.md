# VeriPB integration survey

> Drafted by AI assistant under human oversight. Stub — to be filled
> in. Last updated 2026-05-29.

Status: **TODO**.

This document will collect findings on what of VeriPB's Rust codebase
is currently usable as a library crate, and what would have to change
upstream for `opbdiff` to depend on it directly.

The human notes that making VeriPB's API more usable in other tools
is a work-in-progress upstream effort. Our position is therefore:
ship our own parser now, and design the AST in `parser/ast.rs` so
that swapping in VeriPB's parser later is a translation layer, not a
rewrite.

## Open questions to answer

- What parts of the OPB parser are exposed as public API?
- What does VeriPB's parser AST look like? Where does our AST need
  to match it so the swap is mechanical?
- Is there a stable crate-version story (semver, published on
  crates.io, git dependency only)?
- What is the licence and is it compatible with our
  `MIT OR Apache-2.0`?
- Are there work-in-progress branches we should track for the
  library-isation effort?
- Does VeriPB expose constraint-normalisation primitives we could
  reuse, or is normalisation purely internal to its proof checker?

## Plan

Fill this document in before any v0.2 milestone. The survey itself is
research, not code, so it can happen at any time without blocking the
core comparison work.
