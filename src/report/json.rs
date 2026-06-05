//! JSON reporter: a stable, machine-readable serialisation of a
//! [`DiffResult`].
//!
//! The audience is *programs*, not people: another tool (or another
//! Claude) that needs to either understand what differs between two OPB
//! files or assert that they are "effectively the same" without
//! re-parsing the human-readable output. The two questions it answers
//! directly:
//!
//! * *Are these files equivalent?* — the top-level `equivalent` boolean,
//!   which mirrors the process exit code (0 ⇔ `true`, 1 ⇔ `false`).
//! * *If not, how do they differ?* — the `constraints` array plus the
//!   `objective` / `preserved` objects, each a tagged record carrying
//!   both sides' canonical form and, for a content difference, the
//!   pinpointed term-level delta.
//!
//! ## Schema stability
//!
//! The wire format is **not** a derived dump of the internal compare
//! types; it is an explicit set of `#[derive(Serialize)]` view structs
//! in this module that map *from* [`DiffResult`]. That decoupling lets
//! the comparison engine evolve its in-memory representation without
//! silently breaking consumers, and lets the schema carry its own
//! version (`schema_version`). Bump [`SCHEMA_VERSION`] on any
//! breaking change to the shape below.
//!
//! ## Shape (schema version 1)
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "tool_version": "0.3.0",
//!   "equivalent": false,
//!   "comparison": {
//!     "mode": "ordered",            // or "unordered"
//!     "matched_by_label": false,
//!     "aux_names_ignored": false,
//!     "projected_variables": null,  // sorted [String] when folding, else null
//!     "ignored_missing_preserved": null  // "a"/"b" when that file's missing preserved: line was ignored
//!   },
//!   "summary": {
//!     "matches": 3, "differing": 1, "only_in_a": 0, "only_in_b": 0,
//!     "label_mismatches": 0,
//!     "objective_difference": false, "preserved_difference": false
//!   },
//!   "objective": { "kind": "match" },
//!   "preserved": { "kind": "both_absent" },
//!   "constraints": [
//!     {
//!       "kind": "differ",
//!       "index_a": 3, "index_b": 3,
//!       "a": { "label": null, "line": 4, "raw": "...",
//!              "form": { "terms": [{ "variable": "x1", "coefficient": 1 }], "rhs": 2 } },
//!       "b": { ... },
//!       "term_diff": {
//!         "variables": [ { "variable": "x2", "a": 1, "b": 2 } ],
//!         "rhs": null                           // { "a": N, "b": M } when the RHS differs
//!       }
//!     }
//!   ],
//!   "label_permutation": null    // object under --match-labels when labels are permuted
//! }
//! ```
//!
//! Under `--match-labels`, when constraints paired by equal label
//! disagree, `label_permutation` reports whether those differences are
//! explained by a *permutation* of labels — A's constraint labelled `L`
//! being canonically identical to B's constraint labelled `M`:
//!
//! ```json
//! "label_permutation": {
//!   "all_differing_explained": true,
//!   "swaps": 2,
//!   "correspondences": [
//!     { "a_label": "c[_1][posle]", "b_label": "c[_1][posge]", "swap": true },
//!     { "a_label": "c[_1][posge]", "b_label": "c[_1][posle]", "swap": true }
//!   ],
//!   "cycles": [ ["c[_1][posle]", "c[_1][posge]"] ],
//!   "unexplained": []
//! }
//! ```
//!
//! It is `null` outside `--match-labels` and when no two equally-labelled
//! constraints disagreed. It is *informational only*: a permuted label is
//! still a genuine label disagreement, so `equivalent` stays `false`. A
//! consumer asking "do the encodings agree up to label naming?" reads
//! `label_permutation.all_differing_explained`. Added as an additive
//! field within schema version 1, so runs that don't use `--match-labels`
//! are unaffected.
//!
//! Only *differences* appear in `constraints` — matched constraints are
//! omitted, exactly as the plain reporter omits them, so an equivalent
//! pair yields an empty array and the payload stays proportional to the
//! diff rather than to the file size. Use `summary.matches` for the
//! count of constraints that matched. The `kind` discriminants a
//! constraint entry can take are `differ`, `only_in_a`, `only_in_b`,
//! and `label_mismatch`; `objective` / `preserved` additionally use
//! `match` and `both_absent`.
//!
//! Note that `term_diff` does *not* fold auxiliary variables: unlike the
//! plain reporter (which collapses them into a multiset row for
//! readability), the JSON view lists every differing variable by name,
//! aux included, and leaves interpretation to the consumer via
//! `comparison.projected_variables`.
//!
//! When `comparison.ignored_missing_preserved` is set (via
//! `--ignore-no-preserved-in`), `preserved` still reports the true
//! structural outcome — e.g. `{"kind": "only_in_b", ...}` — so the data
//! is never hidden, but `summary.preserved_difference` is `false` and
//! `equivalent` ignores the absence. A consumer that wants the headline
//! verdict reads `equivalent`; one auditing the raw finding reads
//! `preserved`.

