# opbdiff

A semantic diff for [VeriPB][veripb]-extended OPB pseudo-Boolean files.

`opbdiff` compares two OPB files by what they *mean* rather than by their
bytes. For example, these two lines describe the same constraint and
`opbdiff` treats them as equivalent:

```
1 x1 1 x2 1 x3 >= 2 ;
+1 ~x3 +1 ~x2 +1 ~x1 <= 1 ;
```

Two intended uses:

- Comparing before-and-after OPB output when a change to a
  proof-generating solver causes unexpected breakage.
- Lining up output from a solver using its own encoder against output
  from an external encoder of the same problem.

## Status

`v0.2` — the originally-scoped v1 feature set plus a JSON output mode
(`--format json`). APIs and CLI flags are pre-1.0 and may change.

What works:

- Full parser for the OPB dialect VeriPB emits (see
  [`dev_docs/0002-opb-format.md`](dev_docs/0002-opb-format.md)).
- Canonical-form normalisation: `<=` flipped to `>=`, negated literals
  rewritten via `~x = 1 - x`, like terms combined, LHS constants moved
  to the RHS, zero-coefficient terms dropped, lexicographic sort. See
  [`dev_docs/0003-normalization.md`](dev_docs/0003-normalization.md).
- Ordered (by position) and `--unordered` (multiset) comparison.
- `--match-labels`: pair constraints by shared label first, then diff
  each pair's contents (with the remainder handled by the fallback
  mode).
- `--ignore-aux-names`: fold auxiliary-variable names (anything outside
  the projected `preserved:` set) so constraints that differ only in
  the names of their auxiliary variables compare equal.
- Directional `--check-labels` with `--reference A|B`.
- Plain-text reporter with a per-position breakdown and a summary line.
- JSON reporter (`--format json`) with a stable, versioned schema for
  scripts, tests, and feeding the diff to other tooling.
- 0 / 1 / 2 exit codes for use in scripts and tests.

Documented v1 limitations:

- Equality constraints (`=`) are rejected with a clear error.
  Practical concern is low; rationale in
  [`dev_docs/0003-normalization.md`](dev_docs/0003-normalization.md).
- Auxiliary-variable name differences across the two files are
  surfaced by default rather than silently equated, since spotting them
  is part of the use-case. Opt in to folding them with
  `--ignore-aux-names`, which compares auxiliary terms by coefficient
  only. Note this is a per-constraint check: it does not verify a
  single globally-consistent renaming of the auxiliary variables.
- Plain-text, colour-aware, and JSON reporters ship; side-by-side is
  still sketched in
  [`dev_docs/0005-output-formats.md`](dev_docs/0005-output-formats.md).

## Build

Requires a stable Rust toolchain (edition 2024, MSRV 1.85).

```
cargo build --release
```

The binary lands at `target/release/opbdiff`.

## Usage

```
opbdiff [OPTIONS] <A> <B>
```

Options:

| Flag                          | Effect                                                          |
|-------------------------------|-----------------------------------------------------------------|
| `-u`, `--unordered`           | Compare constraints as multisets (ignore order).                |
| `-m`, `--match-labels`        | Pair constraints by shared label first, then diff their contents; unmatched constraints fall back to the (un)ordered matching. |
| `--ignore-aux-names`          | Treat any variable not in the projected (`preserved:`) set as auxiliary and compare it by coefficient only, so differences in auxiliary-variable *names* don't count. Needs a `preserved:` line on at least one file (both must agree). |
| `-L`, `--check-labels`        | Enforce reference-side labels on the candidate.                 |
| `-r`, `--reference <A\|B>`    | Which side is the label reference (default `B`).                |
| `-f`, `--format <plain\|json>` | Output format. `plain` (default) is human-readable text; `json` is the machine-readable schema below. |
| `--color <auto\|always\|never>` | When to emit ANSI colour (applies to `plain` only). `auto` honours TTY and `NO_COLOR`. |

Exit codes:

| Code | Meaning                                  |
|-----:|------------------------------------------|
| `0`  | Files are semantically equivalent.       |
| `1`  | Files differ.                            |
| `2`  | Parse, I/O, or other usage error.        |

### Examples

Default ordered comparison:

```
$ opbdiff a.opb b.opb
Differing at constraint #11 (A line 14, B line 12):
  A: @c[_1][le] -2 i[a][b0] ... >= -1;
  B: @c[_1][ge]  1 i[e][b0] ... >=  1 ;
...
Summary (ordered): 10 matches, 4 differing, 0 only in A, 0 only in B.
```

Ignoring constraint order:

```
$ opbdiff --unordered a.opb b.opb
Files are semantically equivalent (14 constraints compared, unordered).
```

