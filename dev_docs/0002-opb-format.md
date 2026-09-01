# The OPB dialect we accept

> Drafted by AI assistant under human oversight. Last updated 2026-09-01.

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
| Reified      | `z1 ==> 1 x1 1 x2 >= 2 ;`        | Desugared, then compared   |
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

with whitespace between terms. The sign is attached to the
coefficient token (`+1`, `-1`) or omitted (interpreted as `+`).
Coefficients are non-zero integers. Literals are either `varname` for
a positive literal or `~varname` for the negation of a variable.

Variable names are any non-whitespace token that is not itself an
operator (`>=`, `<=`, `=`), the terminator (`;`), or a coefficient.
In practice the real-world OPB files we have seen use names like
`x1`, `y_x1_5`, `i[a][b0]`, `f[0][notequals]`, and
`x[money_a_d][0_1]` — alphanumeric with `_`, `[`, and `]`. We do
not impose a tighter grammar than "non-whitespace, not a reserved
sigil"; tightening can come later if upstream VeriPB nails down a
formal grammar.

A bare integer (a coefficient with no following literal) is treated
as an LHS constant.

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

## Reification shorthands

VeriPB accepts three shorthands that write a constraint as a
reification. They are syntactic sugar for ordinary pseudo-Boolean
constraints, and we desugar them during normalisation rather than
treating them as a kind of their own; see
[0003](0003-normalization.md#reification-shorthands) for exactly what
each one stands for.

```
z1 z2 ~z3 ==> +1 x1 +2 x2 >= 2 ;   % if z1 and z2 and not z3, then the constraint
z1        <== +1 x1 +2 x2 >= 2 ;   % if the constraint, then z1
z1       <==> +1 x1 +2 x2 >= 2 ;   % both of the above
```

The right implication (`==>`) takes one or more literals, read as
their conjunction. The left implication (`<==`) and the equivalence
(`<==>`) take exactly one literal. Anything else in front of an arrow
— a coefficient, say — is an error.

An equivalence stands for **two** constraints: the `==>` direction
followed by the `<==` direction. Everything downstream of the parser
therefore has to cope with one source line producing more than one
canonical constraint.

## Labels

A constraint line may be prefixed with `@name `, where `name` is a
non-whitespace identifier. The label is associated with that
constraint. Real labels we have seen include `@c[_1][le]`,
`@i[a][lb]`, and `@c[money_a_d][[1]gt[2]]` (note the nested
brackets), so the label name follows the same liberal
"non-whitespace token" rule as variable names. Labels on lines other
than constraints are rejected.

Since each label names one constraint, a line that stands for more
than one carries **one label for each**, and the i-th label names the
i-th constraint the line is loaded as. So

```
@right @left z1 <==> 1 x1 1 x2 >= 1 ;
```

names the `==>` direction `@right` and the `<==` direction `@left`.
A line carries either no labels at all or exactly as many as it has
constraints; any other number is an error, as it is in VeriPB.

Comment lines and labelled constraints can interleave freely; a
labelled constraint is just a constraint with leading label
tokens.

## Whitespace

Operators must be whitespace-separated: `1 x1 >= 1 ;`, not
`1 x1>=1;`. The single exception is the `;` terminator, which may be
attached to the preceding token because real files write it both
ways.

VeriPB's own OPB lexer needs no whitespace at all — `z1<==>1 x1 >= 1`
is legal there — but every file we have seen writes operators with
surrounding whitespace, and accepting an attached `<==>` while still
rejecting an attached `>=` would be a confusing half-measure. If this
ever bites, the fix is to tokenise properly, all operators at once.

## Terminator

Each constraint, objective, and `preserved:` line ends with a `;`.
In practice both ` ;` (with a leading space) and `;` (attached to
the previous token) occur in real OPB output, and both are accepted.
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
- A number of labels on a line that is neither zero nor one per
  constraint the line stands for.
- A left implication or equivalence with more than one literal in
  front of the arrow, or any implication with none.

## Examples

```
* header comment is fine
* #variable= 3 #constraint= 2

min: 1 x1 2 x2 ;
preserved: 1 x1 1 x2 ;

@card 1 x1 1 x2 1 x3 >= 2 ;
+1 ~x3 +1 ~x2 +1 ~x1 <= 1 ;

@right @left f[a][ge3] <==> 1 i[a][b0] 2 i[a][b1] 4 i[a][b2] >= 3 ;
```

The two constraints above are semantically equivalent. See
[0003](0003-normalization.md#worked-example) for the worked
normalisation.
