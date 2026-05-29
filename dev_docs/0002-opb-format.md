# The OPB dialect we accept

> Drafted by AI assistant under human oversight. Last updated 2026-05-29.

This document defines the subset of OPB (the pseudo-Boolean
DIMACS-style format) that `opbdiff` understands. We follow
[VeriPB's][veripb] usage where the bare standard is ambiguous.

[veripb]: https://gitlab.com/MIAOresearch/software/VeriPB

## Lines

An OPB file is a sequence of lines. Each line is one of:

| Kind         | Example                          | Treatment                  |
|--------------|----------------------------------|----------------------------|
| Comment      | `* this is a comment`            | Parsed, ignored            |
| Constraint   | `1 x1 1 x2 >= 2 ;`               | Compared semantically      |
| Labelled     | `@cardinality 1 x1 1 x2 >= 2 ;`  | Compared; label tracked    |
| Objective    | `min: 3 x1 2 x2 ;`               | Compared semantically      |
| Preserved    | `preserved: 1 x1 1 x2 ;`         | Compared semantically      |
| Blank        | (empty)                          | Skipped                    |

The conventional header `* #variable= N #constraint= M` is a comment
and is treated as such — purely for human reading. Differences in
header content or other comments **do not** cause a diff. This matches
the human's note that one of the tools we want to compare emits the
header and one does not.

## Terms

A constraint or objective body is a sequence of terms, each of the
form:

```
<sign>? <coefficient> <literal>
```

with whitespace between terms. The sign defaults to `+` if absent.
Coefficients are non-zero integers. Literals are either `varname` for
a positive literal or `~varname` for the negation of a variable.
Variable names are sequences of printable non-whitespace characters
starting with a letter or underscore (we accept VeriPB's typical
form `x1`, `x42`, `aux_foo`, etc.).

## Operators

For constraints: `>=`, `<=`. Equality `=` is **not supported in v1**
and is rejected with a clear error; see
[0003](0003-normalization.md#equality-constraints) for the reasoning.

For `min:` and `preserved:`, there is no operator — the body is
purely a term sequence.

## Constants

A bare integer in the term list is treated as an LHS constant that is
moved to the RHS during normalisation. See
[0003](0003-normalization.md).

## Labels

A constraint line may be prefixed with `@name `, where `name` is a
non-whitespace identifier. The label is associated with that
constraint. Labels on lines other than constraints are not part of v1.

## Terminator

Each constraint, objective, and `preserved:` line ends with ` ;`.
Lines that omit the terminator are rejected.

## What we reject

- Equality constraints (`=`). VeriPB internally turns `a = b` into
  two inequalities with specific line-number semantics, and the right
  answer for comparing `=` against a `>=` / `<=` pair depends on
  whether labels and line numbers must round-trip across that rewrite.
  We defer a decision; in practice the human reports that the OPB
  files of interest do not currently emit `=`.
- Non-integer coefficients (OPB is integer-only).
- Lines that mix label syntax with non-constraint roles.

## Examples

```
* header comment is fine
* #variable= 3 #constraint= 2

min: 1 x1 2 x2 ;
preserved: 1 x1 1 x2 ;

@card 1 x1 1 x2 1 x3 >= 2 ;
+1 ~x3 +1 ~x2 +1 ~x1 <= 1 ;
```

The two constraints above are semantically equivalent. See
[0003](0003-normalization.md#worked-example) for the worked
normalisation.
