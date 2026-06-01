# Comparison algorithm

> Drafted by AI assistant under human oversight. Last updated 2026-06-01.

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

## Constraint comparison: label-matched (`--match-labels`)

`--match-labels` changes *how constraints are paired*, orthogonally to
ordered/unordered (which still governs the leftovers) and to
`--check-labels` (which still post-checks labels on the pairs).

The pairing runs in two passes:

1. **Label pass.** Walk A in order. For each A constraint carrying a
   label that is also present (and not yet claimed) in B, pair the two
   immediately. The pair is **matched** if their canonical forms
   agree, **differing** otherwise. Duplicate labels — which VeriPB
   does not normally emit — are paired first-come-first-served over
   B's occurrences.
2. **Fallback pass.** Everything left unpaired (A constraints with no
   label, or whose label is absent from B, together with the
   corresponding remainder of B) is run through the ordinary
   ordered/unordered matching. Sub-indices from this pass are
   translated back to original file positions before reporting.

Label-matched pairs are emitted first (in A order), then the fallback
diffs.

This mode is the right tool when two encoders **agree on labels but
disagree on order or on auxiliary-variable names**. The label pins the
pairing so the diff lands on the genuinely-differing content rather
than on a positional misalignment. Typical invocation for
cross-encoder comparison is `--match-labels --unordered`, so that
constraints which carry no label on one side (e.g. variable bounds that
one encoder labels and the other does not) still align by canonical
form rather than by position.

Note that `--match-labels` pairs constraints with *equal* labels; it
never equates differently-labelled constraints. Differently- or
one-sidedly-labelled pairs are surfaced as content/leftover
differences here, and as explicit mismatches under `--check-labels`.

## Auxiliary-variable folding (`--ignore-aux-names`)

By default two constraints are equal only if their canonical forms are
identical, variable names included. `--ignore-aux-names` relaxes this
for *auxiliary* variables: two constraints become equal when they are
identical after some renaming of their auxiliary variables.

**What counts as auxiliary.** A variable is auxiliary iff it is *not*
in the projected set, which is taken from the `preserved:` line. We do
not hard-code any naming convention. The projected set is resolved as:

- exactly one file has a `preserved:` line → use it;
- both have one → they must be equal, else it is an error;
- neither has one → it is an error (no basis for the split).

**How the comparison works.** Each constraint is reduced to a match
key with three parts: the projected terms kept by name (sorted), the
multiset of auxiliary coefficients (names dropped, sorted), and the
right-hand side. Two constraints match iff their keys are equal. This
is exactly "there exists a renaming of the auxiliary variables of one
constraint that makes it identical to the other", because within a
single constraint a renaming is free to permute auxiliary variables,
and only their coefficient multiset is invariant under that.

The match key is the unit all three pairing strategies (ordered,
unordered, label-matched) compare on, so folding composes with all of
them, and with `--check-labels`.

**Known limitation — per-constraint, not global.** Folding is decided
one constraint at a time. It does *not* check that a single consistent
bijection over auxiliary variables works across the whole file. So two
files can be reported equivalent under `--ignore-aux-names` even if no
global renaming reconciles them (e.g. constraint 1 needs `f→g` while
constraint 2 needs `f→h`). Verifying a global renaming is the harder
constraint-graph-isomorphism problem and is out of scope; the
per-constraint check is the cheap, broadly-useful approximation.

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

## Known limitation: auxiliary-variable renaming

Three of the four `.opb` / `.verifiedopb` fixture pairs we have
(`money`, `crystal_maze`, `sudoku`) use *different* names for the
auxiliary variables introduced by AllDifferent and similar
decompositions. For example, one side uses `f[0][notequals]` where
the other uses `x[money_a_d][0_1]`. The bound constraints on the
shared user variables normalise to the same canonical form on both
sides, but the AllDifferent constraints do not, because they
mention aux variables that have no shared identity.

By **default** we do not treat differently-named variables as
equivalent, so the comparison engine reports these as differing and
the human can see the difference is confined to aux-var names.
`--match-labels` makes this easier to read when the two sides share
labels: the differing constraints are paired up by label and the
canonical-form view shows only the renamed aux variables.

`--ignore-aux-names` (see above) is the opt-in that folds these away
entirely, comparing auxiliary terms by coefficient. On the `colour`
fixtures, `--unordered --ignore-aux-names` collapses all the
`f`/`b`/`x`-renamed reified constraints to matches and leaves only the
genuine structural difference. The full graph-isomorphism approach (a
single globally-consistent renaming, or an explicit mapping file)
remains a possible future refinement.

## Verdict and exit code

The verdict is **equivalent** if and only if every recorded entry
across all sections is a `matched` pair. Anything else makes the
verdict **different**. The CLI maps these to exit codes:

| Exit | Meaning                              |
|-----:|--------------------------------------|
|  0   | Files are semantically equivalent    |
|  1   | Files differ                         |
|  2   | Parse, I/O, or other usage error     |
