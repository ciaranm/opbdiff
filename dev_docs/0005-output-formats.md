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
environment variable is honoured automatically. Stages 3 and 4 (JSON
and side-by-side) are not yet implemented.

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

## Stage 3: JSON

Machine-readable serialisation of the diff result. Stable schema
documented separately. Useful for CI, scripts, and structured
post-processing. Uses `serde` with `serde_json`.

The schema will distinguish all the entry kinds the comparison engine
produces (matched, differing, extra-in-A/B, only-in-A/B,
label-mismatch, objective-difference, etc.) so consumers do not have
to re-parse the human output.

## Stage 4: side-by-side

Two-column aligned view with differences highlighted. The largest
implementation effort of the four; the lowest priority because the
plain reporter already conveys the same content less prettily. Will
likely depend on terminal-width detection.

## Selection

`--format <plain|colour|json|side-by-side>`. Default behaviour:

- `colour` if stdout is a TTY,
- `plain` otherwise.

JSON and side-by-side are never picked implicitly. `--no-color` is a
shorthand for forcing `plain` when colour would otherwise be chosen.
