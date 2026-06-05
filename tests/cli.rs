//! End-to-end CLI tests: spawn the actual binary and check stdout +
//! exit code. Synthetic inputs go through temp files; real fixtures
//! come from `tests/data/`.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
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
fn match_labels_pairs_by_label_regardless_of_order() {
    // Same two labels in opposite order; label-matching pairs them and
    // reports them equivalent without needing --unordered.
    let a = write_tmp("ml_a.opb", "@p 1 x1 >= 1 ;\n@q 1 x2 >= 1 ;\n");
    let b = write_tmp("ml_b.opb", "@q 1 x2 >= 1 ;\n@p 1 x1 >= 1 ;\n");
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg("--match-labels")
        .arg(&a)
        .arg(&b)
        .assert()
        .success()
        .stdout(contains("label-matched"));
}

#[test]
fn match_labels_shows_label_tag_and_content_diff() {
    // Same label, content differs by one variable name. The header
    // carries the shared label tag and the canonical view isolates
    // the difference.
    let a = write_tmp("mld_a.opb", "@edge 1 x1 8 a >= 1 ;\n");
    let b = write_tmp("mld_b.opb", "@edge 1 x1 8 c >= 1 ;\n");
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg("--match-labels")
        .arg(&a)
        .arg(&b)
        .assert()
        .code(1)
        .stdout(contains("[@edge]"))
        .stdout(contains("canonical-form view"));
}

#[test]
fn ignore_aux_names_equates_renamed_aux_variables() {
    // Same projected variable x, aux var named differently on each
    // side. With --ignore-aux-names (and a preserved set on at least
    // one side) the pair is equivalent.
    let a = write_tmp("aux_a.opb", "preserved: x ;\n1 x 8 f >= 1 ;\n");
    let b = write_tmp("aux_b.opb", "preserved: x ;\n1 x 8 g >= 1 ;\n");
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg("--ignore-aux-names")
        .arg(&a)
        .arg(&b)
        .assert()
        .success()
        .stdout(contains("aux names ignored"));
}

#[test]
fn ignore_aux_names_without_any_preserved_set_exits_two() {
    // Neither file has a preserved: line, so there is no basis for
    // deciding what is auxiliary.
    let a = write_tmp("noaux_a.opb", "1 x 8 f >= 1 ;\n");
    let b = write_tmp("noaux_b.opb", "1 x 8 g >= 1 ;\n");
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg("--ignore-aux-names")
        .arg(&a)
        .arg(&b)
        .assert()
        .code(2)
        .stderr(contains("preserved"));
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

/// Run with `--format json`, assert the exit code, and parse stdout as
/// JSON so a malformed payload fails loudly rather than passing a
/// substring check.
fn run_json(args: &[&str], a: &Path, b: &Path) -> (i32, serde_json::Value) {
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg("--format").arg("json");
    for arg in args {
        cmd.arg(arg);
    }
    let output = cmd.arg(a).arg(b).output().expect("run opbdiff");
    let code = output.status.code().expect("process exited normally");
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON ({e}): {:?}", output.stdout));
    (code, value)
}

#[test]
fn json_equivalent_pair_is_marked_equivalent_and_exits_zero() {
    let a = write_tmp("json_eq_a.opb", "1 x1 1 x2 >= 1 ;\n");
    let b = write_tmp("json_eq_b.opb", "+1 ~x2 +1 ~x1 <= 1 ;\n");
    let (code, v) = run_json(&[], &a, &b);
    assert_eq!(code, 0);
    assert_eq!(v["equivalent"], serde_json::Value::Bool(true));
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["constraints"].as_array().unwrap().len(), 0);
}

#[test]
fn json_differing_pair_reports_the_difference_and_exits_one() {
    let a = write_tmp("json_diff_a.opb", "1 x1 >= 1 ;\n");
    let b = write_tmp("json_diff_b.opb", "1 x2 >= 1 ;\n");
    let (code, v) = run_json(&[], &a, &b);
    assert_eq!(code, 1);
    assert_eq!(v["equivalent"], serde_json::Value::Bool(false));
    assert_eq!(v["constraints"][0]["kind"], "differ");
    assert_eq!(v["summary"]["differing"], 1);
}

#[test]
fn json_output_is_never_coloured_even_with_color_always() {
    // JSON bypasses the colour stream, so even `--color=always` must
    // not inject ANSI escapes that would corrupt the payload.
    let a = write_tmp("json_col_a.opb", "1 x1 >= 1 ;\n");
    let b = write_tmp("json_col_b.opb", "1 x2 >= 1 ;\n");
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg("--format=json")
        .arg("--color=always")
        .arg(&a)
        .arg(&b)
        .assert()
        .code(1)
        .stdout(predicates::str::contains("\x1b[").not());
}

#[test]
fn json_fixture_pair_serialises_under_unordered() {
    let (code, v) = run_json(
        &["--unordered"],
        &data("odd_even_sum.opb"),
        &data("odd_even_sum.verifiedopb"),
    );
    assert_eq!(code, 0);
    assert_eq!(v["equivalent"], serde_json::Value::Bool(true));
    assert_eq!(v["comparison"]["mode"], "unordered");
}

#[test]
fn ignore_no_preserved_in_a_makes_missing_line_equivalent() {
    // A has no preserved: line, B does, constraints agree. Without the
    // flag this exits 1; ignoring A's absence makes it equivalent.
    let a = write_tmp("nopres_a.opb", "1 x1 >= 1 ;\n");
    let b = write_tmp("nopres_b.opb", "preserved: x1 ;\n1 x1 >= 1 ;\n");
    run(&a, &b).assert().code(1);
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg("--ignore-no-preserved-in=a")
        .arg(&a)
        .arg(&b)
        .assert()
        .success()
        .stdout(contains("semantically equivalent"))
        .stdout(contains("missing preserved in A ignored"));
}

#[test]
fn ignore_no_preserved_in_wrong_side_still_differs() {
    // The flag names A, but it is B that lacks the line. That stays a
    // difference and exits 1.
    let a = write_tmp("nopres2_a.opb", "preserved: x1 ;\n1 x1 >= 1 ;\n");
    let b = write_tmp("nopres2_b.opb", "1 x1 >= 1 ;\n");
    let mut cmd = Command::cargo_bin("opbdiff").expect("binary built");
    cmd.arg("--ignore-no-preserved-in=a")
        .arg(&a)
        .arg(&b)
        .assert()
        .code(1)
        .stdout(contains("Preserved only in A"));
}

#[test]
fn ignore_no_preserved_in_json_keeps_raw_finding() {
    let a = write_tmp("nopres3_a.opb", "1 x1 >= 1 ;\n");
    let b = write_tmp("nopres3_b.opb", "preserved: x1 ;\n1 x1 >= 1 ;\n");
    let (code, v) = run_json(&["--ignore-no-preserved-in=a"], &a, &b);
    assert_eq!(code, 0);
    assert_eq!(v["equivalent"], serde_json::Value::Bool(true));
    assert_eq!(v["comparison"]["ignored_missing_preserved"], "a");
    assert_eq!(v["preserved"]["kind"], "only_in_b");
}
