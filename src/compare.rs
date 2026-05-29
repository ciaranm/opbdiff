//! Comparison engine. Takes two canonical models and produces a
//! structured diff result. Ordered mode only in v1; unordered and
//! label handling land in later commits.
//!
//! See `dev_docs/0004-comparison-algorithm.md` for the algorithm.

use crate::model::{
    CanonicalFile, CanonicalLabelledConstraint, CanonicalObjectiveItem, CanonicalPreservedItem,
};

/// The full structured diff of two canonical files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
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

/// Per-position outcome for one constraint pair in ordered mode.
///
/// `Match` and `Differ` both carry both source-side records so the
/// reporter can show the original text; `Match` is not opaque because
/// later modes (label checking) may reclassify a match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConstraintDiff {
    Match {
        index: usize,
        a: CanonicalLabelledConstraint,
        b: CanonicalLabelledConstraint,
    },
    Differ {
        index: usize,
        a: CanonicalLabelledConstraint,
        b: CanonicalLabelledConstraint,
    },
    ExtraInA {
        index: usize,
        a: CanonicalLabelledConstraint,
    },
    ExtraInB {
        index: usize,
        b: CanonicalLabelledConstraint,
    },
}

/// A flat tally of the diff result for human and machine summaries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub matches: usize,
    pub differing: usize,
    pub extra_in_a: usize,
    pub extra_in_b: usize,
    pub objective_difference: bool,
    pub preserved_difference: bool,
}

