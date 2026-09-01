# Normalisation: defining semantic equivalence

> Drafted by AI assistant under human oversight. Last updated 2026-09-01.

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

## Reification shorthands

VeriPB's `==>`, `<==` and `<==>` are syntactic sugar (see
[0002](0002-opb-format.md#reification-shorthands)). We desugar them
into the constraints they stand for, so a file using a shorthand
compares equal to one writing the expansion out. The rules below are
taken from VeriPB itself — `veripb-parser/src/terms.rs` and
`GeneralPBConstraint::get_lit_reification` in
`veripb-formula/src/general_pb_constraint.rs`, with the worked
examples in its `proof_format_overview.md` — and every one of those
examples is a test in `src/model/normal.rs`.

### The two derived quantities

Reification carries the reified literals at a coefficient that depends
on the constraint's **normalised degree**, which is the degree VeriPB
uses internally: every coefficient made positive by rewriting
`-a · x` as `a · ~x - a`, which moves `a` to the right-hand side.
That is not the RHS of our canonical form, but it is computable from
it. Given a constraint already in canonical form `Σ kᵥ · v >= R`:

```
D  =  R + Σ |kᵥ|   over the negative coefficients      (normalised degree)
S  =  Σ |kᵥ|       over all coefficients               (coefficient sum)
```

**`D` has to be read off the constraint after normalisation.** Terms
that cancel each other change it, and a reification built on the
degree as written would be too strong. `z ==> 1 x2 1 ~x2 1 x1 >= 2`
has `D = 1`, not the 2 on the page.

### `l₁ … lₘ ==> C`

The conjunction of the literals implies `C`. Each literal is negated
and added to `C` with `C`'s degree as coefficient, so falsifying any
one of them satisfies the constraint on its own:

```
add the term  D · ~lⱼ  to the left-hand side for each lⱼ, keeping the RHS
```

then re-run the last steps of normalisation over the result, since a
reified literal may share a variable with the constraint it reifies.

If `D <= 0` the constraint is trivially satisfied, so it is implied by
anything and **loses its literals entirely** — carrying them at a
degree that is not positive would give a constraint that is too
strong. `z1 z2 ==> 1 x1 >= -1` is just `1 x1 >= -1`.

### `z <== C`

`C` implies `z`. Here it is `C` that is negated, and `z` is added to
that negation with the negation's degree as coefficient:

```
¬C     =  Σ (-kᵥ) · v >= 1 - R
D'     =  S - D + 1                     (the degree of ¬C)
result =  ¬C with the term D' · z added to the left-hand side
```

If `D > S` the constraint is contradicting, so it implies anything and
its (trivially satisfied) negation is the whole of what it stands for,
with no `z` term: `z1 <== 1 x1 >= 5` is `1 ~x1 >= -3`.

### `z <==> C`

Both of the above over the same `C`, as two constraints, in the order
VeriPB loads them: the `==>` direction first, then the `<==`
direction. This is the only line that produces two canonical
constraints, and it takes one label for each of them.

### Worked example

`z1 <== +1 x1 +2 x2 >= 2`, which VeriPB documents as loading to
`2 z1 1 ~x1 2 ~x2 >= 2`:

- Canonical `C`: `[(x1,1), (x2,2)]`, RHS `2`. No negative
  coefficients, so `D = 2`; `S = 1 + 2 = 3`.
- `D > S`? No (`2 <= 3`), so the literal stays.
- `¬C`: `[(x1,-1), (x2,-2)]`, RHS `1 - 2 = -1`.
- `D' = 3 - 2 + 1 = 2`; add `2 · z1`, a positive literal, so it only
  adds a term: `[(x1,-1), (x2,-2), (z1,2)]`, RHS `-1`.

Normalising VeriPB's `2 z1 1 ~x1 2 ~x2 >= 2` directly: the negated
literals contribute `-1 x1`, `-2 x2` and LHS constants `1 + 2 = 3`,
giving RHS `2 - 3 = -1` and the same term list. ✓

### What this does *not* paper over

We reproduce VeriPB's expansion exactly, which means a *different*
but also correct reification — the same constraint with a larger
big-M on the reified literal, say — is reported as a difference. That
is the same stance as the coefficient-scaling rule below: an encoder
that picks a different big-M is a difference worth seeing, not one to
hide.

## Equality constraints

VeriPB rewrites `a = b` into the two inequalities `a >= b` and
`a <= b` and assigns them distinct line numbers. The right semantics
for comparing `=` against a `>=` / `<=` pair depends on whether
labels and line numbers must round-trip across that rewrite. We have
not yet committed to an answer, so the parser rejects `=` for now and
the comparison engine never sees it. Practical concern is low: the
use-cases we care about today do not emit equality lines.

Note that the label rule already generalises to equalities: VeriPB
loads `@sum_lo @sum_hi 1 x1 1 x2 = 1 ;` as the `>=` direction named
`@sum_lo` and the `<=` direction named `@sum_hi`, exactly as an
equivalence names its two directions. The machinery a line standing
for two constraints needs is therefore already in place; only the
semantics of comparing `=` against a `>=` / `<=` pair is undecided.

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
  entailment. Desugaring a reification shorthand is not an exception:
  it produces the constraint VeriPB itself would load, and comparison
  still happens on canonical forms.