use std::io;

use serde::Serialize;

use crate::compare::{
    CompareMode, ConstraintDiff, DiffResult, LabelPermutation, ObjectiveDiff, PreservedDiff,
    ReferenceSide,
};
use crate::model::{
    CanonicalConstraint, CanonicalLabelledConstraint, CanonicalObjectiveItem,
    CanonicalPreservedItem,
};

/// Version of the JSON schema this module emits. Bump on any breaking
/// change to the serialised shape so consumers can guard on it.
pub const SCHEMA_VERSION: u32 = 1;

// ---- entry point ------------------------------------------------------

/// Serialise `diff` as pretty-printed JSON, terminated with a newline.
///
/// Mirrors [`crate::report::plain::write`]'s signature so the binary can
/// pick a reporter without special-casing the sink. The output is never
/// coloured.
pub fn write(out: &mut dyn io::Write, diff: &DiffResult) -> io::Result<()> {
    let report = Report::from_diff(diff);
    serde_json::to_writer_pretty(&mut *out, &report).map_err(io::Error::other)?;
    writeln!(out)
}

// ---- top-level report -------------------------------------------------

#[derive(Serialize)]
struct Report<'a> {
    schema_version: u32,
    tool_version: &'static str,
    /// True iff the two files are semantically equivalent under the
    /// chosen options; identical to the `0` exit-code condition.
    equivalent: bool,
    comparison: Comparison<'a>,
    summary: SummaryJson,
    objective: ObjectiveJson<'a>,
    preserved: PreservedJson<'a>,
    constraints: Vec<ConstraintJson<'a>>,
    /// Present (non-`null`) only under `--match-labels` when there were
    /// label-paired *differing* constraints to analyse; see
    /// [`LabelPermutationJson`]. Always `null` otherwise.
    label_permutation: Option<LabelPermutationJson<'a>>,
}

impl<'a> Report<'a> {
    fn from_diff(diff: &'a DiffResult) -> Self {
        let constraints = diff
            .constraints
            .iter()
            .filter_map(ConstraintJson::from_diff)
            .collect();
        Report {
            schema_version: SCHEMA_VERSION,
            tool_version: env!("CARGO_PKG_VERSION"),
            equivalent: diff.is_equivalent(),
            comparison: Comparison::from_diff(diff),
            summary: SummaryJson::from_diff(diff),
            objective: ObjectiveJson::from_diff(&diff.objective),
            preserved: PreservedJson::from_diff(&diff.preserved),
            constraints,
            label_permutation: diff
                .label_permutation
                .as_ref()
                .map(LabelPermutationJson::from_perm),
        }
    }
}

