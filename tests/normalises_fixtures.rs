//! End-to-end parser + normaliser smoke tests against real fixtures.
//!
//! These are tripwires for parser/normaliser regressions, not a full
//! semantic-equivalence check across the two encoders. Only the
//! `odd_even_sum` pair is expected to normalise identically on both
//! sides; the other three pairs (`crystal_maze`, `money`, `sudoku`)
//! all use different auxiliary-variable names on their AllDifferent
//! decompositions, which (correctly) shows as differing constraints
//! until aux-var renaming lands. See `dev_docs/0004` for the
//! discussion.

use std::collections::HashSet;
use std::path::PathBuf;

use opbdiff::model::{CanonicalConstraint, CanonicalFile, normalise_file};
use opbdiff::parser::parse;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

fn load(name: &str) -> CanonicalFile {
    let path = fixture(name);
    let input =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let ast = parse(&input).unwrap_or_else(|e| panic!("parse {name}: {e}"));
    normalise_file(&ast).unwrap_or_else(|e| panic!("normalise {name}: {e}"))
}

fn shared_constraint_count(a: &CanonicalFile, b: &CanonicalFile) -> usize {
    let set_a: HashSet<&CanonicalConstraint> = a.constraints.iter().map(|c| &c.form).collect();
    b.constraints
        .iter()
        .filter(|c| set_a.contains(&c.form))
        .count()
}

#[test]
fn odd_even_sum_pair_fully_matches_under_normalisation() {
    let a = load("odd_even_sum.opb");
    let b = load("odd_even_sum.verifiedopb");
    assert_eq!(a.constraints.len(), b.constraints.len());
    assert_eq!(
        shared_constraint_count(&a, &b),
        a.constraints.len(),
        "every constraint in odd_even_sum should normalise to the same form on both sides",
    );
}

#[test]
fn crystal_maze_pair_normalises_and_shares_bounds() {
    // We expect the bound constraints on user variables to match but
    // the AllDifferent constraints to differ because of aux-var
    // renaming. Assertion: at least some constraints match.
    let a = load("crystal_maze.opb");
    let b = load("crystal_maze.verifiedopb");
    let shared = shared_constraint_count(&a, &b);
    assert!(shared > 0, "expected some bound constraints to match");
    assert!(
        shared < a.constraints.len(),
        "expected aux-var renaming to leave some constraints unmatched",
    );
}

#[test]
fn sudoku_pair_normalises_and_shares_bounds() {
    let a = load("sudoku.opb");
    let b = load("sudoku.verifiedopb");
    let shared = shared_constraint_count(&a, &b);
    assert!(shared > 0);
    assert!(shared < a.constraints.len());
}

#[test]
fn money_pair_normalises_and_shares_bounds() {
    let a = load("money.opb");
    let b = load("money.verifiedopb");
    let shared = shared_constraint_count(&a, &b);
    assert!(shared > 0);
    assert!(shared < a.constraints.len());
}
