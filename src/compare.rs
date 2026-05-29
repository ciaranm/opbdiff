//! Comparison engine. Supports two matching modes (ordered by
//! position, unordered by canonical-form multiset) and optional
//! directional label checking.
//!
//! See `dev_docs/0004-comparison-algorithm.md` for the algorithm.

use std::collections::HashMap;

use crate::model::{
    CanonicalConstraint, CanonicalFile, CanonicalLabelledConstraint, CanonicalObjectiveItem,
    CanonicalPreservedItem,
};

/// How to pair up constraints between the two files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CompareMode {
    /// Pair by position: `A[i]` is compared with `B[i]`.
    #[default]
    Ordered,
    /// Pair by canonical form: greedy multiset matching, position
    /// within each file is irrelevant.
    Unordered,
}

/// Which side carries the "must-be-honoured" labels for
/// `--check-labels` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReferenceSide {
    /// File A is the reference; labels in A must be honoured in B.
    A,
    /// File B is the reference; labels in B must be honoured in A.
    /// This is the default for the verbal "candidate vs reference"
    /// description discussed in `dev_docs/0004`.
    #[default]
    B,
}

/// Comparison options. Public so callers can construct without going
/// through clap.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompareOptions {
    pub mode: CompareMode,
    /// Some(reference) enables directional label checking.
    pub label_check: Option<ReferenceSide>,
}

/// The full structured diff of two canonical files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    pub mode: CompareMode,
    pub objective: ObjectiveDiff,
    pub preserved: PreservedDiff,
    pub constraints: Vec<ConstraintDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectiveDiff {
    BothAbsent,
    Match,
    Differ {
        a: CanonicalObjectiveItem,
        b: CanonicalObjectiveItem,
    },
    OnlyInA(CanonicalObjectiveItem),
    OnlyInB(CanonicalObjectiveItem),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreservedDiff {
    BothAbsent,
    Match,
    Differ {
        a: CanonicalPreservedItem,
        b: CanonicalPreservedItem,
    },
    OnlyInA(CanonicalPreservedItem),
    OnlyInB(CanonicalPreservedItem),
}

/// Per-constraint outcome. Carries both sides' source records so the
/// reporter can show originals. `index_a` and `index_b` are the
/// 0-based positions in each file's constraint list; under ordered
/// mode they are equal for `Match`, `Differ`, and `LabelMismatch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintDiff {
    Match {
        index_a: usize,
        index_b: usize,
        a: CanonicalLabelledConstraint,
        b: CanonicalLabelledConstraint,
    },
    /// Ordered mode only: the same position holds non-equivalent
    /// constraints on each side. Never produced by unordered mode.
    Differ {
        index_a: usize,
        index_b: usize,
        a: CanonicalLabelledConstraint,
        b: CanonicalLabelledConstraint,
    },
    /// A's constraint at this position has no partner in B.
    OnlyInA {
        index: usize,
        a: CanonicalLabelledConstraint,
    },
    /// B's constraint at this position has no partner in A.
    OnlyInB {
        index: usize,
        b: CanonicalLabelledConstraint,
    },
    /// Canonical forms matched, but the reference-side label was
    /// not honoured by the candidate side.
    LabelMismatch {
        index_a: usize,
        index_b: usize,
        a: CanonicalLabelledConstraint,
        b: CanonicalLabelledConstraint,
        reference: ReferenceSide,
    },
}

/// Flat tally of the diff for human and machine summaries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub matches: usize,
    pub differing: usize,
    pub only_in_a: usize,
    pub only_in_b: usize,
    pub label_mismatches: usize,
    pub objective_difference: bool,
    pub preserved_difference: bool,
}

impl DiffResult {
    /// True iff every part of the diff is a `Match` (or both-absent
    /// for objective/preserved). Anything else — including label
    /// mismatches — counts as different.
    pub fn is_equivalent(&self) -> bool {
        matches!(
            self.objective,
            ObjectiveDiff::BothAbsent | ObjectiveDiff::Match
        ) && matches!(
            self.preserved,
            PreservedDiff::BothAbsent | PreservedDiff::Match
        ) && self
            .constraints
            .iter()
            .all(|d| matches!(d, ConstraintDiff::Match { .. }))
    }

    pub fn summary(&self) -> Summary {
        let mut s = Summary::default();
        for d in &self.constraints {
            match d {
                ConstraintDiff::Match { .. } => s.matches += 1,
                ConstraintDiff::Differ { .. } => s.differing += 1,
                ConstraintDiff::OnlyInA { .. } => s.only_in_a += 1,
                ConstraintDiff::OnlyInB { .. } => s.only_in_b += 1,
                ConstraintDiff::LabelMismatch { .. } => s.label_mismatches += 1,
            }
        }
        s.objective_difference = !matches!(
            self.objective,
            ObjectiveDiff::BothAbsent | ObjectiveDiff::Match
        );
        s.preserved_difference = !matches!(
            self.preserved,
            PreservedDiff::BothAbsent | PreservedDiff::Match
        );
        s
    }
}