#[derive(Serialize)]
struct Comparison<'a> {
    mode: &'static str,
    matched_by_label: bool,
    aux_names_ignored: bool,
    /// The projected (`preserved:`) variable set in force when auxiliary
    /// names were folded, sorted for determinism; `null` otherwise.
    projected_variables: Option<Vec<&'a str>>,
    /// `"a"` or `"b"` when a one-sided missing `preserved:` line on that
    /// file was ignored (`--ignore-no-preserved-in`); `null` otherwise.
    /// When set, `preserved` still reports the true `only_in_*` outcome,
    /// but `summary.preserved_difference` is `false` and `equivalent`
    /// disregards the absence.
    ignored_missing_preserved: Option<&'static str>,
}

impl<'a> Comparison<'a> {
    fn from_diff(diff: &'a DiffResult) -> Self {
        let projected_variables = diff.aux_projection.as_ref().map(|set| {
            let mut vars: Vec<&str> = set.iter().map(String::as_str).collect();
            vars.sort_unstable();
            vars
        });
        Comparison {
            mode: mode_str(diff.mode),
            matched_by_label: diff.matched_by_label,
            aux_names_ignored: diff.aux_projection.is_some(),
            projected_variables,
            ignored_missing_preserved: diff.ignored_missing_preserved.map(|side| match side {
                ReferenceSide::A => "a",
                ReferenceSide::B => "b",
            }),
        }
    }
}

fn mode_str(mode: CompareMode) -> &'static str {
    match mode {
        CompareMode::Ordered => "ordered",
        CompareMode::Unordered => "unordered",
    }
}

#[derive(Serialize)]
struct SummaryJson {
    matches: usize,
    differing: usize,
    only_in_a: usize,
    only_in_b: usize,
    label_mismatches: usize,
    objective_difference: bool,
    preserved_difference: bool,
}

impl SummaryJson {
    fn from_diff(diff: &DiffResult) -> Self {
        let s = diff.summary();
        SummaryJson {
            matches: s.matches,
            differing: s.differing,
            only_in_a: s.only_in_a,
            only_in_b: s.only_in_b,
            label_mismatches: s.label_mismatches,
            objective_difference: s.objective_difference,
            preserved_difference: s.preserved_difference,
        }
    }
}

// ---- canonical content views -----------------------------------------

/// A canonical constraint as `{ terms: [{variable, coefficient}], rhs }`.
#[derive(Serialize)]
struct FormJson<'a> {
    terms: Vec<TermJson<'a>>,
    rhs: i64,
}

#[derive(Serialize)]
struct TermJson<'a> {
    variable: &'a str,
    coefficient: i64,
}

impl<'a> FormJson<'a> {
    fn from_form(form: &'a CanonicalConstraint) -> Self {
        FormJson {
            terms: form.terms.iter().map(TermJson::from_pair).collect(),
            rhs: form.rhs,
        }
    }
}

impl<'a> TermJson<'a> {
    fn from_pair((variable, coefficient): &'a (String, i64)) -> Self {
        TermJson {
            variable,
            coefficient: *coefficient,
        }
    }
}

/// A labelled constraint with the source metadata a consumer needs to
/// locate it (`line`, `raw`) plus its semantic `form`.
#[derive(Serialize)]
struct ConstraintSide<'a> {
    label: Option<&'a str>,
    line: usize,
    raw: &'a str,
    form: FormJson<'a>,
}

impl<'a> ConstraintSide<'a> {
    fn from_constraint(c: &'a CanonicalLabelledConstraint) -> Self {
        ConstraintSide {
            label: c.label.as_deref(),
            line: c.line,
            raw: c.raw.trim(),
            form: FormJson::from_form(&c.form),
        }
    }
}

#[derive(Serialize)]
struct ObjectiveSide<'a> {
    line: usize,
    raw: &'a str,
    /// Sorted objective terms; objectives carry no RHS.
    terms: Vec<TermJson<'a>>,
}

