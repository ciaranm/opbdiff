# Normalisation: defining semantic equivalence

> Drafted by AI assistant under human oversight. Last updated 2026-05-29.

Two constraints are considered semantically equivalent by `opbdiff` if
and only if they reduce to the same canonical form by the procedure
described here. The same canonical form is also used for the bodies
of `min:` and `preserved:` lines.

## Canonical form

A canonical constraint is the tuple

```
( [ (variable, non_zero_signed_int_coefficient), ... ],  rhs: signed_int )
```

ordered lexicographically by variable name, with the implied meaning

```
Σ coefficient · variable  >=  rhs
```

over Boolean variables. The sorted-tuple-with-RHS encoding gives a
deterministic representation suitable for hashing and equality.

For `min:` and `preserved:`, there is no RHS — they reduce to the
sorted term list alone (with `min:` and `preserved:` as distinct
kinds, so a `min:` is never equal to a `preserved:` of the same
body).

## Normalisation steps

Given an AST entry, produce its canonical form as follows.

### Step 1: orient the inequality to `>=`

If the operator is `<=`, multiply every coefficient (including any
LHS constant) and the RHS by `-1` and change the operator to `>=`.

### Step 2: rewrite negated literals

For every term `c · ~x`, rewrite as `(-c) · x + c`. The constant `c`
will be absorbed into the LHS-constant pool in step 3.

This uses the identity `~x = 1 - x`, valid because `x ∈ {0, 1}`.

### Step 3: move all LHS constants to the RHS

Any term with no literal (constant terms produced explicitly or by
the negated-literal rewrite) is summed and subtracted from the RHS.

### Step 4: combine like terms

Sum coefficients of terms referring to the same variable. After this
step every variable appears at most once.

### Step 5: drop zero-coefficient terms

Terms whose summed coefficient is `0` are removed. This is also where
tolerating coefficient-shifts that net to nothing — e.g. an explicit
`0 x3` term — fall away.

### Step 6: sort

Sort terms by variable name to give a deterministic representation.

The pair `(sorted_terms, rhs)` is the canonical form.

## Worked example

The two lines below are intuitively the same constraint:

```
A:  1 x1 1 x2 1 x3 >= 2 ;
B:  +1 ~x3 +1 ~x2 +1 ~x1 <= 1 ;
```

### Normalising A

- Step 1: already `>=`, no change.
- Steps 2–5: no negated literals, no LHS constants, no duplicates,
  no zeros.
- Step 6: sort by variable → `[(x1, 1), (x2, 1), (x3, 1)]`,
  RHS = `2`.

Canonical form: `( [(x1,1), (x2,1), (x3,1)], 2 )`.

### Normalising B

- Step 1: operator is `<=`. Negate everything: terms become
  `-1 ~x3`, `-1 ~x2`, `-1 ~x1`; RHS becomes `-1`; operator becomes
  `>=`.
- Step 2: rewrite each `c · ~x` as `(-c) · x + c`:
  - `-1 ~x3` → `+1 x3` plus LHS constant `-1`
  - `-1 ~x2` → `+1 x2` plus LHS constant `-1`
  - `-1 ~x1` → `+1 x1` plus LHS constant `-1`

  Now: terms `+1 x1`, `+1 x2`, `+1 x3`; LHS constants sum to `-3`;
  RHS = `-1`.
- Step 3: subtract the LHS-constant sum from the RHS:
  `-1 - (-3) = 2`.
- Steps 4–5: nothing to do.
- Step 6: sort → `[(x1, 1), (x2, 1), (x3, 1)]`, RHS = `2`.

Canonical form: `( [(x1,1), (x2,1), (x3,1)], 2 )`. ✓ matches A.

## Equality constraints

VeriPB rewrites `a = b` into the two inequalities `a >= b` and
`a <= b` and assigns them distinct line numbers. The right semantics
for comparing `=` against a `>=` / `<=` pair depends on whether
labels and line numbers must round-trip across that rewrite. We have
not yet committed to an answer, so the parser rejects `=` for now and
the comparison engine never sees it. Practical concern is low: the
use-cases we care about today do not emit equality lines.

When we revisit this, candidate behaviours include:
- Reject in both files (status quo).
- Expand `=` into two inequalities on read and compare against the
  pair, accepting whichever VeriPB order is canonical.
- Treat `=` as its own canonical kind and require both files to use
  the same form.

## What we deliberately do *not* do

- We **do not** scale coefficients by positive constants. A
  constraint with all coefficients doubled produces a different
  canonical form than the original, even though the constraints are
  mathematically equivalent. Reason: a solver that doubles its
  coefficients is exactly the kind of bug we want to surface, not
  paper over.
- We **do not** infer that one constraint is implied by another. We
  compare syntactic-after-normalisation equivalence, not logical
  entailment.
