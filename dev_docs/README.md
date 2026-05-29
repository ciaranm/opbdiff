# opbdiff development notes

This directory contains design and implementation notes for the `opbdiff`
tool. Documents are numbered to give a reading order but are otherwise
flat. Read in numeric order for a tour from architecture to specific
algorithms to roadmap.

## AI collaboration disclosure

These documents are drafted by an AI coding assistant (Claude Opus 4.7,
by Anthropic) operating under human oversight. The human in the loop
reviews and directs changes. Where a document records a decision, the
decision was discussed with and approved by the human; the AI drafted
the wording. Treat factual claims about VeriPB or third-party tooling
with normal scepticism: confirm against upstream before relying on
them.

## Index

- [0001 — Architecture](0001-architecture.md): the pipeline from input
  files to a diff result, and how the crate is laid out.
- [0002 — OPB dialect we accept](0002-opb-format.md): which OPB
  constructs the parser handles, what we ignore, what we reject.
- [0003 — Normalisation](0003-normalization.md): the canonical-form
  transformation that defines semantic equivalence.
- [0004 — Comparison algorithm](0004-comparison-algorithm.md): ordered
  vs unordered modes and label handling.
- [0005 — Output formats](0005-output-formats.md): staged rollout
  plan for plain, colour, JSON, side-by-side.
- [0006 — VeriPB integration survey](0006-veripb-integration-survey.md):
  stub for tracking what is reusable from VeriPB upstream.

## Conventions

- Filenames carry a four-digit prefix that fixes reading order. New
  documents take the next number; insertion between numbers is fine if
  a topic needs a natural neighbour.
- Each document opens with a one-line statement of its purpose and a
  note on AI authorship.
- Code identifiers refer to symbols as they exist *now*; rename the
  references when the code changes.
- Decisions that flow from a conversation with the human are recorded
  as decisions, not options; the option discussion belongs in the
  commit message or PR description, not in evergreen docs.