impl<'a> ObjectiveSide<'a> {
    fn from_item(item: &'a CanonicalObjectiveItem) -> Self {
        ObjectiveSide {
            line: item.line,
            raw: item.raw.trim(),
            terms: item.form.terms.iter().map(TermJson::from_pair).collect(),
        }
    }
}

#[derive(Serialize)]
struct PreservedSide<'a> {
    line: usize,
    raw: &'a str,
    literals: Vec<LiteralJson<'a>>,
}

#[derive(Serialize)]
struct LiteralJson<'a> {
    variable: &'a str,
    negated: bool,
}

impl<'a> PreservedSide<'a> {
    fn from_item(item: &'a CanonicalPreservedItem) -> Self {
        PreservedSide {
            line: item.line,
            raw: item.raw.trim(),
            literals: item
                .form
                .literals
                .iter()
                .map(|(variable, negated)| LiteralJson {
                    variable,
                    negated: *negated,
                })
                .collect(),
        }
    }
}

// ---- objective / preserved diffs -------------------------------------

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ObjectiveJson<'a> {
    BothAbsent,
    Match,
    Differ {
        a: ObjectiveSide<'a>,
        b: ObjectiveSide<'a>,
    },
    OnlyInA {
        a: ObjectiveSide<'a>,
    },
    OnlyInB {
        b: ObjectiveSide<'a>,
    },
}

impl<'a> ObjectiveJson<'a> {
    fn from_diff(o: &'a ObjectiveDiff) -> Self {
        match o {
            ObjectiveDiff::BothAbsent => ObjectiveJson::BothAbsent,
            ObjectiveDiff::Match => ObjectiveJson::Match,
            ObjectiveDiff::Differ { a, b } => ObjectiveJson::Differ {
                a: ObjectiveSide::from_item(a),
                b: ObjectiveSide::from_item(b),
            },
            ObjectiveDiff::OnlyInA(a) => ObjectiveJson::OnlyInA {
                a: ObjectiveSide::from_item(a),
            },
            ObjectiveDiff::OnlyInB(b) => ObjectiveJson::OnlyInB {
                b: ObjectiveSide::from_item(b),
            },
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PreservedJson<'a> {
    BothAbsent,
    Match,
    Differ {
        a: PreservedSide<'a>,
        b: PreservedSide<'a>,
    },
    OnlyInA {
        a: PreservedSide<'a>,
    },
    OnlyInB {
        b: PreservedSide<'a>,
    },
}

impl<'a> PreservedJson<'a> {
    fn from_diff(p: &'a PreservedDiff) -> Self {
        match p {
            PreservedDiff::BothAbsent => PreservedJson::BothAbsent,
            PreservedDiff::Match => PreservedJson::Match,
            PreservedDiff::Differ { a, b } => PreservedJson::Differ {
                a: PreservedSide::from_item(a),
                b: PreservedSide::from_item(b),
            },
            PreservedDiff::OnlyInA(a) => PreservedJson::OnlyInA {
                a: PreservedSide::from_item(a),
            },
            PreservedDiff::OnlyInB(b) => PreservedJson::OnlyInB {
                b: PreservedSide::from_item(b),
            },
        }
    }
}

// ---- per-constraint diff ---------------------------------------------

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ConstraintJson<'a> {
    Differ {
        index_a: usize,
        index_b: usize,
        a: ConstraintSide<'a>,
        b: ConstraintSide<'a>,
        /// The pinpointed term-level delta, mirroring the plain
        /// reporter's canonical-form view: only the variables (and RHS,
        /// and folded aux multisets) that actually differ.
        term_diff: TermDiffJson<'a>,
    },
    OnlyInA {
        index: usize,
        a: ConstraintSide<'a>,
    },
    OnlyInB {
        index: usize,
        b: ConstraintSide<'a>,
    },
    LabelMismatch {
        index_a: usize,
        index_b: usize,
        /// Which side carried the reference label that was not honoured.
        reference: &'static str,
        expected_label: Option<&'a str>,
        actual_label: Option<&'a str>,
        a: ConstraintSide<'a>,
        b: ConstraintSide<'a>,
    },
}

impl<'a> ConstraintJson<'a> {
    /// `None` for matched constraints, which are omitted from the array.
    fn from_diff(d: &'a ConstraintDiff) -> Option<Self> {
        match d {
            ConstraintDiff::Match { .. } => None,
            ConstraintDiff::Differ {
                index_a,
                index_b,
                a,
                b,
            } => Some(ConstraintJson::Differ {
                index_a: *index_a,
                index_b: *index_b,
                term_diff: TermDiffJson::between(&a.form, &b.form),
                a: ConstraintSide::from_constraint(a),
                b: ConstraintSide::from_constraint(b),
            }),
            ConstraintDiff::OnlyInA { index, a } => Some(ConstraintJson::OnlyInA {
                index: *index,
                a: ConstraintSide::from_constraint(a),
            }),
            ConstraintDiff::OnlyInB { index, b } => Some(ConstraintJson::OnlyInB {
                index: *index,
                b: ConstraintSide::from_constraint(b),
            }),
            ConstraintDiff::LabelMismatch {
                index_a,
                index_b,
                a,
                b,
                reference,
            } => {
                let (expected_label, actual_label, reference) = match reference {
                    ReferenceSide::A => (a.label.as_deref(), b.label.as_deref(), "a"),
                    ReferenceSide::B => (b.label.as_deref(), a.label.as_deref(), "b"),
                };
                Some(ConstraintJson::LabelMismatch {
                    index_a: *index_a,
                    index_b: *index_b,
                    reference,
                    expected_label,
                    actual_label,
                    a: ConstraintSide::from_constraint(a),
                    b: ConstraintSide::from_constraint(b),
                })
            }
        }
    }
}

/// The term-level delta between two canonical forms. Only differing
/// variables are listed; `rhs` and `aux` are present only when they
/// themselves differ, so the structure carries exactly the difference
/// and no agreeing rows.
#[derive(Serialize)]
struct TermDiffJson<'a> {
    variables: Vec<VarDeltaJson<'a>>,
    rhs: Option<RhsDeltaJson>,
}

