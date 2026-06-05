# Output formats

> Drafted by AI assistant under human oversight. Last updated 2026-05-29.

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
`matched_by_label`, `aux_names_ignored`, `projected_variables`),
`summary` (the flat tally), `objective`, `preserved`, and
`constraints`.

`objective` and `preserved` are tagged records with `kind` ∈
`{both_absent, match, differ, only_in_a, only_in_b}`. Each side carries
`line`, `raw`, and the canonical content (`terms` for objectives;
`literals` for preserved).

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
