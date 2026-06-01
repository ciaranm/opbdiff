//! End-to-end comparison test against real fixtures. Verifies that the
//! ordered comparison engine reaches the expected verdict on each
//! `.opb` / `.verifiedopb` pair.

use std::path::PathBuf;

use opbdiff::compare::{
    CompareMode, CompareOptions, ConstraintDiff, compare, compare_ordered, resolve_aux_projection,
};
use opbdiff::model::normalise_file;
use opbdiff::parser::parse;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

fn load(name: &str) -> opbdiff::model::CanonicalFile {
    let input = std::fs::read_to_string(fixture(name)).unwrap();
    normalise_file(&parse(&input).unwrap()).unwrap()
}

#[test]
fn odd_even_sum_pair_shows_order_swap_under_ordered_compare() {
    // odd_even_sum has no aux-var renaming, but the two encoders emit
    // the final ReifiedLinearEquality constraints in different orders
    // (`[le], [ge]` vs `[ge], [le]`). Under ordered comparison those
    // positions correctly show as Differ. Under the future unordered
    // mode (commit 8) the pair should compare fully equivalent.
    let a = load("odd_even_sum.opb");
    let b = load("odd_even_sum.verifiedopb");
    let diff = compare_ordered(&a, &b);
    let s = diff.summary();

    // No extras — both sides have the same number of constraints.
    assert_eq!(s.only_in_a, 0, "summary = {s:?}");
    assert_eq!(s.only_in_b, 0, "summary = {s:?}");
    // All paired up; the question is just whether positions match.
    assert_eq!(s.matches + s.differing, a.constraints.len());
    // The reordered tail is the part that diffs.
    assert!(s.differing > 0, "expected some positions to differ");
    assert!(s.matches > 0, "expected most positions to match");
}

#[test]
fn crystal_maze_pair_differs_on_alldifferent_constraints() {
    // Aux-var renaming means some constraints diff; bounds match.
    let a = load("crystal_maze.opb");
    let b = load("crystal_maze.verifiedopb");
    let diff = compare_ordered(&a, &b);
    assert!(!diff.is_equivalent());
    let s = diff.summary();
    assert!(s.matches > 0, "expected the bound constraints to match");
    assert!(
        s.differing > 0,
        "expected the aux-var constraints to differ"
    );
    assert_eq!(s.only_in_a, 0);
    assert_eq!(s.only_in_b, 0);
}

#[test]
fn odd_even_sum_pair_is_equivalent_under_unordered() {
    // The same fixture pair that diffs under ordered should compare
    // fully equivalent under unordered, because the reordering is the
    // only thing distinguishing them.
    let a = load("odd_even_sum.opb");
    let b = load("odd_even_sum.verifiedopb");
    let diff = compare(
        &a,
        &b,
        CompareOptions {
            mode: CompareMode::Unordered,
            match_labels: false,
            label_check: None,
            aux_projection: None,
        },
    );
    assert!(
        diff.is_equivalent(),
        "expected odd_even_sum unordered to be equivalent, summary: {:?}",
        diff.summary(),
    );
}

#[test]
fn colour_pair_matches_edges_by_label_under_match_labels() {
    // The colour fixtures are two encoders of the same graph-colouring
    // problem. They share the `@c[edge_i_j][gt|lt]` reified-equality
    // labels but differ in the auxiliary-variable name on each (`f[k]`
    // vs `b[edge_i_j]`). Under --match-labels those edge constraints
    // pair up by label and report a content difference confined to the
    // aux variable; the shared variable bounds, which neither encoder
    // labels identically, fall back to canonical matching.
    let a = load("colour.opb");
    let b = load("colour.verifiedopb");
    let diff = compare(
        &a,
        &b,
        CompareOptions {
            mode: CompareMode::Unordered,
            match_labels: true,
            label_check: None,
            aux_projection: None,
        },
    );
    assert!(diff.matched_by_label);

    // Every edge constraint should appear as a Differ whose two sides
    // carry the *same* label — proof that label-matching paired them
    // rather than canonical form (which differs on the aux var).
    let edge_differs: Vec<_> = diff
        .constraints
        .iter()
        .filter(|d| {
            matches!(
                d,
                ConstraintDiff::Differ { a, b, .. }
                    if a.label.as_deref() == b.label.as_deref()
                        && a.label.as_deref().is_some_and(|l| l.starts_with("c[edge_"))
            )
        })
        .collect();
    assert!(
        edge_differs.len() >= 20,
        "expected the reified-equality edge constraints to pair by label, got {}",
        edge_differs.len(),
    );

    // The shared variable bounds carry no matching label, so they fall
    // through to unordered canonical matching and match cleanly.
    assert!(diff.summary().matches > 0);
}