#[derive(Serialize)]
struct VarDeltaJson<'a> {
    variable: &'a str,
    /// This variable's coefficient on each side; `null` when the side
    /// does not reference the variable at all.
    a: Option<i64>,
    b: Option<i64>,
}

#[derive(Serialize)]
struct RhsDeltaJson {
    a: i64,
    b: i64,
}

impl<'a> TermDiffJson<'a> {
    /// Compare two canonical forms by variable. Mirrors the plain
    /// reporter's row collection, but does not fold auxiliary names:
    /// the JSON consumer gets every differing variable by name, even
    /// auxiliaries, and decides for itself what to make of them.
    fn between(a: &'a CanonicalConstraint, b: &'a CanonicalConstraint) -> Self {
        let mut variables = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < a.terms.len() && j < b.terms.len() {
            let (va, ca) = &a.terms[i];
            let (vb, cb) = &b.terms[j];
            match va.as_str().cmp(vb.as_str()) {
                std::cmp::Ordering::Less => {
                    variables.push(VarDeltaJson {
                        variable: va,
                        a: Some(*ca),
                        b: None,
                    });
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    variables.push(VarDeltaJson {
                        variable: vb,
                        a: None,
                        b: Some(*cb),
                    });
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    if ca != cb {
                        variables.push(VarDeltaJson {
                            variable: va,
                            a: Some(*ca),
                            b: Some(*cb),
                        });
                    }
                    i += 1;
                    j += 1;
                }
            }
        }
        for (va, ca) in &a.terms[i..] {
            variables.push(VarDeltaJson {
                variable: va,
                a: Some(*ca),
                b: None,
            });
        }
        for (vb, cb) in &b.terms[j..] {
            variables.push(VarDeltaJson {
                variable: vb,
                a: None,
                b: Some(*cb),
            });
        }
        let rhs = (a.rhs != b.rhs).then_some(RhsDeltaJson { a: a.rhs, b: b.rhs });
        TermDiffJson { variables, rhs }
    }
}