Pairing by label, then diffing each pair's contents. This is the most
useful mode when two encoders agree on labels but differ in constraint
order or in auxiliary-variable names — the label pins the pairing and
the canonical-form view isolates what actually differs:

```
$ opbdiff --match-labels --unordered a.opb b.opb
Differing at constraint #17 [@c[edge_0_1][gt]] (A line 21, B line 18):
  A: @c[edge_0_1][gt] ... 8 ~f[0][ne] >= 1 ;
  B: @c[edge_0_1][gt] ... 8 ~b[edge_0_1][ne] >= 1 ;
  canonical-form view (sorted):
    b[edge_0_1][ne]   A=(absent)   B=-8
    f[0][ne]          A=-8         B=(absent)
    (7 identical rows hidden)
...
Summary (label-matched, unordered fallback): 16 matches, 28 differing, 0 only in A, 0 only in B.
```

Ignoring auxiliary-variable names. When two encoders introduce the
same auxiliary variables under different names (e.g. `f[10][…]` vs
`x[colours_def][0]`), folding them away leaves only the genuine
differences:

```
$ opbdiff --unordered --ignore-aux-names a.opb b.opb
Only in A (line 77):
  A: @c[colours_def][0ge] ... 8 f[10][arrayminmax0] >= 1 ;
...
Summary (unordered, aux names ignored): 51 matches, 0 differing, 7 only in A, 0 only in B, objective differs, preserved differs.
```

Here only the truly structural difference survives the fold (one
encoder emits a reverse-reification half the other omits), plus the
objective and `preserved:` differences. "Auxiliary" means any variable
not in the projected `preserved:` set; at least one file must carry
that line, and if both do they must agree.

Enforcing labels with `b.opb` as the reference:

```
$ opbdiff --unordered --check-labels a.opb b.opb
Label mismatch at constraint A#1 / B#1 (reference=B):
  expected label: i[a][lb]
  actual label:   (none)
  ...
Summary (unordered): 4 matches, 0 differing, 0 only in A, 0 only in B, 10 label mismatches.
```

### JSON output

`--format json` emits a stable, versioned serialisation of the diff
for scripts, tests, and other tooling. The top-level `equivalent`
boolean mirrors the exit code (`true` ⇔ exit `0`), so a "are these two
files effectively the same?" check needs nothing more than that field.
Only *differences* appear in `constraints` (matched constraints are
omitted, just as in the text report), so an equivalent pair is a tiny
payload; `summary.matches` carries the matched count. The full schema
lives in [`dev_docs/0005-output-formats.md`](dev_docs/0005-output-formats.md)
and in the `report::json` module doc comment.

```
$ opbdiff --format json a.opb b.opb
{
  "schema_version": 1,
  "tool_version": "0.2.0",
  "equivalent": false,
  "comparison": { "mode": "ordered", "matched_by_label": false,
                  "aux_names_ignored": false, "projected_variables": null },
  "summary": { "matches": 1, "differing": 1, "only_in_a": 0, "only_in_b": 0,
               "label_mismatches": 0, "objective_difference": false,
               "preserved_difference": false },
  "objective": { "kind": "both_absent" },
  "preserved": { "kind": "both_absent" },
  "constraints": [
    {
      "kind": "differ", "index_a": 1, "index_b": 1,
      "a": { "label": null, "line": 2, "raw": "1 x1 1 x2 >= 2 ;",
             "form": { "terms": [ { "variable": "x1", "coefficient": 1 },
                                  { "variable": "x2", "coefficient": 1 } ], "rhs": 2 } },
      "b": { "label": null, "line": 2, "raw": "1 x1 2 x2 >= 2 ;",
             "form": { "terms": [ { "variable": "x1", "coefficient": 1 },
                                  { "variable": "x2", "coefficient": 2 } ], "rhs": 2 } },
      "term_diff": { "variables": [ { "variable": "x2", "a": 1, "b": 2 } ], "rhs": null }
    }
  ]
}
```

## Development notes

Active design and implementation notes live in
[`dev_docs/`](dev_docs/), including the OPB dialect we accept, the
normalisation rules, the comparison algorithm, and the staged output
format roadmap.

This project is developed with significant assistance from an AI
coding assistant (Claude, by Anthropic) under human oversight. Every
commit carries a `Co-Authored-By` trailer identifying the AI
contributor; every dev_doc opens with the same disclosure. Factual
claims about VeriPB or third-party tooling are worth re-verifying
against upstream.

CI runs `cargo fmt --check`, `cargo clippy -D warnings`, the full
test suite, and `cargo doc -D rustdoc::*` on every push and PR.

## Licence

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the
Apache-2.0 licence, shall be dual licensed as above, without any
additional terms or conditions.

[veripb]: https://gitlab.com/MIAOresearch/software/VeriPB