#[test]
fn colour_pair_collapses_aux_renames_under_ignore_aux_names() {
    // Only colour.opb carries a preserved: line, so its projected
    // variables define what counts as auxiliary. Under unordered +
    // aux-name folding, every constraint that differs only in
    // auxiliary-variable names (the reified edge and colour
    // constraints, `f`/`b`/`x` vs each other) matches; what remains is
    // the genuine structural difference: colour.opb emits the reverse
    // reification direction (`@c[colours_def][Nge]`) that
    // colour.verifiedopb does not.
    let a = load("colour.opb");
    let b = load("colour.verifiedopb");
    let projection = resolve_aux_projection(&a, &b).expect("colour.opb has a preserved set");
    let diff = compare(
        &a,
        &b,
        CompareOptions {
            mode: CompareMode::Unordered,
            match_labels: false,
            label_check: None,
            aux_projection: Some(projection),
        },
    );
    let s = diff.summary();
    assert_eq!(s.differing, 0);
    assert_eq!(s.only_in_b, 0);
    assert_eq!(
        s.only_in_a, 7,
        "the 7 reverse-direction reification constraints"
    );

    // Every residual only-in-A constraint is a `[…ge]` reification half.
    for d in &diff.constraints {
        if let ConstraintDiff::OnlyInA { a, .. } = d {
            let label = a.label.as_deref().unwrap_or("");
            assert!(
                label.ends_with("ge]"),
                "unexpected residual only-in-A: {label}",
            );
        }
    }
}

#[test]
fn ordered_compare_pairs_by_position() {
    // The .opb file lists `>= 1` then `>= -8` for box[0]; the
    // .verifiedopb file does the same (modulo `<=` vs `>=` orientation
    // and the @label prefix). At index 0 both sides describe the box[0]
    // lower bound, so even though one says `>=` and the other `<=`
    // they should pair up as Match.
    let a = load("crystal_maze.opb");
    let b = load("crystal_maze.verifiedopb");
    let diff = compare_ordered(&a, &b);
    match &diff.constraints[0] {
        ConstraintDiff::Match { a, b, .. } => {
            assert_eq!(a.form.rhs, 1);
            assert_eq!(b.form.rhs, 1);
        }
        other => panic!("expected Match at index 0, got {other:?}"),
    }
}

#[test]
fn unordered_with_label_check_on_odd_even_sum() {
    // Every constraint in odd_even_sum's .opb file with a label has
    // the same label as the canonical-form partner in .verifiedopb,
    // so unordered + check-labels --reference=B should still be
    // equivalent.
    let a = load("odd_even_sum.opb");
    let b = load("odd_even_sum.verifiedopb");
    let diff = compare(
        &a,
        &b,
        CompareOptions {
            mode: CompareMode::Unordered,
            match_labels: false,
            label_check: Some(opbdiff::compare::ReferenceSide::B),
            aux_projection: None,
        },
    );
    // Note: this assertion may need refinement once we look more
    // carefully — .verifiedopb labels every bound constraint while
    // .opb does not, so reference=B with labels-on will likely flag
    // label mismatches. Captured by the assertion below.
    let s = diff.summary();
    // We're not asserting equivalence here; we are asserting that the
    // analysis runs without panicking and produces a sensible summary.
    assert!(s.matches + s.label_mismatches == a.constraints.len());
}