/// Backward-compatible thin wrapper: ordered mode, no label check.
pub fn compare_ordered(a: &CanonicalFile, b: &CanonicalFile) -> DiffResult {
    compare(
        a,
        b,
        CompareOptions {
            mode: CompareMode::Ordered,
            label_check: None,
        },
    )
}

/// Compare two canonical files. Always produces a `DiffResult`; the
/// callsite is responsible for interpreting the verdict.
pub fn compare(a: &CanonicalFile, b: &CanonicalFile, options: CompareOptions) -> DiffResult {
    let constraints = match options.mode {
        CompareMode::Ordered => ordered_constraints(&a.constraints, &b.constraints),
        CompareMode::Unordered => unordered_constraints(&a.constraints, &b.constraints),
    };

    let constraints = if let Some(reference) = options.label_check {
        apply_label_check(constraints, reference)
    } else {
        constraints
    };

    DiffResult {
        mode: options.mode,
        objective: compare_objectives(&a.objective, &b.objective),
        preserved: compare_preserved(&a.preserved, &b.preserved),
        constraints,
    }
}

fn ordered_constraints(
    a: &[CanonicalLabelledConstraint],
    b: &[CanonicalLabelledConstraint],
) -> Vec<ConstraintDiff> {
    let max = a.len().max(b.len());
    let mut out = Vec::with_capacity(max);
    for i in 0..max {
        let entry = match (a.get(i), b.get(i)) {
            (Some(ac), Some(bc)) if ac.form == bc.form => ConstraintDiff::Match {
                index_a: i,
                index_b: i,
                a: ac.clone(),
                b: bc.clone(),
            },
            (Some(ac), Some(bc)) => ConstraintDiff::Differ {
                index_a: i,
                index_b: i,
                a: ac.clone(),
                b: bc.clone(),
            },
            (Some(ac), None) => ConstraintDiff::OnlyInA {
                index: i,
                a: ac.clone(),
            },
            (None, Some(bc)) => ConstraintDiff::OnlyInB {
                index: i,
                b: bc.clone(),
            },
            (None, None) => unreachable!("max bounded by both lengths"),
        };
        out.push(entry);
    }
    out
}

/// Unordered mode: greedy multiset match.
///
/// Build a map from canonical form to a FIFO of B's available indices.
/// Walk A; for each constraint, pop a partner from B's queue or emit
/// OnlyInA. Anything left in B's queues at the end is OnlyInB.
///
/// Greedy is good enough for v1; it may produce a sub-optimal pairing
/// under future label-checking when multiple equivalent constraints
/// have different labels, but that case is exotic.
fn unordered_constraints(
    a: &[CanonicalLabelledConstraint],
    b: &[CanonicalLabelledConstraint],
) -> Vec<ConstraintDiff> {
    let mut b_available: HashMap<&CanonicalConstraint, Vec<usize>> = HashMap::new();
    for (i, bc) in b.iter().enumerate() {
        b_available.entry(&bc.form).or_default().push(i);
    }
    // FIFO: pop from front. Vec::remove(0) is O(n) but n is small per
    // canonical form. For very pathological inputs we could switch to
    // VecDeque; not worth the noise for v1.

    let mut out = Vec::with_capacity(a.len() + b.len());

    for (ai, ac) in a.iter().enumerate() {
        let bi_opt = b_available.get_mut(&ac.form).and_then(|q| {
            if q.is_empty() {
                None
            } else {
                Some(q.remove(0))
            }
        });
        match bi_opt {
            Some(bi) => out.push(ConstraintDiff::Match {
                index_a: ai,
                index_b: bi,
                a: ac.clone(),
                b: b[bi].clone(),
            }),
            None => out.push(ConstraintDiff::OnlyInA {
                index: ai,
                a: ac.clone(),
            }),
        }
    }

    // Anything still queued in B is unmatched. Walk B in original
    // order, emitting OnlyInB for indices that remain.
    let mut remaining: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for q in b_available.values() {
        for i in q {
            remaining.insert(*i);
        }
    }
    for (bi, bc) in b.iter().enumerate() {
        if remaining.contains(&bi) {
            out.push(ConstraintDiff::OnlyInB {
                index: bi,
                b: bc.clone(),
            });
        }
    }

    out
}

