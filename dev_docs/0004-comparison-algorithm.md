# Comparison algorithm

> Drafted by AI assistant under human oversight. Last updated 2026-05-29.

The comparison engine consumes two canonical models and produces a
structured diff result. This document defines its behaviour.

## Sections of a file

For comparison purposes a canonical model is split into named
sections:

- **objective** (`min:`): at most one per file. Either both files
  have one and they are compared, or neither does, or it is a
  per-side missing/extra mismatch.
- **preserved** (`preserved:`): same shape as objective.
- **constraints**: ordered sequence (or multiset under
  `--unordered`).

Differences in any section count towards the overall verdict.

## Constraint comparison: ordered (default)

The constraint sections of A and B are zipped index-wise. For each
index `i`:

- If both sides have a constraint at `i` and their canonical forms
  match, the pair is recorded as **matched**.
- If they differ, the pair is recorded as **differing**, carrying
  both originals for the reporter.
- If only one side has a constraint at `i` (the other ran out
  earlier), the surplus is recorded as **extra-in-A** or
  **extra-in-B**.

## Constraint comparison: unordered (`--unordered`)

Both sides' constraints are placed into multisets keyed by canonical
form. The symmetric difference produces **only-in-A** and
**only-in-B** buckets; the intersection produces **matched** pairs
(one per shared canonical-form occurrence).

When duplicates exist (the same canonical form occurring multiple
times), they are matched up to the smaller multiplicity; the surplus
appears in the appropriate only-in-X bucket.

When `--check-labels` and `--unordered` combine, label checking is
applied to each matched pair after multiset alignment.

## Label handling (`--check-labels`)

Without `--check-labels`, labels are parsed and preserved on the AST
but ignored for the verdict.

With `--check-labels` and `--reference <A|B>`, label correspondence
is checked across matched pairs:

- Let `R` be the reference side and `C` be the candidate side. For
  every matched pair where the reference-side constraint carries a
  label `@l`, the candidate-side constraint must also carry the
  label `@l`. Otherwise the pair is reclassified as
  **label-mismatch**.
- Labels on the candidate side for pairs where the reference side is
  unlabelled are allowed and do not cause a mismatch.
- A constraint on the candidate side with the *wrong* label (rather
  than just an extra label) is a mismatch.

The default for `--reference` is `B`. This matches the verbal
description we discussed: A is the candidate (e.g. solver output
under test), B is the reference (e.g. expected output). Either is
selectable for use-cases where the polarity is reversed.

## v1 alignment policy

For human output we currently use exact-bucketing only: a pair is
either matched, differing-at-index, or extra-on-one-side. We do not
fuzzy-match "similar" constraints across A and B for v1. Fuzzy
alignment is a known follow-up.

## Verdict and exit code

The verdict is **equivalent** if and only if every recorded entry
across all sections is a `matched` pair. Anything else makes the
verdict **different**. The CLI maps these to exit codes:

| Exit | Meaning                              |
|-----:|--------------------------------------|
|  0   | Files are semantically equivalent    |
|  1   | Files differ                         |
|  2   | Parse, I/O, or other usage error     |
