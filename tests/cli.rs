//! End-to-end CLI tests: spawn the actual binary and check stdout +
//! exit code. Synthetic inputs go through temp files; real fixtures
//! come from `tests/data/`.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::str::contains;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("data")
        .join(name)
}

fn write_tmp(name: &str, content: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("opbdiff_test_{}_{name}", std::process::id()));
    std::fs::write(&path, content).expect("write tmp");
    path
}

fn run(a: &Path, b: &Path) -> assert_cmd::Command {
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg(a).arg(b);
    cmd
}

#[test]
fn equivalent_synthetic_pair_exits_zero() {
    let a = write_tmp("equiv_a.opb", "1 x1 1 x2 >= 1 ;\n");
    let b = write_tmp("equiv_b.opb", "+1 ~x2 +1 ~x1 <= 1 ;\n");
    run(&a, &b)
        .assert()
        .success()
        .stdout(contains("semantically equivalent"));
}

#[test]
fn differing_synthetic_pair_exits_one_and_shows_diff() {
    let a = write_tmp("differ_a.opb", "1 x1 >= 1 ;\n");
    let b = write_tmp("differ_b.opb", "1 x2 >= 1 ;\n");
    run(&a, &b)
        .assert()
        .code(1)
        .stdout(contains("Differing at constraint #1"))
        .stdout(contains("Summary (ordered)"));
}

#[test]
fn nonexistent_file_exits_two() {
    let a = std::env::temp_dir().join("opbdiff_does_not_exist_a.opb");
    let b = std::env::temp_dir().join("opbdiff_does_not_exist_b.opb");
    run(&a, &b).assert().code(2).stderr(contains("opbdiff:"));
}

#[test]
fn equality_constraint_is_rejected_with_exit_two() {
    let a = write_tmp("equality_a.opb", "1 x1 = 1 ;\n");
    let b = write_tmp("equality_b.opb", "1 x1 >= 1 ;\n");
    run(&a, &b).assert().code(2).stderr(contains("equality"));
}

#[test]
fn odd_even_sum_fixture_pair_exits_one_under_ordered_mode() {
    // odd_even_sum's last 4 constraints are reordered between the two
    // encoders, so ordered comparison correctly reports differences.
    run(&data("odd_even_sum.opb"), &data("odd_even_sum.verifiedopb"))
        .assert()
        .code(1)
        .stdout(contains("Summary (ordered)"));
}

#[test]
fn odd_even_sum_pair_exits_zero_under_unordered_mode() {
    // Same pair, unordered mode: the reordering no longer matters.
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg("--unordered")
        .arg(data("odd_even_sum.opb"))
        .arg(data("odd_even_sum.verifiedopb"))
        .assert()
        .success()
        .stdout(contains("semantically equivalent"));
}

#[test]
fn check_labels_with_explicit_reference_flag() {
    let a = write_tmp("labels_a.opb", "1 x1 >= 1 ;\n");
    let b = write_tmp("labels_b.opb", "@card 1 x1 >= 1 ;\n");
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg("--check-labels")
        .arg("--reference=b")
        .arg(&a)
        .arg(&b)
        .assert()
        .code(1)
        .stdout(contains("Label mismatch"))
        .stdout(contains("expected label: card"));
}
