//! Comparison engine. Supports two matching modes (ordered by
//! position, unordered by canonical-form multiset), an optional
//! label-matching pre-pass (`match_labels`) that pairs constraints by
//! shared label before falling back to the chosen mode, and optional
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
    /// When true, constraints that share a label (present on both
    /// sides) are paired up first, regardless of position, and the
    /// content of each such pair is diffed. Constraints with no
    /// matching label fall back to `mode`. See
    /// `dev_docs/0004-comparison-algorithm.md`.
    pub match_labels: bool,
    /// Some(reference) enables directional label checking.
    pub label_check: Option<ReferenceSide>,
}

/// The full structured diff of two canonical files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    pub mode: CompareMode,
    /// Whether constraints were paired by shared label before the
    /// fallback `mode` matching ran. Reported for context only.
    pub matched_by_label: bool,
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
            match_labels: false,
            label_check: None,
        },
    )
}

/// Compare two canonical files. Always produces a `DiffResult`; the
/// callsite is responsible for interpreting the verdict.
pub fn compare(a: &CanonicalFile, b: &CanonicalFile, options: CompareOptions) -> DiffResult {
    let constraints = if options.match_labels {
        label_matched_constraints(&a.constraints, &b.constraints, options.mode)
    } else {
        match options.mode {
            CompareMode::Ordered => ordered_constraints(&a.constraints, &b.constraints),
            CompareMode::Unordered => unordered_constraints(&a.constraints, &b.constraints),
        }
    };

    let constraints = if let Some(reference) = options.label_check {
        apply_label_check(constraints, reference)
    } else {
        constraints
    };

    DiffResult {
        mode: options.mode,
        matched_by_label: options.match_labels,
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

/// Label-matched mode: pair constraints that share a label, then fall
/// back to `mode` for the rest.
///
/// Pass 1 walks A in order; for each A constraint that carries a label
/// also present (and still unclaimed) in B, the two are paired — a
/// `Match` if their canonical forms agree, a `Differ` otherwise.
/// Duplicate labels are paired first-come-first-served (FIFO over B's
/// occurrences), though VeriPB labels are normally unique.
///
/// Pass 2 takes everything left unpaired — A constraints with no label,
/// or whose label is absent from B, and the corresponding remainder of
/// B — and runs the ordinary `mode` matching over those two
/// subsequences, then maps the sub-indices back to original positions.
///
/// Label-matched pairs are emitted first (in A order), followed by the
/// fallback diffs.
fn label_matched_constraints(
    a: &[CanonicalLabelledConstraint],
    b: &[CanonicalLabelledConstraint],
    mode: CompareMode,
) -> Vec<ConstraintDiff> {
    let mut b_by_label: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, bc) in b.iter().enumerate() {
        if let Some(label) = bc.label.as_deref() {
            b_by_label.entry(label).or_default().push(i);
        }
    }

    let mut out = Vec::new();
    let mut b_claimed = vec![false; b.len()];
    let mut leftover_a: Vec<usize> = Vec::new();

    for (ai, ac) in a.iter().enumerate() {
        let bi = ac.label.as_deref().and_then(|label| {
            b_by_label
                .get_mut(label)
                .filter(|q| !q.is_empty())
                .map(|q| q.remove(0))
        });
        match bi {
            Some(bi) => {
                b_claimed[bi] = true;
                let bc = &b[bi];
                out.push(if ac.form == bc.form {
                    ConstraintDiff::Match {
                        index_a: ai,
                        index_b: bi,
                        a: ac.clone(),
                        b: bc.clone(),
                    }
                } else {
                    ConstraintDiff::Differ {
                        index_a: ai,
                        index_b: bi,
                        a: ac.clone(),
                        b: bc.clone(),
                    }
                });
            }
            None => leftover_a.push(ai),
        }
    }

    let leftover_b: Vec<usize> = (0..b.len()).filter(|&i| !b_claimed[i]).collect();

    // Diff the unpaired remainders with the ordinary mode, working on
    // owned subsequences, then translate sub-indices back.
    let a_sub: Vec<CanonicalLabelledConstraint> =
        leftover_a.iter().map(|&i| a[i].clone()).collect();
    let b_sub: Vec<CanonicalLabelledConstraint> =
        leftover_b.iter().map(|&i| b[i].clone()).collect();
    let sub = match mode {
        CompareMode::Ordered => ordered_constraints(&a_sub, &b_sub),
        CompareMode::Unordered => unordered_constraints(&a_sub, &b_sub),
    };

    out.extend(
        sub.into_iter()
            .map(|d| remap_indices(d, &leftover_a, &leftover_b)),
    );
    out
}

/// Translate the A/B indices in a fallback-pass `ConstraintDiff` from
/// positions in the leftover subsequences back to positions in the
/// original files.
fn remap_indices(d: ConstraintDiff, a_idx: &[usize], b_idx: &[usize]) -> ConstraintDiff {
    match d {
        ConstraintDiff::Match {
            index_a,
            index_b,
            a,
            b,
        } => ConstraintDiff::Match {
            index_a: a_idx[index_a],
            index_b: b_idx[index_b],
            a,
            b,
        },
        ConstraintDiff::Differ {
            index_a,
            index_b,
            a,
            b,
        } => ConstraintDiff::Differ {
            index_a: a_idx[index_a],
            index_b: b_idx[index_b],
            a,
            b,
        },
        ConstraintDiff::OnlyInA { index, a } => ConstraintDiff::OnlyInA {
            index: a_idx[index],
            a,
        },
        ConstraintDiff::OnlyInB { index, b } => ConstraintDiff::OnlyInB {
            index: b_idx[index],
            b,
        },
        // The fallback passes never emit LabelMismatch; label checking
        // is applied later, in `compare`.
        ConstraintDiff::LabelMismatch { .. } => d,
    }
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
                match_labels: false,
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
                match_labels: false,
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

    // ----- label-matched mode -----------------------------------------

    fn match_labels(a: &CanonicalFile, b: &CanonicalFile, mode: CompareMode) -> DiffResult {
        compare(
            a,
            b,
            CompareOptions {
                mode,
                match_labels: true,
                label_check: None,
            },
        )
    }

    #[test]
    fn match_labels_pairs_same_label_across_positions() {
        // Same labels, opposite order, content differs per label. Each
        // label pairs up and reports a content difference; nothing is
        // OnlyInA / OnlyInB.
        let a = canonical("@p 1 x1 >= 1 ;\n@q 1 x2 >= 1 ;\n");
        let b = canonical("@q 1 x9 >= 1 ;\n@p 1 x8 >= 1 ;\n");
        let diff = match_labels(&a, &b, CompareMode::Ordered);
        let s = diff.summary();
        assert_eq!(s.differing, 2, "summary: {s:?}");
        assert_eq!(s.only_in_a, 0);
        assert_eq!(s.only_in_b, 0);
        // The @p pair should reference x1 (A) and x8 (B), proving the
        // pairing was by label rather than by position.
        let p = diff
            .constraints
            .iter()
            .find_map(|d| match d {
                ConstraintDiff::Differ { a, b, .. } if a.label.as_deref() == Some("p") => {
                    Some((a.clone(), b.clone()))
                }
                _ => None,
            })
            .expect("a Differ for label p");
        assert_eq!(p.0.form.terms[0].0, "x1");
        assert_eq!(p.1.form.terms[0].0, "x8");
    }

    #[test]
    fn match_labels_reports_match_when_content_agrees() {
        let a = canonical("@p 1 x1 >= 1 ;\n@q 1 x2 >= 1 ;\n");
        let b = canonical("@q 1 x2 >= 1 ;\n@p +1 ~x1 <= 0 ;\n");
        let diff = match_labels(&a, &b, CompareMode::Ordered);
        assert!(diff.is_equivalent(), "summary: {:?}", diff.summary());
        assert_eq!(diff.summary().matches, 2);
    }

    #[test]
    fn match_labels_falls_back_to_mode_for_unlabelled() {
        // @p pairs by label (content differs). The unlabelled bound
        // constraints have no label to match on, so they fall through
        // to the fallback mode: unordered matches them by canonical
        // form even though A and B list them in opposite order.
        let a = canonical("1 x1 >= 0 ;\n@p 1 x2 >= 1 ;\n1 x3 >= 0 ;\n");
        let b = canonical("1 x3 >= 0 ;\n@p 1 x9 >= 1 ;\n1 x1 >= 0 ;\n");
        let diff = match_labels(&a, &b, CompareMode::Unordered);
        let s = diff.summary();
        assert_eq!(s.matches, 2, "the two unlabelled bounds: {s:?}");
        assert_eq!(s.differing, 1, "the @p pair");
        assert_eq!(s.only_in_a, 0);
        assert_eq!(s.only_in_b, 0);
    }

    #[test]
    fn match_labels_unmatched_label_becomes_leftover() {
        // @only exists in A but not B; it falls back to mode matching
        // and (no canonical partner in B) becomes OnlyInA.
        let a = canonical("@only 1 x1 >= 1 ;\n");
        let b = canonical("@other 1 x2 >= 1 ;\n");
        let diff = match_labels(&a, &b, CompareMode::Unordered);
        let s = diff.summary();
        assert_eq!(s.differing, 0);
        assert_eq!(s.only_in_a, 1);
        assert_eq!(s.only_in_b, 1);
    }

    #[test]
    fn match_labels_remaps_indices_to_original_positions() {
        // The fallback pass works on subsequences; its indices must be
        // translated back. Here the only unlabelled constraint sits at
        // original A index 1 / B index 1.
        let a = canonical("@p 1 x1 >= 1 ;\n1 z >= 5 ;\n");
        let b = canonical("@p 1 x1 >= 1 ;\n1 w >= 5 ;\n");
        let diff = match_labels(&a, &b, CompareMode::Ordered);
        let leftover = diff
            .constraints
            .iter()
            .find(|d| matches!(d, ConstraintDiff::Differ { a, .. } if a.label.is_none()))
            .expect("a Differ for the unlabelled pair");
        if let ConstraintDiff::Differ {
            index_a, index_b, ..
        } = leftover
        {
            assert_eq!(*index_a, 1);
            assert_eq!(*index_b, 1);
        }
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
