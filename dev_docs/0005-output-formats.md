# Output formats

> Drafted by AI assistant under human oversight. Last updated 2026-09-01.

`opbdiff`'s diff result is a structured value that lives in the
library. Reporters render it for output. We plan four reporters; v1
ships only the first, but the abstraction is designed so the later
ones drop in without touching the comparison engine.

## Status

Stages 1 and 2 (plain text + colour-aware) are shipped, served by
the same reporter: the reporter unconditionally emits ANSI SGR codes
and `anstream::AutoStream` around stdout strips them when not on a
TTY (or when the user passed `--color=never`). The `NO_COLOR`
environment variable is honoured automatically. Stage 3 (JSON) is
shipped — see below for the schema. Stage 4 (side-by-side) is not yet
implemented.

## Stage 1: plain text (v1)

Goal: maximally portable, pipeable, no extra dependencies, easy to
read in any terminal or log file.

Sketch:

```
Differing at constraint #4:
  A: 1 x1 1 x2 >= 2 ;
  B: 1 x1 2 x2 >= 2 ;
Extra in A (after constraint #7):
  A: 1 x9 >= 1 ;
Objectives differ:
  A: min: 1 x1 2 x2 ;
  B: min: 1 x1 ;
Label mismatch at constraint #2 (reference=B):
  expected: @card1
  actual:   @card2

Summary: 1 differing, 1 extra in A, 1 objective difference, 1 label mismatch.
```

Implementation: a `report::plain` module writes to any
`std::io::Write`. No colour, no styling, no dependencies beyond `std`.

## Stage 2: colour-aware text

Same content, ANSI colouring when stdout is a TTY (red for "only in
A" / "differs", green for "only in B", yellow for "label mismatch").
Auto-disabled with `--no-color`, when the `NO_COLOR` env var is set,
or when stdout is not a TTY. Add a small terminal-detection crate at
this stage (`anstream` or similar) rather than rolling our own.

## Stage 3: JSON (shipped)

Machine-readable serialisation of the diff result, selected with
`--format json`. Useful for CI, scripts, structured post-processing,
and — a primary motivating use case — feeding the diff to another
agent (e.g. another Claude) that needs to either understand what
differs or assert that two files are "effectively the same" without
re-parsing the human output. Uses `serde` with `serde_json`; output is
pretty-printed and never coloured.

### Design

The wire format is **not** a `#[derive(Serialize)]` slapped onto the
internal compare types. The `report::json` module defines a separate
set of view structs that map *from* `DiffResult`. This decouples the
schema from the in-memory representation: the comparison engine can
rename fields or restructure enums without silently breaking
consumers, and the schema carries its own `schema_version` integer
(currently `1`) to bump on any breaking change. The module doc comment
is the authoritative schema reference; `report::json::SCHEMA_VERSION`
is the single source of truth for the version number.

### Shape

Top level: `schema_version`, `tool_version`, `equivalent` (mirrors the
exit code: `0` ⇔ `true`, `1` ⇔ `false`), `comparison` (mode,
`matched_by_label`, `aux_names_ignored`, `projected_variables`,
`ignored_missing_preserved`), `summary` (the flat tally), `objective`,
`preserved`, `constraints`, and `label_permutation`.

`objective` and `preserved` are tagged records with `kind` ∈
`{both_absent, match, differ, only_in_a, only_in_b}`. Each side carries
`line`, `raw`, and the canonical content (`terms` for objectives;
`literals` for preserved).

`comparison.ignored_missing_preserved` is `"a"`/`"b"` when a missing
`preserved:` line on that file was ignored via
`--ignore-no-preserved-in`, else `null`. When set, `preserved` still
reports the true `only_in_*` finding (the data is never hidden), but
`summary.preserved_difference` is `false` and `equivalent` disregards
the absence. This field was added in schema version 1 as an additive,
opt-in-only field, so it does not change output for runs that don't use
the flag.

`constraints` is an array of tagged records with `kind` ∈ `{differ,
only_in_a, only_in_b, label_mismatch}`. **Matched constraints are
omitted**, exactly as the plain reporter omits them, so an equivalent
pair yields an empty array and the payload stays proportional to the
diff rather than to the file size (important both for token budgets
when an agent reads it and for diffing JSON in tests). The count of
matched constraints is still available as `summary.matches`. Each side
(`a` / `b`) carries `label`, `line`, `raw`, and `form` (`{terms, rhs}`
where each term is `{variable, coefficient}`). A `differ` entry adds
`term_diff`: the pinpointed delta listing only the variables whose
coefficients differ (each `{variable, a, b}`, with `null` where a side
omits the variable) plus `rhs` (a `{a, b}` object only when the RHS
differs, else `null`). Unlike the plain reporter, the JSON `term_diff`
does *not* fold auxiliary variables into a multiset row — it reports
every differing variable by name and leaves interpretation to the
consumer, which can consult `comparison.projected_variables`.

`label_permutation` is `null` except under `--match-labels` when there
were label-paired *differing* constraints to analyse. When present it is
an object with `all_differing_explained` (bool), `swaps` (count of
length-2 cycles), `correspondences` (`[{a_label, b_label, swap}]`, where
`A@a_label` ≡ `B@b_label`), `cycles` (the permutation as disjoint label
cycles), and `unexplained` (differing labels that found no cross-match).
It answers "do the encodings agree up to label naming?" via
`all_differing_explained`, but is informational only: a permuted label is
still a real disagreement, so `equivalent` stays `false`. Like
`ignored_missing_preserved`, it was added as an additive field within
schema version 1, so runs that don't use `--match-labels` are
unaffected. See `dev_docs/0004-comparison-algorithm.md` for how the
permutation is computed.

### Constraint sides

Each side of a constraint entry carries `label`, `line`, `raw`, `part`
and `form`. `part` is `"right_implication"`, `"left_implication"`, or
`null`: an equivalence (`<==>`) line stands for two constraints, so
two entries can share a `line` and `raw`, and a consumer keying on the
source line needs `part` to tell them apart. It was added as an
additive field within schema version 1 and is `null` for every file
that uses no reification shorthands.

## Stage 4: side-by-side

Two-column aligned view with differences highlighted. The largest
implementation effort of the four; the lowest priority because the
plain reporter already conveys the same content less prettily. Will
likely depend on terminal-width detection.

## Selection

The original sketch folded colour into the format flag
(`--format <plain|colour|json|side-by-side>`). The shipped design
splits the two concerns instead, because colour turned out to be a
property of the *stream* (handled by `anstream::AutoStream`), not of
the *content*:

- `--format <plain|json>` selects the content. Default `plain`.
  `side-by-side` is not yet implemented.
- `--color <auto|always|never>` controls ANSI colour, and applies to
  `plain` only. `auto` keeps colour for a TTY and strips it otherwise;
  `NO_COLOR` forces stripping.

JSON is never picked implicitly and is never coloured (it bypasses the
colour stream and writes straight to stdout, so `--color=always` can
never corrupt the payload). This keeps colour orthogonal to format
rather than multiplying the format list by a colour variant.