// Note: auxiliary-variable folding is *not* consulted for the JSON
// term-level diff. The plain reporter collapses aux terms into a folded
// multiset row for readability; the JSON view instead reports every
// differing variable by name (aux included), leaving the consumer to
// interpret them against `comparison.projected_variables`.

// ---- label permutation -----------------------------------------------

/// Cross-matching of label-paired differing constraints into a label
/// permutation. `A@a_label` ≡ `B@b_label` for each correspondence; when
/// `all_differing_explained` is `true` the two files hold the same set
/// of constraints with only the labels permuted. This is informational
/// and never affects `equivalent` — a permuted label is still a genuine
/// disagreement of label assignment.
#[derive(Serialize)]
struct LabelPermutationJson<'a> {
    /// True iff every label-paired differing constraint was cross-matched
    /// (`unexplained` is empty).
    all_differing_explained: bool,
    /// Count of pure pairwise swaps (length-2 cycles).
    swaps: usize,
    /// `A@a_label` ≡ `B@b_label`, one per explained differing label, in
    /// A order.
    correspondences: Vec<LabelCorrespondenceJson<'a>>,
    /// The permutation as disjoint cycles of labels: `[l0, l1, …]` means
    /// `A@li` ≡ `B@l(i+1 mod n)`.
    cycles: Vec<Vec<&'a str>>,
    /// Differing labels that found no cross-match; empty iff
    /// `all_differing_explained`.
    unexplained: Vec<&'a str>,
}

#[derive(Serialize)]
struct LabelCorrespondenceJson<'a> {
    a_label: &'a str,
    b_label: &'a str,
    /// True iff the reverse also holds (`A@b_label` ≡ `B@a_label`).
    swap: bool,
}

