//! Confidence test: every OPB fixture in `tests/data/` parses without
//! error and produces at least one item. This is what catches whole
//! categories of "real OPB has something we did not anticipate" bugs.

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

fn assert_parses(name: &str) {
    let path = fixture(name);
    let input = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    let parsed =
        opbdiff::parser::parse(&input).unwrap_or_else(|e| panic!("parsing {name} failed: {e}"));
    assert!(
        !parsed.items.is_empty(),
        "{name} parsed but produced no items",
    );
}

#[test]
fn odd_even_sum_pair_parses() {
    assert_parses("odd_even_sum.opb");
    assert_parses("odd_even_sum.verifiedopb");
}

#[test]
fn money_pair_parses() {
    assert_parses("money.opb");
    assert_parses("money.verifiedopb");
}

#[test]
fn crystal_maze_pair_parses() {
    assert_parses("crystal_maze.opb");
    assert_parses("crystal_maze.verifiedopb");
}

#[test]
fn sudoku_pair_parses() {
    assert_parses("sudoku.opb");
    assert_parses("sudoku.verifiedopb");
}