fn apply_label_check(
    constraints: Vec<ConstraintDiff>,
    reference: ReferenceSide,
) -> Vec<ConstraintDiff> {
    constraints
        .into_iter()
        .map(|d| match d {
            ConstraintDiff::Match {
                index_a,
                index_b,
                a,
                b,
            } => {
                if label_violates(&a, &b, reference) {
                    ConstraintDiff::LabelMismatch {
                        index_a,
                        index_b,
                        a,
                        b,
                        reference,
                    }
                } else {
                    ConstraintDiff::Match {
                        index_a,
                        index_b,
                        a,
                        b,
                    }
                }
            }
            other => other,
        })
        .collect()
}

/// Returns true if the candidate side fails to honour the reference
/// side's label. The rule:
///
/// - if the reference side has no label, no violation;
/// - if the reference side has a label, the candidate side must have
///   the *same* label (extras don't matter here because each
///   constraint has at most one label).
fn label_violates(
    a: &CanonicalLabelledConstraint,
    b: &CanonicalLabelledConstraint,
    reference: ReferenceSide,
) -> bool {
    let (ref_label, cand_label) = match reference {
        ReferenceSide::A => (a.label.as_deref(), b.label.as_deref()),
        ReferenceSide::B => (b.label.as_deref(), a.label.as_deref()),
    };
    match ref_label {
        None => false,
        Some(r) => cand_label != Some(r),
    }
}

fn compare_objectives(
    a: &Option<CanonicalObjectiveItem>,
    b: &Option<CanonicalObjectiveItem>,
) -> ObjectiveDiff {
    match (a, b) {
        (None, None) => ObjectiveDiff::BothAbsent,
        (Some(av), Some(bv)) if av.form == bv.form => ObjectiveDiff::Match,
        (Some(av), Some(bv)) => ObjectiveDiff::Differ {
            a: av.clone(),
            b: bv.clone(),
        },
        (Some(av), None) => ObjectiveDiff::OnlyInA(av.clone()),
        (None, Some(bv)) => ObjectiveDiff::OnlyInB(bv.clone()),
    }
}