impl DiffResult {
    /// True iff every part of the diff is a `Match` (or both-absent for
    /// objective/preserved). This is what determines exit code 0 vs 1.
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
                ConstraintDiff::ExtraInA { .. } => s.extra_in_a += 1,
                ConstraintDiff::ExtraInB { .. } => s.extra_in_b += 1,
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

/// Compare two canonical files in ordered mode: constraints are
/// paired by position. Labels are ignored for the match decision in
/// this mode.
pub fn compare_ordered(a: &CanonicalFile, b: &CanonicalFile) -> DiffResult {
    let mut constraints = Vec::with_capacity(a.constraints.len().max(b.constraints.len()));
    let max = a.constraints.len().max(b.constraints.len());
    for i in 0..max {
        let entry = match (a.constraints.get(i), b.constraints.get(i)) {
            (Some(ac), Some(bc)) if ac.form == bc.form => ConstraintDiff::Match {
                index: i,
                a: ac.clone(),
                b: bc.clone(),
            },
            (Some(ac), Some(bc)) => ConstraintDiff::Differ {
                index: i,
                a: ac.clone(),
                b: bc.clone(),
            },
            (Some(ac), None) => ConstraintDiff::ExtraInA {
                index: i,
                a: ac.clone(),
            },
            (None, Some(bc)) => ConstraintDiff::ExtraInB {
                index: i,
                b: bc.clone(),
            },
            (None, None) => unreachable!("max bounded by both lengths"),
        };
        constraints.push(entry);
    }

    DiffResult {
        objective: compare_objectives(&a.objective, &b.objective),
        preserved: compare_preserved(&a.preserved, &b.preserved),
        constraints,
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

    #[test]
    fn equivalent_files_compare_equal() {
        let a = canonical("1 x1 1 x2 >= 1 ;\n");
        let b = canonical("+1 ~x2 +1 ~x1 <= 1 ;\n");
        let diff = compare_ordered(&a, &b);
        assert!(diff.is_equivalent());
        let s = diff.summary();
        assert_eq!(s.matches, 1);
        assert_eq!(s.differing, 0);
    }

    #[test]
    fn differing_constraint_at_index_is_flagged() {
        let a = canonical("1 x1 >= 1 ;\n1 x2 >= 1 ;\n");
        let b = canonical("1 x1 >= 1 ;\n1 x3 >= 1 ;\n");
        let diff = compare_ordered(&a, &b);
        assert!(!diff.is_equivalent());
        let s = diff.summary();
        assert_eq!(s.matches, 1);
        assert_eq!(s.differing, 1);
        match &diff.constraints[1] {
            ConstraintDiff::Differ { index, .. } => assert_eq!(*index, 1),
            other => panic!("expected Differ at index 1, got {other:?}"),
        }
    }

    #[test]
    fn extra_constraint_in_a_is_flagged() {
        let a = canonical("1 x1 >= 1 ;\n1 x2 >= 1 ;\n");
        let b = canonical("1 x1 >= 1 ;\n");
        let diff = compare_ordered(&a, &b);
        assert!(!diff.is_equivalent());
        let s = diff.summary();
        assert_eq!(s.matches, 1);
        assert_eq!(s.extra_in_a, 1);
        assert_eq!(s.extra_in_b, 0);
        assert!(matches!(
            diff.constraints[1],
            ConstraintDiff::ExtraInA { .. }
        ));
    }

    #[test]
    fn extra_constraint_in_b_is_flagged() {
        let a = canonical("1 x1 >= 1 ;\n");
        let b = canonical("1 x1 >= 1 ;\n1 x2 >= 1 ;\n");
        let diff = compare_ordered(&a, &b);
        assert!(!diff.is_equivalent());
        let s = diff.summary();
        assert_eq!(s.matches, 1);
        assert_eq!(s.extra_in_b, 1);
        assert!(matches!(
            diff.constraints[1],
            ConstraintDiff::ExtraInB { .. }
        ));
    }

    #[test]
    fn labels_do_not_affect_match_in_v1() {
        // Same canonical form, different labels — still a Match.
        let a = canonical("@card 1 x1 1 x2 >= 1 ;\n");
        let b = canonical("@something_else 1 x1 1 x2 >= 1 ;\n");
        let diff = compare_ordered(&a, &b);
        assert!(diff.is_equivalent());
        // But the labels are preserved in the diff record for the
        // future label-checking mode.
        match &diff.constraints[0] {
            ConstraintDiff::Match { a, b, .. } => {
                assert_eq!(a.label.as_deref(), Some("card"));
                assert_eq!(b.label.as_deref(), Some("something_else"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn objectives_compared_for_equivalence() {
        let a = canonical("min: 1 x1 1 x2 ;\n");
        let b = canonical("min: 1 x2 1 x1 ;\n");
        let diff = compare_ordered(&a, &b);
        assert!(matches!(diff.objective, ObjectiveDiff::Match));
        assert!(diff.is_equivalent());
    }

    #[test]
    fn objective_in_one_only_is_flagged() {
        let a = canonical("min: 1 x1 ;\n");
        let b = canonical("");
        let diff = compare_ordered(&a, &b);
        assert!(matches!(diff.objective, ObjectiveDiff::OnlyInA(_)));
        assert!(!diff.is_equivalent());
        assert!(diff.summary().objective_difference);
    }

    #[test]
    fn objective_disagreement_is_flagged() {
        let a = canonical("min: 1 x1 ;\n");
        let b = canonical("min: 1 x2 ;\n");
        let diff = compare_ordered(&a, &b);
        assert!(matches!(diff.objective, ObjectiveDiff::Differ { .. }));
        assert!(!diff.is_equivalent());
    }

    #[test]
    fn preserved_in_one_only_is_flagged() {
        let a = canonical("preserved: x1 x2 ;\n");
        let b = canonical("");
        let diff = compare_ordered(&a, &b);
        assert!(matches!(diff.preserved, PreservedDiff::OnlyInA(_)));
        assert!(!diff.is_equivalent());
        assert!(diff.summary().preserved_difference);
    }

    #[test]
    fn preserved_order_does_not_matter() {
        let a = canonical("preserved: x1 x2 x3 ;\n");
        let b = canonical("preserved: x3 x1 x2 ;\n");
        let diff = compare_ordered(&a, &b);
        assert!(matches!(diff.preserved, PreservedDiff::Match));
    }

    #[test]
    fn empty_files_are_equivalent() {
        let a = canonical("");
        let b = canonical("");
        let diff = compare_ordered(&a, &b);
        assert!(diff.is_equivalent());
        assert_eq!(diff.summary(), Summary::default());
    }
}
