//! End-to-end comparison test against real fixtures. Verifies that the
//! ordered comparison engine reaches the expected verdict on each
//! `.opb` / `.verifiedopb` pair.

use std::path::PathBuf;

use opbdiff::compare::{ConstraintDiff, compare_ordered};
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
    assert_eq!(s.extra_in_a, 0, "summary = {s:?}");
    assert_eq!(s.extra_in_b, 0, "summary = {s:?}");
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
    assert_eq!(s.extra_in_a, 0);
    assert_eq!(s.extra_in_b, 0);
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