fn compare_preserved(
    a: &Option<CanonicalPreservedItem>,
    b: &Option<CanonicalPreservedItem>,
) -> PreservedDiff {
    match (a, b) {
        (None, None) => PreservedDiff::BothAbsent,
        (Some(av), Some(bv)) if av.form == bv.form => PreservedDiff::Match,
        (Some(av), Some(bv)) => PreservedDiff::Differ {
            a: av.clone(),
            b: bv.clone(),
        },
        (Some(av), None) => PreservedDiff::OnlyInA(av.clone()),
        (None, Some(bv)) => PreservedDiff::OnlyInB(bv.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::normalise_file;
    use crate::parser::parse;

    fn canonical(input: &str) -> CanonicalFile {
        normalise_file(&parse(input).unwrap()).unwrap()
    }

    // ----- ordered mode -------------------------------------------------

    #[test]
    fn equivalent_files_compare_equal_ordered() {
        let a = canonical("1 x1 1 x2 >= 1 ;\n");
        let b = canonical("+1 ~x2 +1 ~x1 <= 1 ;\n");
        let diff = compare_ordered(&a, &b);
        assert!(diff.is_equivalent());
    }

    #[test]
    fn differing_at_index_in_ordered_mode() {
        let a = canonical("1 x1 >= 1 ;\n1 x2 >= 1 ;\n");
        let b = canonical("1 x1 >= 1 ;\n1 x3 >= 1 ;\n");
        let diff = compare_ordered(&a, &b);
        let s = diff.summary();
        assert_eq!(s.matches, 1);
        assert_eq!(s.differing, 1);
    }

    #[test]
    fn extras_in_ordered_mode() {
        let a = canonical("1 x1 >= 1 ;\n1 x2 >= 1 ;\n");
        let b = canonical("1 x1 >= 1 ;\n");
        let diff = compare_ordered(&a, &b);
        let s = diff.summary();
        assert_eq!(s.only_in_a, 1);
        assert_eq!(s.only_in_b, 0);
    }

    // ----- unordered mode ----------------------------------------------

    fn compare_unordered(a: &CanonicalFile, b: &CanonicalFile) -> DiffResult {
        compare(
            a,
            b,
            CompareOptions {
                mode: CompareMode::Unordered,
                label_check: None,
            },
        )
    }

    #[test]
    fn reordered_constraints_compare_equal_unordered() {
        let a = canonical("1 x1 >= 1 ;\n1 x2 >= 1 ;\n1 x3 >= 1 ;\n");
        let b = canonical("1 x3 >= 1 ;\n1 x1 >= 1 ;\n1 x2 >= 1 ;\n");
        // Ordered should NOT be equivalent.
        assert!(!compare_ordered(&a, &b).is_equivalent());
        // Unordered SHOULD be equivalent.
        let diff = compare_unordered(&a, &b);
        assert!(diff.is_equivalent(), "summary: {:?}", diff.summary());
        assert_eq!(diff.summary().matches, 3);
    }

    #[test]
    fn unordered_matches_duplicates_by_multiplicity() {
        // A has x1 twice, B has x1 once. One match, one OnlyInA.
        let a = canonical("1 x1 >= 1 ;\n1 x1 >= 1 ;\n");
        let b = canonical("1 x1 >= 1 ;\n");
        let diff = compare_unordered(&a, &b);
        let s = diff.summary();
        assert_eq!(s.matches, 1);
        assert_eq!(s.only_in_a, 1);
        assert_eq!(s.only_in_b, 0);
    }

    #[test]
    fn unordered_never_emits_differ() {
        // In unordered mode, a constraint either matches or it doesn't;
        // there is no notion of "differing at a position".
        let a = canonical("1 x1 >= 1 ;\n1 x2 >= 1 ;\n");
        let b = canonical("1 x3 >= 1 ;\n1 x4 >= 1 ;\n");
        let diff = compare_unordered(&a, &b);
        for d in &diff.constraints {
            assert!(!matches!(d, ConstraintDiff::Differ { .. }));
        }
        let s = diff.summary();
        assert_eq!(s.matches, 0);
        assert_eq!(s.only_in_a, 2);
        assert_eq!(s.only_in_b, 2);
    }

    // ----- label checking ---------------------------------------------

    fn with_label_check(
        a: &CanonicalFile,
        b: &CanonicalFile,
        mode: CompareMode,
        reference: ReferenceSide,
    ) -> DiffResult {
        compare(
            a,
            b,
            CompareOptions {
                mode,
                label_check: Some(reference),
            },
        )
    }

    #[test]
    fn label_check_off_ignores_label_differences() {
        let a = canonical("@one 1 x1 >= 1 ;\n");
        let b = canonical("@two 1 x1 >= 1 ;\n");
        let diff = compare_ordered(&a, &b);
        assert!(diff.is_equivalent());
    }

    #[test]
    fn label_check_reference_b_flags_missing_label_in_a() {
        let a = canonical("1 x1 >= 1 ;\n");
        let b = canonical("@card 1 x1 >= 1 ;\n");
        let diff = with_label_check(&a, &b, CompareMode::Ordered, ReferenceSide::B);
        assert!(!diff.is_equivalent());
        let s = diff.summary();
        assert_eq!(s.label_mismatches, 1);
    }

    #[test]
    fn label_check_reference_b_tolerates_extra_labels_in_a() {
        // B is the reference and has no label → extra label in A is fine.
        let a = canonical("@extra 1 x1 >= 1 ;\n");
        let b = canonical("1 x1 >= 1 ;\n");
        let diff = with_label_check(&a, &b, CompareMode::Ordered, ReferenceSide::B);
        assert!(diff.is_equivalent());
    }

    #[test]
    fn label_check_reference_a_inverts_polarity() {
        // Same constraint pair as the previous test, but now A is the
        // reference, so the extra label in A is required in B and B
        // doesn't have it.
        let a = canonical("@extra 1 x1 >= 1 ;\n");
        let b = canonical("1 x1 >= 1 ;\n");
        let diff = with_label_check(&a, &b, CompareMode::Ordered, ReferenceSide::A);
        assert!(!diff.is_equivalent());
        assert_eq!(diff.summary().label_mismatches, 1);
    }

    #[test]
    fn label_check_wrong_label_is_mismatch() {
        let a = canonical("@bar 1 x1 >= 1 ;\n");
        let b = canonical("@foo 1 x1 >= 1 ;\n");
        let diff = with_label_check(&a, &b, CompareMode::Ordered, ReferenceSide::B);
        assert!(!diff.is_equivalent());
        assert_eq!(diff.summary().label_mismatches, 1);
    }

    #[test]
    fn label_check_combines_with_unordered_mode() {
        // Same canonical forms, different order, same labels. Should
        // be equivalent under unordered + label check.
        let a = canonical("@one 1 x1 >= 1 ;\n@two 1 x2 >= 1 ;\n");
        let b = canonical("@two 1 x2 >= 1 ;\n@one 1 x1 >= 1 ;\n");
        let diff = with_label_check(&a, &b, CompareMode::Unordered, ReferenceSide::B);
        assert!(diff.is_equivalent(), "summary: {:?}", diff.summary());
    }
}