impl<'a> LabelPermutationJson<'a> {
    fn from_perm(p: &'a LabelPermutation) -> Self {
        LabelPermutationJson {
            all_differing_explained: p.all_explained(),
            swaps: p.swaps(),
            correspondences: p
                .correspondences
                .iter()
                .map(|c| LabelCorrespondenceJson {
                    a_label: &c.from,
                    b_label: &c.to,
                    swap: c.swap,
                })
                .collect(),
            cycles: p
                .cycles
                .iter()
                .map(|c| c.iter().map(String::as_str).collect())
                .collect(),
            unexplained: p.unexplained.iter().map(String::as_str).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    use crate::compare::{CompareMode, CompareOptions, ReferenceSide, compare, compare_ordered};
    use crate::model::normalise_file;
    use crate::parser::parse;
    use serde_json::Value;

    fn diff(a: &str, b: &str) -> DiffResult {
        compare_ordered(
            &normalise_file(&parse(a).unwrap()).unwrap(),
            &normalise_file(&parse(b).unwrap()).unwrap(),
        )
    }

    fn diff_with(a: &str, b: &str, options: CompareOptions) -> DiffResult {
        compare(
            &normalise_file(&parse(a).unwrap()).unwrap(),
            &normalise_file(&parse(b).unwrap()).unwrap(),
            options,
        )
    }

    fn render(d: &DiffResult) -> Value {
        let mut buf: Vec<u8> = Vec::new();
        write(&mut buf, d).unwrap();
        // Trailing newline plus valid JSON.
        let text = String::from_utf8(buf).unwrap();
        assert!(text.ends_with('\n'), "output should end with a newline");
        serde_json::from_str(&text).expect("emitted valid JSON")
    }

    #[test]
    fn equivalent_pair_is_marked_equivalent_with_empty_constraints() {
        let v = render(&diff("1 x1 >= 1 ;\n", "+1 ~x1 <= 0 ;\n"));
        assert_eq!(v["equivalent"], Value::Bool(true));
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["comparison"]["mode"], "ordered");
        assert_eq!(v["summary"]["matches"], 1);
        // Matches are omitted: an equivalent pair has no constraint rows.
        assert_eq!(v["constraints"].as_array().unwrap().len(), 0);
        assert_eq!(v["objective"]["kind"], "both_absent");
        assert_eq!(v["preserved"]["kind"], "both_absent");
    }

    #[test]
    fn differing_constraint_carries_sides_and_term_delta() {
        let v = render(&diff("1 x1 1 x2 1 x3 >= 1 ;\n", "1 x1 2 x2 1 x3 >= 1 ;\n"));
        assert_eq!(v["equivalent"], Value::Bool(false));
        let c = &v["constraints"][0];
        assert_eq!(c["kind"], "differ");
        assert_eq!(c["index_a"], 0);
        assert_eq!(c["index_b"], 0);
        assert_eq!(c["a"]["raw"], "1 x1 1 x2 1 x3 >= 1 ;");
        assert_eq!(c["b"]["form"]["rhs"], 1);
        // Only the differing variable is listed in term_diff.
        let vars = c["term_diff"]["variables"].as_array().unwrap();
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0]["variable"], "x2");
        assert_eq!(vars[0]["a"], 1);
        assert_eq!(vars[0]["b"], 2);
        // RHS agrees, so it is null rather than a delta object.
        assert_eq!(c["term_diff"]["rhs"], Value::Null);
        // The full forms are still present for reconstruction.
        assert_eq!(c["a"]["form"]["terms"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn term_delta_reports_one_sided_variable_and_rhs() {
        let v = render(&diff("1 x1 1 x3 >= 1 ;\n", "1 x1 1 x4 >= 2 ;\n"));
        let td = &v["constraints"][0]["term_diff"];
        let vars = td["variables"].as_array().unwrap();
        // x3 only in A, x4 only in B.
        let x3 = vars.iter().find(|d| d["variable"] == "x3").unwrap();
        assert_eq!(x3["a"], 1);
        assert_eq!(x3["b"], Value::Null);
        let x4 = vars.iter().find(|d| d["variable"] == "x4").unwrap();
        assert_eq!(x4["a"], Value::Null);
        assert_eq!(x4["b"], 1);
        // RHS differs, so a delta object appears.
        assert_eq!(td["rhs"]["a"], 1);
        assert_eq!(td["rhs"]["b"], 2);
    }

    #[test]
    fn only_in_a_and_b_use_index_and_single_side() {
        let v = render(&diff("1 x1 >= 1 ;\n1 x2 >= 1 ;\n", "1 x1 >= 1 ;\n"));
        let only = v["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == "only_in_a")
            .unwrap();
        assert_eq!(only["index"], 1);
        assert_eq!(only["a"]["form"]["terms"][0]["variable"], "x2");
        assert!(only.get("b").is_none());
    }

    #[test]
    fn unordered_mode_and_projection_are_reported_in_comparison() {
        let projection: HashSet<String> = ["x".to_string()].into_iter().collect();
        let v = render(&diff_with(
            "preserved: x ;\n1 x 8 f >= 1 ;\n",
            "preserved: x ;\n1 x 7 g >= 2 ;\n",
            CompareOptions {
                mode: CompareMode::Unordered,
                match_labels: false,
                label_check: None,
                aux_projection: Some(projection),
                ignore_missing_preserved: None,
            },
        ));
        assert_eq!(v["comparison"]["mode"], "unordered");
        assert_eq!(v["comparison"]["aux_names_ignored"], Value::Bool(true));
        assert_eq!(v["comparison"]["projected_variables"][0], "x");
    }

    #[test]
    fn label_mismatch_records_reference_and_expected_actual() {
        let v = render(&diff_with(
            "1 x1 >= 1 ;\n",
            "@card 1 x1 >= 1 ;\n",
            CompareOptions {
                mode: CompareMode::Ordered,
                match_labels: false,
                label_check: Some(ReferenceSide::B),
                aux_projection: None,
                ignore_missing_preserved: None,
            },
        ));
        assert_eq!(v["equivalent"], Value::Bool(false));
        let c = &v["constraints"][0];
        assert_eq!(c["kind"], "label_mismatch");
        assert_eq!(c["reference"], "b");
        assert_eq!(c["expected_label"], "card");
        assert_eq!(c["actual_label"], Value::Null);
        assert_eq!(v["summary"]["label_mismatches"], 1);
    }

    #[test]
    fn objective_and_preserved_differences_are_tagged() {
        let v = render(&diff(
            "min: 1 x1 2 x2 ;\npreserved: x1 ;\n1 x1 >= 1 ;\n",
            "min: 1 x1 ;\npreserved: x2 ;\n1 x1 >= 1 ;\n",
        ));
        assert_eq!(v["objective"]["kind"], "differ");
        assert_eq!(v["objective"]["a"]["terms"].as_array().unwrap().len(), 2);
        assert_eq!(v["preserved"]["kind"], "differ");
        assert_eq!(v["preserved"]["a"]["literals"][0]["variable"], "x1");
        assert_eq!(v["summary"]["objective_difference"], Value::Bool(true));
        assert_eq!(v["summary"]["preserved_difference"], Value::Bool(true));
    }

    #[test]
    fn label_permutation_swap_is_reported_without_changing_the_verdict() {
        let v = render(&diff_with(
            "@le 1 x1 >= 1 ;\n@ge 1 x2 >= 1 ;\n",
            "@le 1 x2 >= 1 ;\n@ge 1 x1 >= 1 ;\n",
            CompareOptions {
                mode: CompareMode::Ordered,
                match_labels: true,
                label_check: None,
                aux_projection: None,
                ignore_missing_preserved: None,
            },
        ));
        // A permuted label is still a genuine disagreement.
        assert_eq!(v["equivalent"], Value::Bool(false));
        let p = &v["label_permutation"];
        assert_eq!(p["all_differing_explained"], Value::Bool(true));
        assert_eq!(p["swaps"], 1);
        assert_eq!(p["cycles"].as_array().unwrap().len(), 1);
        assert_eq!(p["unexplained"].as_array().unwrap().len(), 0);
        let corr = p["correspondences"].as_array().unwrap();
        let le = corr.iter().find(|c| c["a_label"] == "le").unwrap();
        assert_eq!(le["b_label"], "ge");
        assert_eq!(le["swap"], Value::Bool(true));
    }

    #[test]
    fn label_permutation_is_null_without_match_labels() {
        let v = render(&diff("1 x1 >= 1 ;\n", "1 x2 >= 1 ;\n"));
        assert_eq!(v["label_permutation"], Value::Null);
    }

    #[test]
    fn ignored_missing_preserved_is_recorded_honestly() {
        // A lacks preserved:, B has it, constraints agree, ignore set to
        // A. The headline is equivalent and the summary flag is cleared,
        // but `preserved` still reports the true only_in_b finding and
        // `comparison.ignored_missing_preserved` explains the relaxation.
        let v = render(&diff_with(
            "1 x1 >= 1 ;\n",
            "preserved: x1 ;\n1 x1 >= 1 ;\n",
            CompareOptions {
                ignore_missing_preserved: Some(ReferenceSide::A),
                ..Default::default()
            },
        ));
        assert_eq!(v["equivalent"], Value::Bool(true));
        assert_eq!(v["comparison"]["ignored_missing_preserved"], "a");
        assert_eq!(v["summary"]["preserved_difference"], Value::Bool(false));
        // The raw structural finding is preserved, not masked.
        assert_eq!(v["preserved"]["kind"], "only_in_b");
        assert_eq!(v["preserved"]["b"]["literals"][0]["variable"], "x1");
    }
}
